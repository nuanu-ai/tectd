use crate::{TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use tect_domain::{
    ADVISORY_DECISION_POINT_VERSION, ADVISORY_POLICY_VERSION, AdvisoryCapability,
    AdvisoryDecisionPoint, AdvisoryOpportunity, AdvisoryOpportunityInput, AdvisoryOpportunityState,
    AdvisoryReason, AdvisoryRequestPreference, EngineeringChoiceSet, EngineeringMatrixComposition,
    EngineeringMatrixInput, Error, MatrixAdviceEligibility, OwnerReportedEngineeringMatrixFacts,
    RequestContext, Result, WorkspaceAdvisoryConfig, WorkspaceAdvisoryMode,
    compose_owner_reported_engineering_matrix,
};
use uuid::Uuid;

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

/// Records a decision at the current saved Matrix task revision. Preparation
/// and budget authorization are pure; this request never sends to a provider.
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
    ) -> Result<AdvisoryOpportunity> {
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
        tx.commit().await?;
        Ok(receipt)
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
        if let Some(existing) = existing.as_ref() {
            if !matrix_advisory_replay_matches(existing, request, session.id, identity.principal_id)
            {
                return Err(Error::InputConflict);
            }
        }
        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if existing.is_some() && revision.revision != request.expected_task_revision {
            return Err(Error::InputConflict);
        }
        if existing.is_none() && revision.revision != request.expected_task_revision {
            return Err(Error::StaleRevision);
        }
        let config = tx.advisory_config(workspace.id).await?;
        let mut input = matrix_advisory_opportunity_input(
            &revision,
            request,
            &config,
            session.id,
            identity.principal_id,
        )?;
        if let Some(existing) = existing {
            if existing.material_digest != input.material_digest
                || existing.config_revision != input.config_revision
                || existing.matrix_choice_set_digest != input.matrix_choice_set_digest
            {
                return Err(Error::InputConflict);
            }
            tx.commit().await?;
            return Ok(existing);
        }
        crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut input,
            &revision,
            &config,
            workspace.id,
            identity.principal_id,
            self.matrix_advice_provider.as_ref(),
            self.matrix_budget.as_ref(),
        )
        .await?;
        let result = tx
            .capture_advisory_opportunity(workspace.id, &input)
            .await?;
        tx.commit().await?;
        Ok(result)
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
        let revision = self.get_matrix_task(context, task_id).await?;
        compose_current_revision(revision, expected_task_revision)
    }
}

fn valid_advisory_request_key(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0') && value.trim() == value
}

fn matrix_advisory_receipt_matches(
    receipt: &AdvisoryOpportunity,
    workspace_id: Uuid,
    task_id: Uuid,
    request_key: &str,
) -> bool {
    receipt.workspace_id == workspace_id
        && receipt.capability == AdvisoryCapability::EngineeringProfile
        && receipt.decision_point == AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        && receipt.target_kind == "matrix_task"
        && receipt.target_id == Some(task_id)
        && receipt
            .matrix_task_revision
            .is_some_and(|revision| revision > 0)
        && receipt.matrix_task_revision == receipt.work_revision
        && receipt.workflow_occurrence_key == request_key
}

fn matrix_advisory_replay_matches(
    existing: &AdvisoryOpportunity,
    request: &RequestEngineeringAdvisory,
    session_id: Uuid,
    actor_id: Uuid,
) -> bool {
    existing.capability == AdvisoryCapability::EngineeringProfile
        && existing.decision_point == AdvisoryDecisionPoint::EngineeringProfileBeforeSelection
        && existing.target_kind == "matrix_task"
        && existing.target_id == Some(request.task_id)
        && existing.matrix_task_revision == Some(request.expected_task_revision)
        && existing.work_revision == Some(request.expected_task_revision)
        && existing.workflow_occurrence_key == request.request_key
        && existing.session_id == session_id
        && existing.authorized_actor_id == actor_id
        && existing.session_preference == request.session_preference
        && existing.request_preference == request.request_preference
        && matches!(
            existing.state,
            AdvisoryOpportunityState::NoCall
                | AdvisoryOpportunityState::Prepared
                | AdvisoryOpportunityState::Advised
                | AdvisoryOpportunityState::Failed
        )
}

