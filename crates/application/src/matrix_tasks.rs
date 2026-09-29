use crate::{
    MatrixEvidenceValidator, MatrixRequirementsLocator, MatrixVerificationStore, TransactionMode,
    WorkspaceService,
};
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
#[derive(Debug, Clone, PartialEq)]
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
    /// Present only on versioned robust-trial receipts. No raw response bytes.
    pub trial_evidence: Option<tect_domain::MatrixTrialRankingEvidence>,
}

#[derive(Debug, Clone, PartialEq)]
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

/// A source revision carries its frozen accepted declaration context when it
/// was recorded through the context-aware route. None denotes historical
/// unbound material and must not be promoted to context-aware authority.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixTaskSource {
    pub revision: MatrixTaskRevision,
    pub requirements_binding: Option<MatrixTaskRequirementsBinding>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixTaskRequirementsBinding {
    pub locator: MatrixRequirementsLocator,
    pub snapshot_id: Uuid,
    pub semantic_digest: String,
    pub authority_schema: String,
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
    /// Record a source bound to accepted declarations at the exact authorized
    /// Program/Scope/logical Work anchor. Replay is checked against the raw
    /// caller request before reading current context, so later owner edits do
    /// not change an already accepted receipt.
    pub async fn record_matrix_task_with_requirements(
        &self,
        context: &RequestContext,
        request: &RecordMatrixTask,
        locator: &MatrixRequirementsLocator,
    ) -> Result<MatrixTaskSource> {
        validate_request(request)?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let (workspace, session) = Self::bound_session(&mut *tx, context, &identity).await?;
        let original_request_digest = canonical_matrix_source_request_digest(request, locator)?;
        if let Some((prior, prior_digest)) = tx
            .matrix_task_source_by_request(workspace.id, request.request_id)
            .await?
        {
            let prior = validated_bound_replay(
                prior,
                &prior_digest,
                &original_request_digest,
                request,
                locator,
            )?;
            tx.commit().await?;
            return Ok(prior);
        }
        let frozen = freeze_locked_matrix_requirements_context(
            tx.matrix_requirements_context_store()
                .ok_or(Error::Forbidden)?,
            workspace.id,
            identity.principal_id,
            locator,
        )
        .await?;
        let bound_input =
            tect_domain::bind_matrix_requirements_input(&frozen.effective, &request.input)?;
        let mut bound_request = request.clone();
        bound_request.input = bound_input;
        validate_request(&bound_request)?;
        let canonical_bound_input =
            serde_json::to_value(&bound_request.input).map_err(|_| Error::InvalidArguments)?;
        let bound_input_digest = canonical_matrix_input_digest(&canonical_bound_input)?;
        let binding = MatrixTaskRequirementsBinding {
            locator: locator.clone(),
            snapshot_id: frozen.id,
            semantic_digest: frozen.effective.semantic_digest().to_owned(),
            authority_schema: frozen.effective.schema().to_owned(),
        };
        let source = tx
            .record_matrix_task_bound(
                workspace.id,
                identity.principal_id,
                session.id,
                crate::BoundMatrixTaskRecord {
                    request: &bound_request,
                    canonical_input: &canonical_bound_input,
                    input_digest: &bound_input_digest,
                    original_request_digest: &original_request_digest,
                    binding: &binding,
                },
            )
            .await?;
        if source.revision.task_id != request.task_id
            || source.revision.revision != request.revision
            || source.revision.request_id != request.request_id
        {
            return Err(Error::InternalInvariant);
        }
        let source = if source.requirements_binding.as_ref() == Some(&binding) {
            if source.revision.input_digest != bound_input_digest {
                return Err(Error::InternalInvariant);
            }
            source
        } else {
            // A concurrent exact retry may have won under the earlier accepted
            // context. Its frozen binding is authoritative only after reading
            // the persisted original-request digest back in this transaction.
            let (prior, prior_digest) = tx
                .matrix_task_source_by_request(workspace.id, request.request_id)
                .await?
                .ok_or(Error::InternalInvariant)?;
            if prior != source {
                return Err(Error::InternalInvariant);
            }
            validated_bound_replay(
                prior,
                &prior_digest,
                &original_request_digest,
                request,
                locator,
            )?
        };
        tx.commit().await?;
        Ok(source)
    }

    pub async fn get_matrix_task_source(
        &self,
        context: &RequestContext,
        task_id: Uuid,
    ) -> Result<MatrixTaskSource> {
        if task_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let source = tx
            .matrix_task_source(workspace.id, task_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(source)
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

/// Lock inherited declarations before freezing the source, keeping those
/// transaction-scoped locks until the caller appends and commits the task.
pub(crate) async fn freeze_locked_matrix_requirements_context(
    store: &mut dyn crate::MatrixRequirementsContextStore,
    workspace_id: Uuid,
    principal_id: Uuid,
    locator: &MatrixRequirementsLocator,
) -> Result<crate::FrozenMatrixRequirementsContext> {
    let lineage = store
        .matrix_requirements_lineage(workspace_id, principal_id, locator, true)
        .await?;
    for anchor in lineage {
        store
            .lock_matrix_requirements_head(workspace_id, anchor)
            .await?;
    }
    crate::freeze_effective_matrix_requirements_context(store, workspace_id, principal_id, locator)
        .await
}

mod advisory;
mod binding;
mod verified_cards;
#[cfg(test)]
use binding::compose_current_revision;
pub(crate) use binding::{
    compose_bound_revision_with_verification, compose_current_revision_with_validated_verification,
    compose_current_revision_with_verification, current_public_matrix_advice,
    matrix_advisory_opportunity_input,
};
use binding::{
    matrix_advisory_receipt_matches, matrix_advisory_replay_matches, valid_advisory_request_key,
};
pub use verified_cards::{
    GetVerifiedMatrixCards, VERIFIED_MATRIX_CARDS_SCHEMA, VerifiedMatrixCardSummary,
    VerifiedMatrixCards,
};

/// Hash the same canonical JSON representation that the store persists.
pub fn canonical_matrix_input_digest(input: &serde_json::Value) -> Result<String> {
    let encoded = serde_json::to_vec(input).map_err(|_| Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(encoded)))
}

pub fn canonical_matrix_source_request_digest(
    request: &RecordMatrixTask,
    locator: &MatrixRequirementsLocator,
) -> Result<String> {
    let raw = serde_json::json!({
        "task_id": request.task_id,
        "revision": request.revision,
        "expected_current_revision": request.expected_current_revision,
        "request_id": request.request_id,
        "input": request.input,
        "choice_set": request.choice_set,
        "requirements_locator": locator.as_json(),
    });
    canonical_matrix_input_digest(&raw)
}

fn validated_bound_replay(
    prior: MatrixTaskSource,
    prior_digest: &str,
    requested_digest: &str,
    request: &RecordMatrixTask,
    locator: &MatrixRequirementsLocator,
) -> Result<MatrixTaskSource> {
    if prior_digest != requested_digest
        || prior.revision.task_id != request.task_id
        || prior.revision.revision != request.revision
        || prior.revision.request_id != request.request_id
        || prior
            .requirements_binding
            .as_ref()
            .map(|binding| &binding.locator)
            != Some(locator)
    {
        return Err(Error::InputConflict);
    }
    Ok(prior)
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
