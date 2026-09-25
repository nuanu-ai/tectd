use crate::{MatrixEvidenceValidator, MatrixVerificationStore, TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use tect_domain::{
    ADVISORY_DECISION_POINT_VERSION, ADVISORY_POLICY_VERSION, AdvisoryCapability,
    AdvisoryDecisionPoint, AdvisoryOpportunity, AdvisoryOpportunityInput, AdvisoryOpportunityState,
    AdvisoryReason, AdvisoryRequestPreference, EngineeringChoiceSet, EngineeringMatrixComposition,
    EngineeringMatrixInput, Error, MatrixAdviceEligibility, MatrixSourceVerificationStatus,
    OwnerReportedEngineeringMatrixFacts, RequestContext, Result, WorkspaceAdvisoryConfig,
    WorkspaceAdvisoryMode, compose_independently_verified_owner_matrix,
    compose_owner_reported_engineering_matrix, evaluate_matrix_verification, required_matrix_facts,
};
use uuid::Uuid;

/// Public projection of advice that still matches the current Matrix head.
/// Provider transport bytes deliberately have no place in this model.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct CurrentMatrixAdvice {
    pub advice_id: Uuid,
    pub dispatch_id: Uuid,
    pub task_revision: i64,
    pub input_digest: String,
    pub choice_set_id: String,
    pub choice_set_version: u64,
    pub choice_set_digest: String,
    pub evaluation_digest: String,
    pub verification_digest: String,
    pub provider_profile_ref: tect_domain::AdvisoryProviderProfileRef,
    pub model_configuration: tect_domain::AdvisoryModelConfiguration,
    pub response_payload_sha256: String,
    pub advice_digest: String,
    pub outcome: crate::GuardedMatrixAdviceOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EngineeringAdvisoryRead {
    pub opportunity: AdvisoryOpportunity,
    pub current_advice: Option<CurrentMatrixAdvice>,
}

pub const MATRIX_INPUT_SCHEMA: &str = "tect.engineering-matrix-input/1";

/// The requested revision is exact: a new task starts at 1 and each edit
/// must name the immediate successor of the accepted revision.
#[derive(Debug, Clone)]
pub struct RecordMatrixTask {
    pub task_id: Uuid,
    pub revision: i64,
    pub expected_current_revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
    pub choice_set: Option<EngineeringChoiceSet>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixTaskRevision {
    pub task_id: Uuid,
    pub revision: i64,
    pub request_id: Uuid,
    pub input: EngineeringMatrixInput,
    pub input_digest: String,
    pub choice_set: Option<EngineeringChoiceSet>,
    pub choice_set_digest: Option<String>,
    pub recorded_by_principal_id: Uuid,
    pub recorded_by_session_id: Uuid,
}

/// Captures a Matrix advisory decision at the current saved task revision.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct RequestEngineeringAdvisory {
    pub task_id: Uuid,
    pub expected_task_revision: i64,
    pub request_key: String,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
}

impl WorkspaceService {
    /// Read a saved Matrix advisory receipt, including historical no-call
    /// decisions, for an authenticated member of the receipt's workspace.
    pub async fn get_engineering_advisory(
        &self,
        context: &RequestContext,
        task_id: Uuid,
        request_key: &str,
    ) -> Result<EngineeringAdvisoryRead> {
        if task_id.is_nil() || !valid_advisory_request_key(request_key) {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let receipt = tx
            .advisory_opportunity_by_request(workspace.id, request_key)
            .await?
            .ok_or(Error::NotFound)?;
        if !matrix_advisory_receipt_matches(&receipt, workspace.id, task_id, request_key) {
            return Err(Error::NotFound);
        }
        let advice = if receipt.state == AdvisoryOpportunityState::Advised {
            tx.guarded_matrix_advice(workspace.id, receipt.id).await?
        } else {
            None
        };
        let current_advice = if let Some(advice) = advice {
            let current = tx.matrix_task(workspace.id, task_id).await?;
            let config = tx.advisory_config(workspace.id).await?;
            let fresh = if let Some(current) =
                current.filter(|current| current.revision == advice.record.binding.task_revision)
            {
                let validated = compose_current_revision_with_validated_verification(
                    tx.matrix_verification_store(),
                    self.matrix_evidence_validator.as_ref(),
                    workspace.id,
                    current.clone(),
                    advice.record.binding.task_revision,
                    crate::matrix_verification::current_epoch_seconds()?,
                )
                .await;
                validated.ok().and_then(|(composition, verification)| {
                    verification.and_then(|verification| {
                        crate::MatrixProviderRequest::new_verified(
                            current,
                            composition,
                            &verification,
                            advice.record.provider_profile_ref.clone(),
                            advice.record.model_configuration.clone(),
                        )
                        .ok()
                    })
                })
            } else {
                None
            };
            current_public_matrix_advice(
                &receipt,
                &advice,
                &config,
                fresh.as_ref().map(|request| request.binding()),
            )
        } else {
            None
        };
        tx.commit().await?;
        Ok(EngineeringAdvisoryRead {
            opportunity: receipt,
            current_advice,
        })
    }

    pub async fn request_engineering_advisory(
        &self,
        context: &RequestContext,
        request: &RequestEngineeringAdvisory,
    ) -> Result<AdvisoryOpportunity> {
        if request.task_id.is_nil()
            || request.expected_task_revision < 1
            || !valid_advisory_request_key(&request.request_key)
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let existing = tx
            .advisory_opportunity_by_request(workspace.id, &request.request_key)
            .await?;
        if let Some(existing) = existing {
            if !matrix_advisory_replay_matches(
                &existing,
                request,
                session.id,
                identity.principal_id,
            ) {
                return Err(Error::InputConflict);
            }
            let saved = if matches!(
                existing.state,
                AdvisoryOpportunityState::Prepared | AdvisoryOpportunityState::AwaitingResponse
            ) {
                Some(
                    tx.matrix_dispatch_for_recovery(
                        &crate::AdvisoryLifecycleCapability::internal(),
                        workspace.id,
                        identity.principal_id,
                        existing.id,
                        None,
                    )
                    .await?,
                )
            } else {
                None
            };
            tx.commit().await?;
            return match saved {
                Some(saved) => {
                    self.recover_matrix_advisory(context, workspace.id, existing, saved)
                        .await
                }
                None => Ok(existing),
            };
        }
        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if revision.revision != request.expected_task_revision {
            return Err(Error::StaleRevision);
        }
        let config = tx.advisory_config(workspace.id).await?;
        let (composition, verification) = compose_current_revision_with_validated_verification(
            tx.matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace.id,
            revision.clone(),
            request.expected_task_revision,
            crate::matrix_verification::current_epoch_seconds()?,
        )
        .await?;
        let mut input = matrix_advisory_opportunity_input(
            &revision,
            request,
            &config,
            session.id,
            identity.principal_id,
        )?;
        // A verified no-call (for example, explicit optional-JEV skip) must retain
        // the exact evidence and mandatory-card snapshot. Historical v1 no-call
        // receipts are immutable and retain their original material digest.
        if let (Some(verification), Some(choice_set)) = (
            verification.as_ref().filter(|_| composition.is_resolved()),
            revision.choice_set.as_ref(),
        ) {
            input.material_digest =
                verification.disposition_digest(&revision.input, &composition, choice_set)?;
            input.matrix_verification_digest = Some(verification.record_digest().to_owned());
            input.validate()?;
        }
        let mut provider_request = None;
        if matches!(
            input.primary_reason,
            AdvisoryReason::MatrixSourceUnverified | AdvisoryReason::MatrixEvidenceUnresolved
        ) && let Some(verification) = verification.as_ref()
        {
            if !composition.unresolved_evidence.is_empty() {
                input.primary_reason = AdvisoryReason::MatrixEvidenceUnresolved;
            } else if !composition.is_resolved() {
                input.primary_reason = AdvisoryReason::MatrixSourceUnverified;
            } else if let (Some(profile), Some(model)) = (
                config.provider_profile_ref.clone(),
                config.model_configuration.clone(),
            ) {
                let verified = crate::MatrixProviderRequest::new_verified(
                    revision.clone(),
                    composition,
                    verification,
                    profile,
                    model,
                )?;
                input.primary_reason = AdvisoryReason::CapabilityUnavailable;
                provider_request = Some(verified);
            } else {
                input.primary_reason = AdvisoryReason::ProviderUnconfigured;
            }
        }
        let prepared = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut input,
            provider_request.as_ref(),
            workspace.id,
            identity.principal_id,
            self.matrix_advice_provider.as_ref(),
            self.matrix_budget.as_ref(),
        )
        .await?;
        let opportunity = tx
            .capture_advisory_opportunity(workspace.id, &input)
            .await?;
        let crate::matrix_advisory_capture::PreparedMatrixOpportunity::Authorized {
            prepared,
            authorization: budget,
        } = prepared
        else {
            tx.commit().await?;
            return Ok(opportunity);
        };
        let provider_request = provider_request.ok_or(Error::InternalInvariant)?;
        let prepared = *prepared;
        let authorization = crate::matrix_advisory_dispatch::authorize_prepared_matrix(
            opportunity.id,
            &opportunity,
            &prepared,
            &budget,
        )?;
        let lifecycle = crate::AdvisoryLifecycleCapability::internal();
        tx.authorize_advisory_dispatch(&lifecycle, workspace.id, config.revision, &authorization)
            .await?;
        tx.commit().await?;
        self.dispatch_prepared_matrix_advisory(
            context,
            workspace.id,
            crate::matrix_advisory_dispatch::PreparedMatrixDispatch {
                opportunity,
                config_revision: config.revision,
                authorization,
                provider_request,
                prepared,
            },
        )
        .await
    }

    pub async fn record_matrix_task(
        &self,
        context: &RequestContext,
        request: &RecordMatrixTask,
    ) -> Result<MatrixTaskRevision> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        validate_request(request)?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let canonical_input =
            serde_json::to_value(&request.input).map_err(|_| Error::InvalidArguments)?;
        let input_digest = canonical_matrix_input_digest(&canonical_input)?;
        let revision = tx
            .record_matrix_task(
                workspace.id,
                identity.principal_id,
                session.id,
                request,
                &canonical_input,
                &input_digest,
            )
            .await?;
        tx.commit().await?;
        Ok(revision)
    }

    pub async fn get_matrix_task(
        &self,
        context: &RequestContext,
        task_id: Uuid,
    ) -> Result<MatrixTaskRevision> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if task_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let revision = tx
            .matrix_task(workspace.id, task_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(revision)
    }

    /// Compose cards from the accepted current task revision visible to this
    /// authenticated workspace member. This does not make a release decision.
    pub async fn compose_matrix_cards(
        &self,
        context: &RequestContext,
        task_id: Uuid,
        expected_task_revision: i64,
    ) -> Result<EngineeringMatrixComposition> {
        if task_id.is_nil() || expected_task_revision < 1 {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let revision = tx
            .matrix_task(workspace.id, task_id)
            .await?
            .ok_or(Error::NotFound)?;
        let (composition, _) = compose_current_revision_with_verification(
            tx.matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace.id,
            revision,
            expected_task_revision,
            crate::matrix_verification::current_epoch_seconds()?,
        )
        .await?;
        tx.commit().await?;
        Ok(composition)
    }
}

mod binding;
#[cfg(test)]
use binding::compose_current_revision;
pub(crate) use binding::{
    compose_current_revision_with_validated_verification,
    compose_current_revision_with_verification, current_public_matrix_advice,
    matrix_advisory_opportunity_input, matrix_request_still_current,
};
use binding::{
    matrix_advisory_receipt_matches, matrix_advisory_replay_matches, valid_advisory_request_key,
};

/// Hash the same canonical JSON representation that the store persists.
pub fn canonical_matrix_input_digest(input: &serde_json::Value) -> Result<String> {
    let encoded = serde_json::to_vec(input).map_err(|_| Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

fn validate_request(request: &RecordMatrixTask) -> Result<()> {
    if request.task_id.is_nil()
        || request.request_id.is_nil()
        || request.revision < 1
        || request.expected_current_revision != request.revision - 1
    {
        return Err(Error::InvalidArguments);
    }
    request.input.validate()?;
    if let Some(choice_set) = &request.choice_set {
        if choice_set.task_id != request.task_id.to_string()
            || choice_set.task_revision != request.revision.to_string()
        {
            return Err(Error::InvalidArguments);
        }
        choice_set.validate(&request.input)?;
    }
    Ok(())
}

#[cfg(test)]
#[path = "matrix_tasks/tests.rs"]
mod tests;