fn matrix_advisory_opportunity_input(
    revision: &MatrixTaskRevision,
    request: &RequestEngineeringAdvisory,
    config: &WorkspaceAdvisoryConfig,
    session_id: Uuid,
    actor_id: Uuid,
) -> Result<AdvisoryOpportunityInput> {
    let composition = compose_current_revision(revision.clone(), request.expected_task_revision)?;
    let eligibility = revision
        .choice_set
        .as_ref()
        .map(|choice| choice.validate(&revision.input))
        .transpose()?
        .unwrap_or(MatrixAdviceEligibility::NotApplicable);
    let reason = if config.mode == WorkspaceAdvisoryMode::Disabled {
        AdvisoryReason::WorkspaceDisabled
    } else if request.session_preference == AdvisoryRequestPreference::Skip {
        AdvisoryReason::SessionSkip
    } else if request.request_preference == AdvisoryRequestPreference::Skip {
        AdvisoryReason::RequestSkip
    } else if eligibility == MatrixAdviceEligibility::NotApplicable {
        AdvisoryReason::ChoiceSetNotApplicable
    } else {
        AdvisoryReason::CapabilityUnavailable
    };
    let evaluation_digest = revision
        .choice_set
        .as_ref()
        .filter(|_| {
            matches!(
                eligibility,
                MatrixAdviceEligibility::EligibleForAdvice { .. }
            )
        })
        .map(|choice| tect_domain::matrix_evaluation_digest(&revision.input, &composition, choice))
        .transpose()?;
    let material = serde_json::to_vec(&(
        "tect.matrix-advisory-opportunity/1",
        revision.task_id,
        revision.revision,
        &revision.input_digest,
        &revision.choice_set_digest,
        &evaluation_digest,
        &composition,
        config,
        request.session_preference,
        request.request_preference,
        ADVISORY_POLICY_VERSION,
    ))
    .map_err(|_| Error::InternalInvariant)?;
    let input = AdvisoryOpportunityInput {
        session_id,
        authorized_actor_id: actor_id,
        capability: AdvisoryCapability::EngineeringProfile,
        decision_point: AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
        decision_point_version: ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: request.request_key.clone(),
        target_kind: "matrix_task".into(),
        target_id: Some(revision.task_id),
        work_revision: Some(revision.revision),
        matrix_task_revision: Some(revision.revision),
        matrix_choice_set_digest: revision.choice_set_digest.clone(),
        source_ref: None,
        session_preference: request.session_preference,
        request_preference: request.request_preference,
        config_revision: config.revision,
        material_digest: format!("{:x}", Sha256::digest(material)),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: reason,
    };
    input.validate()?;
    Ok(input)
}

pub(crate) fn compose_current_revision(
    revision: MatrixTaskRevision,
    expected_task_revision: i64,
) -> Result<EngineeringMatrixComposition> {
    if expected_task_revision < 1 {
        return Err(Error::InvalidArguments);
    }
    if revision.revision != expected_task_revision {
        return Err(Error::StaleRevision);
    }
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        revision.task_id.to_string(),
        revision.revision.to_string(),
        revision.input,
    )?;
    Ok(compose_owner_reported_engineering_matrix(&reported))
}

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
mod tests {
    use super::*;
    use crate::{
        MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
        MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
        MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt,
    };
    use tect_domain::{
        CommitmentEvidence, EngineeringCandidate, EngineeringIntent, EngineeringMode,
        FactProvenance, MATRIX_CHOICE_SET_SCHEMA, MatrixFact, MatrixSourceVerificationStatus,
        OperatingEnvelope, OperatingFact, OperationalFacts,
    };

    fn known<T>(value: T) -> MatrixFact<T> {
        MatrixFact::Known {
            value,
            provenance: FactProvenance("owner report".into()),
        }
    }

    fn stored_revision() -> MatrixTaskRevision {
        let task_id = Uuid::new_v4();
        let input = EngineeringMatrixInput {
            mode: MatrixFact::Absent,
            envelope: OperatingEnvelope {
                scale: MatrixFact::Absent,
                operational_facts: OperationalFacts::Absent,
            },
            criticality: MatrixFact::Absent,
            intent: MatrixFact::Absent,
            urgency: MatrixFact::Absent,
            promised_behavior: MatrixFact::Absent,
            promised_proof: MatrixFact::Absent,
            affected_guarantees: MatrixFact::Absent,
            actual_exposure: MatrixFact::Absent,
            demand_commitment: MatrixFact::Absent,
            latency_commitment: MatrixFact::Absent,
            urgent_repair: MatrixFact::Absent,
        };
        MatrixTaskRevision {
            task_id,
            revision: 2,
            request_id: Uuid::new_v4(),
            input,
            input_digest: "stored-digest".into(),
            choice_set: None,
            choice_set_digest: None,
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
        }
    }

    fn advisory_request(task_id: Uuid) -> RequestEngineeringAdvisory {
        RequestEngineeringAdvisory {
            task_id,
            expected_task_revision: 2,
            request_key: Uuid::new_v4().to_string(),
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
        }
    }

    fn advisory_config(mode: WorkspaceAdvisoryMode) -> WorkspaceAdvisoryConfig {
        WorkspaceAdvisoryConfig {
            workspace_id: Uuid::new_v4(),
            revision: 1,
            mode,
            materialized: true,
            provider_profile_ref: None,
            model_configuration: None,
        }
    }

    struct TestProvider(MatrixProviderIdentity);

    #[async_trait::async_trait]
    impl MatrixAdviceProvider for TestProvider {
        fn identity(&self) -> Option<MatrixProviderIdentity> {
            Some(self.0.clone())
        }

        fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
            PreparedMatrixAdviceAttempt::new(request, self.0.clone(), b"{}".to_vec())
        }

        async fn attempt_prepared(
            &self,
            _: PreparedMatrixAdviceAttempt,
            _: MatrixStartedDispatchPermit,
        ) -> Result<MatrixProviderResponse> {
            panic!("request capture must not send")
        }
    }

    struct TestBudget(bool);

    #[async_trait::async_trait]
    impl MatrixBudgetPolicy for TestBudget {
        async fn authorize(
            &self,
            _: &MatrixBudgetRequest,
        ) -> Result<Option<MatrixBudgetAuthorization>> {
            Ok(self.0.then(|| MatrixBudgetAuthorization {
                policy_id: "test-policy".into(),
            }))
        }
    }

    #[tokio::test]
    async fn eligible_matrix_prepares_only_after_budget_authorizes() {
        let mut revision = stored_revision();
        revision.input_digest =
            canonical_matrix_input_digest(&serde_json::to_value(&revision.input).unwrap()).unwrap();
        let choice = EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "choice-1".into(),
            version: 1,
            task_id: revision.task_id.to_string(),
            task_revision: revision.revision.to_string(),
            decision_question: "Which approach?".into(),
            candidates: ["a", "b"]
                .into_iter()
                .map(|id| EngineeringCandidate {
                    candidate_id: id.into(),
                    title: id.into(),
                    approach: id.into(),
                    assumption_fact_ids: vec![],
                })
                .collect(),
        };
        revision.choice_set_digest = Some(choice.canonical_digest(&revision.input).unwrap());
        revision.choice_set = Some(choice);
        let request = advisory_request(revision.task_id);
        let mut config = advisory_config(WorkspaceAdvisoryMode::Optional);
        let identity = MatrixProviderIdentity {
            provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
                id: "test-profile".into(),
            },
            model_configuration: tect_domain::AdvisoryModelConfiguration {
                model: "test-model".into(),
            },
            destination: "test-destination".into(),
            wire_version: "test-wire/1".into(),
        };
        config.provider_profile_ref = Some(identity.provider_profile_ref.clone());
        config.model_configuration = Some(identity.model_configuration.clone());
        let mut input = matrix_advisory_opportunity_input(
            &revision,
            &request,
            &config,
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .unwrap();
        let provider = TestProvider(identity);
        let actor_id = input.authorized_actor_id;
        crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut input,
            &revision,
            &config,
            config.workspace_id,
            actor_id,
            &provider,
            &TestBudget(true),
        )
        .await
        .unwrap();
        assert_eq!(input.state, AdvisoryOpportunityState::Prepared);
        assert_eq!(input.primary_reason, AdvisoryReason::DispatchAuthorized);
        let mut denied = matrix_advisory_opportunity_input(
            &revision,
            &request,
            &config,
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .unwrap();
        let actor_id = denied.authorized_actor_id;
        crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut denied,
            &revision,
            &config,
            config.workspace_id,
            actor_id,
            &provider,
            &TestBudget(false),
        )
        .await
        .unwrap();
        assert_eq!(denied.state, AdvisoryOpportunityState::NoCall);
        assert_eq!(denied.primary_reason, AdvisoryReason::BudgetPolicyInvalid);
    }

    #[test]
    fn matrix_no_call_precedence_and_digest_bind_stored_material() {
        let mut revision = stored_revision();
        let mut request = advisory_request(revision.task_id);
        let session = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let mut config = advisory_config(WorkspaceAdvisoryMode::Disabled);
        let disabled =
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap();
        assert_eq!(disabled.primary_reason, AdvisoryReason::WorkspaceDisabled);
        config.mode = WorkspaceAdvisoryMode::Optional;
        request.session_preference = AdvisoryRequestPreference::Skip;
        assert_eq!(
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap()
                .primary_reason,
            AdvisoryReason::SessionSkip
        );
        request.session_preference = AdvisoryRequestPreference::UseWorkspace;
        request.request_preference = AdvisoryRequestPreference::Skip;
        assert_eq!(
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap()
                .primary_reason,
            AdvisoryReason::RequestSkip
        );
        request.request_preference = AdvisoryRequestPreference::UseWorkspace;
        let absent =
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap();
        assert_eq!(
            absent.primary_reason,
            AdvisoryReason::ChoiceSetNotApplicable
        );
        assert_eq!(absent.matrix_task_revision, Some(2));
        assert_eq!(absent.matrix_choice_set_digest, None);

        let choice = EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "choice-1".into(),
            version: 1,
            task_id: revision.task_id.to_string(),
            task_revision: "2".into(),
            decision_question: "Which approach?".into(),
            candidates: ["a", "b"]
                .into_iter()
                .map(|id| EngineeringCandidate {
                    candidate_id: id.into(),
                    title: id.into(),
                    approach: id.into(),
                    assumption_fact_ids: vec![],
                })
                .collect(),
        };
        revision.choice_set_digest = Some(choice.canonical_digest(&revision.input).unwrap());
        revision.choice_set = Some(choice);
        let eligible =
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap();
        assert_eq!(
            eligible.primary_reason,
            AdvisoryReason::CapabilityUnavailable
        );
        assert_eq!(eligible.state, AdvisoryOpportunityState::NoCall);
        assert_ne!(eligible.material_digest, absent.material_digest);
        revision.input_digest = "changed-input".into();
        assert_ne!(
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap()
                .material_digest,
            eligible.material_digest
        );
        config.revision += 1;
        assert_ne!(
            matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
                .unwrap()
                .material_digest,
            eligible.material_digest
        );
    }

    #[test]
    fn matrix_no_call_replay_binds_actor_session_and_request() {
        let revision = stored_revision();
        let request = advisory_request(revision.task_id);
        let config = advisory_config(WorkspaceAdvisoryMode::Optional);
        let session = Uuid::new_v4();
        let actor = Uuid::new_v4();
        let input = matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
            .unwrap();
        let receipt = AdvisoryOpportunity {
            id: Uuid::new_v4(),
            workspace_id: config.workspace_id,
            session_id: session,
            authorized_actor_id: actor,
            capability: input.capability,
            decision_point: input.decision_point,
            decision_point_version: input.decision_point_version,
            workflow_occurrence_key: input.workflow_occurrence_key,
            target_kind: input.target_kind,
            target_id: input.target_id,
            work_revision: input.work_revision,
            matrix_task_revision: input.matrix_task_revision,
            matrix_choice_set_digest: input.matrix_choice_set_digest,
            source_ref: None,
            session_preference: input.session_preference,
            request_preference: input.request_preference,
            config_revision: input.config_revision,
            material_digest: input.material_digest,
            state: input.state,
            primary_reason: input.primary_reason,
            provider_called: false,
        };
        assert!(matrix_advisory_receipt_matches(
            &receipt,
            config.workspace_id,
            request.task_id,
            &request.request_key,
        ));
        assert!(!matrix_advisory_receipt_matches(
            &receipt,
            Uuid::new_v4(),
            request.task_id,
            &request.request_key,
        ));
        assert!(!matrix_advisory_receipt_matches(
            &receipt,
            config.workspace_id,
            Uuid::new_v4(),
            &request.request_key,
        ));
        assert!(!matrix_advisory_receipt_matches(
            &receipt,
            config.workspace_id,
            request.task_id,
            "different-key",
        ));
        let mut wrong_kind = receipt.clone();
        wrong_kind.capability = AdvisoryCapability::ScopeDecomposition;
        assert!(!matrix_advisory_receipt_matches(
            &wrong_kind,
            config.workspace_id,
            request.task_id,
            &request.request_key,
        ));
        let mut wrong_revision = receipt.clone();
        wrong_revision.matrix_task_revision = Some(3);
        assert!(!matrix_advisory_receipt_matches(
            &wrong_revision,
            config.workspace_id,
            request.task_id,
            &request.request_key,
        ));
        assert!(matrix_advisory_replay_matches(
            &receipt, &request, session, actor
        ));
        for state in [
            AdvisoryOpportunityState::Prepared,
            AdvisoryOpportunityState::Advised,
            AdvisoryOpportunityState::Failed,
        ] {
            let mut progressed = receipt.clone();
            progressed.state = state;
            progressed.provider_called = true;
            assert!(matrix_advisory_replay_matches(
                &progressed,
                &request,
                session,
                actor
            ));
        }
        assert!(!matrix_advisory_replay_matches(
            &receipt,
            &request,
            session,
            Uuid::new_v4()
        ));
        assert!(!matrix_advisory_replay_matches(
            &receipt,
            &request,
            Uuid::new_v4(),
            actor
        ));
        let mut changed = request.clone();
        changed.expected_task_revision += 1;
        assert!(!matrix_advisory_replay_matches(
            &receipt, &changed, session, actor
        ));
        changed = request.clone();
        changed.request_preference = AdvisoryRequestPreference::Skip;
        assert!(!matrix_advisory_replay_matches(
            &receipt, &changed, session, actor
        ));
    }

    #[test]
    fn composition_uses_exact_stored_revision_and_keeps_gaps_visible() {
        let stored = stored_revision();
        let output = compose_current_revision(stored.clone(), 2).unwrap();
        assert_eq!(output.task_id, stored.task_id.to_string());
        assert_eq!(output.task_revision, "2");
        assert_eq!(output.mandatory_cards[0].id, "EM02-SCOPE@0.1");
        assert!(!output.is_resolved());
    }

    #[test]
    fn complete_stored_demo_remains_pending_independent_verification() {
        let mut stored = stored_revision();
        stored.input = EngineeringMatrixInput {
            mode: known(EngineeringMode::Demo),
            envelope: OperatingEnvelope {
                scale: known("one synthetic request".into()),
                operational_facts: OperationalFacts::Reported {
                    entries: vec![OperatingFact {
                        name: "environment".into(),
                        fact: known("synthetic".into()),
                    }],
                },
            },
            criticality: known("no protected guarantee".into()),
            intent: known(EngineeringIntent::Other("demo".into())),
            urgency: known("ordinary".into()),
            promised_behavior: known("real demo".into()),
            promised_proof: known("demo check".into()),
            affected_guarantees: MatrixFact::KnownEmpty {
                provenance: FactProvenance("owner report".into()),
            },
            actual_exposure: known(false),
            demand_commitment: known(CommitmentEvidence::NoCommitment),
            latency_commitment: known(CommitmentEvidence::NoCommitment),
            urgent_repair: known(false),
        };
        let output = compose_current_revision(stored, 2).unwrap();
        assert!(output.unresolved_evidence.is_empty());
        assert_eq!(
            output.source_verification_status,
            MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
        );
        assert!(!output.is_resolved());
    }

    #[test]
    fn composition_rejects_stale_or_invalid_expected_revision() {
        let stored = stored_revision();
        assert_eq!(
            compose_current_revision(stored.clone(), 1),
            Err(Error::StaleRevision)
        );
        assert_eq!(
            compose_current_revision(stored, 0),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn canonical_digest_is_independent_of_json_object_key_order() {
        let left = serde_json::json!({"mode": {"known": {"value": "production", "provenance": "source"}}, "envelope": {"scale": "one"}});
        let right = serde_json::from_str::<serde_json::Value>(
            r#"{"envelope":{"scale":"one"},"mode":{"known":{"provenance":"source","value":"production"}}}"#,
        )
        .unwrap();
        assert_eq!(
            canonical_matrix_input_digest(&left).unwrap(),
            canonical_matrix_input_digest(&right).unwrap()
        );
    }

    #[test]
    fn choice_set_must_bind_to_exact_revision_and_input() {
        let stored = stored_revision();
        let mut request = RecordMatrixTask {
            task_id: stored.task_id,
            revision: 2,
            expected_current_revision: 1,
            request_id: stored.request_id,
            input: stored.input,
            choice_set: None,
        };
        assert_eq!(validate_request(&request), Ok(()));
        let choice = EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "choice-1".into(),
            version: 1,
            task_id: request.task_id.to_string(),
            task_revision: "2".into(),
            decision_question: "Which approach?".into(),
            candidates: vec![EngineeringCandidate {
                candidate_id: "a".into(),
                title: "A".into(),
                approach: "Use A".into(),
                assumption_fact_ids: vec!["criticality".into()],
            }],
        };
        for count in 0..=1 {
            let mut choice = choice.clone();
            choice.candidates.truncate(count);
            request.choice_set = Some(choice);
            assert_eq!(validate_request(&request), Ok(()));
        }
        let mut choice = choice.clone();
        choice.task_revision = "1".into();
        request.choice_set = Some(choice.clone());
        assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
        let mut choice = choice.clone();
        choice.task_revision = "2".into();
        choice.task_id = Uuid::new_v4().to_string();
        request.choice_set = Some(choice.clone());
        assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
        let mut choice = choice;
        choice.task_id = request.task_id.to_string();
        choice.candidates[0].assumption_fact_ids = vec!["unknown.fact".into()];
        request.choice_set = Some(choice);
        assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
    }
}
