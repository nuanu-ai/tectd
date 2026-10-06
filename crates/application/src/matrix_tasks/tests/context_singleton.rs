use super::*;
use sha2::Digest;
use tect_domain::*;

fn fixture(
    count: usize,
) -> (
    MatrixTaskRevision,
    EffectiveMatrixRequirements,
    ContextMatrixVerificationRecord,
    Uuid,
) {
    let mut revision = stored_revision();
    let anchor = RequirementsAnchor::Program {
        program_id: Uuid::new_v4(),
    };
    let recorder = DeclarationRecorder {
        principal: "agent".into(),
        session: "session".into(),
    };
    let proposal = MatrixRequirementsProposal::new(
        anchor.clone(),
        1,
        vec![
            DeclaredRequirementValue::Mode(EngineeringMode::Mvp),
            DeclaredRequirementValue::Intent(EngineeringIntent::Other("build".into())),
            DeclaredRequirementValue::Urgency("normal".into()),
            DeclaredRequirementValue::PromisedBehavior("bounded behavior".into()),
            DeclaredRequirementValue::PromisedProof("regression".into()),
            DeclaredRequirementValue::NoDemandCommitment,
            DeclaredRequirementValue::NoLatencyCommitment,
        ]
        .into_iter()
        .map(|value| RequirementDeclarationPatch::Set { value })
        .collect(),
        recorder.clone(),
    )
    .unwrap();
    let confirmation = MatrixRequirementsConfirmation::new(
        &proposal,
        1,
        proposal.digest().into(),
        "owner".into(),
        "human-response".into(),
        recorder,
    )
    .unwrap();
    let context = resolve_matrix_requirements(
        &[anchor],
        &[MatrixRequirementsRevision {
            proposal,
            confirmation: Some(confirmation),
        }],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap();
    revision.input.envelope = OperatingEnvelope {
        scale: known("limited".into()),
        operational_facts: OperationalFacts::KnownEmpty {
            provenance: FactProvenance("observed".into()),
        },
    };
    revision.input.criticality = known("limited".into());
    revision.input.affected_guarantees = MatrixFact::KnownEmpty {
        provenance: FactProvenance("observed".into()),
    };
    revision.input.actual_exposure = known(false);
    revision.input.urgent_repair = known(false);
    revision.input = bind_matrix_requirements_input(&context, &revision.input).unwrap();
    revision.input_digest = matrix_input_digest(&revision.input).unwrap();
    let choices = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "owner-choices".into(),
        version: 1,
        task_id: revision.task_id.to_string(),
        task_revision: revision.revision.to_string(),
        decision_question: "Which approach?".into(),
        candidates: (0..count)
            .map(|i| EngineeringCandidate {
                candidate_id: format!("choice-{i}"),
                title: "Owner choice".into(),
                approach: format!("bounded approach {i}"),
                assumption_fact_ids: vec![],
            })
            .collect(),
    };
    revision.choice_set_digest = Some(choices.canonical_digest(&revision.input).unwrap());
    revision.choice_set = Some(choices);
    let snapshot = Uuid::new_v4();
    let mut record = ContextMatrixVerificationRecord {
        schema: CONTEXT_MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: revision.task_id.to_string(),
        task_revision: revision.revision.to_string(),
        frozen_snapshot_id: snapshot.to_string(),
        authority_schema: context.schema().into(),
        input_digest: revision.input_digest.clone(),
        requirements_semantic_digest: context.semantic_digest().into(),
        owner_principal: revision.recorded_by_principal_id.to_string(),
        verifier_principal: Uuid::new_v4().to_string(),
        policy_version: "test-policy/1".into(),
        bindings: required_matrix_operating_facts(&context, &revision.input)
            .unwrap()
            .into_iter()
            .map(|f| MatrixEvidenceBinding {
                fact_path: f.path,
                value_digest: f.value_digest,
                evidence_ref: "immutable-test-ref".into(),
                content_digest: "a".repeat(64),
                source: "test-source".into(),
                subject: "test-task".into(),
                observed_at: 10,
                expires_at: 100,
                validation_outcome: EvidenceValidationOutcome::Accepted,
            })
            .collect(),
        digest: String::new(),
    };
    record.digest = record.canonical_digest().unwrap();
    (revision, context, record, snapshot)
}

struct Store(Option<ContextMatrixVerificationRecord>);
#[async_trait::async_trait]
impl crate::ContextMatrixVerificationStore for Store {
    async fn context_matrix_verification_for_revision(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: i64,
        _: &str,
        _: Uuid,
    ) -> Result<Option<ContextMatrixVerificationRecord>> {
        Ok(self.0.clone())
    }
    async fn append_context_matrix_verification(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: Uuid,
        _: i64,
        _: &str,
        _: &ContextMatrixVerificationRecord,
    ) -> Result<()> {
        panic!("read-only test")
    }
}
struct Validator(bool);
#[async_trait::async_trait]
impl crate::MatrixEvidenceValidator for Validator {
    fn policy_version(&self) -> &str {
        "test-policy/1"
    }
    async fn validate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        _: &RequiredMatrixFact,
        _: &str,
        _: i64,
    ) -> Result<MatrixEvidenceBinding> {
        panic!("revalidation only")
    }
    async fn revalidate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> Result<()> {
        if self.0
            && fact.path == binding.fact_path
            && fact.value_digest == binding.value_digest
            && now < binding.expires_at
        {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}
struct NoProvider;
#[async_trait::async_trait]
impl MatrixAdviceProvider for NoProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        panic!("singleton must not inspect provider")
    }
    fn prepare(&self, _: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        panic!("singleton must not prepare")
    }
    async fn attempt_prepared(
        &self,
        _: PreparedMatrixAdviceAttempt,
        _: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        panic!("singleton must not send")
    }
}
struct NoBudget;
#[async_trait::async_trait]
impl MatrixBudgetPolicy for NoBudget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
        _: &AdvisoryBudgetPolicy,
    ) -> Result<Option<MatrixBudgetAuthorization>> {
        panic!("singleton must not reserve budget")
    }
}

async fn composed(
    revision: &MatrixTaskRevision,
    context: &EffectiveMatrixRequirements,
    record: Option<ContextMatrixVerificationRecord>,
    snapshot: Uuid,
    trusted: bool,
    now: i64,
) -> Option<(
    ContextEngineeringMatrixComposition,
    ContextMatrixVerificationRecord,
)> {
    binding::compose_bound_revision_with_verification(
        Some(&mut Store(record)),
        &Validator(trusted),
        Uuid::new_v4(),
        revision,
        snapshot,
        context,
        now,
    )
    .await
    .unwrap()
}

#[tokio::test]
async fn context_singleton_captures_selected_material_without_provider_or_budget() {
    let (revision, context, record, snapshot) = fixture(1);
    let config = advisory_config(WorkspaceAdvisoryMode::Optional);
    let mut opportunity = matrix_advisory_opportunity_input(
        &revision,
        &advisory_request(revision.task_id),
        &config,
        Uuid::new_v4(),
        Uuid::new_v4(),
    )
    .unwrap();
    assert!(advisory::singleton_snapshot_no_call(
        &opportunity,
        &revision
    ));
    let (composition, verified) = composed(
        &revision,
        &context,
        Some(record.clone()),
        snapshot,
        true,
        20,
    )
    .await
    .unwrap();
    let verification = crate::MatrixDispositionVerification::ContextV2 {
        binding: crate::MatrixTaskRequirementsBinding {
            locator: crate::MatrixRequirementsLocator::Program {
                program_id: Uuid::new_v4(),
            },
            snapshot_id: snapshot,
            authority_schema: context.schema().into(),
            semantic_digest: context.semantic_digest().into(),
        },
        composition: Box::new(composition),
        record: Box::new(verified),
    };
    crate::matrix_advisory_capture::bind_verified_matrix_snapshot(
        &mut opportunity,
        &revision,
        &verification,
    )
    .unwrap();
    assert_eq!(
        opportunity.matrix_verification_digest.as_deref(),
        Some(record.digest.as_str())
    );
    assert_eq!(
        opportunity.material_digest,
        verification
            .disposition_digest(&revision.input, revision.choice_set.as_ref().unwrap())
            .unwrap()
    );
    assert_eq!(
        opportunity.primary_reason,
        AdvisoryReason::ChoiceSetNotApplicable
    );
    let actor = opportunity.authorized_actor_id;
    assert!(matches!(
        crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut opportunity,
            None,
            config.workspace_id,
            actor,
            &NoProvider,
            &NoBudget,
            None
        )
        .await
        .unwrap(),
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
    ));
    assert_eq!(opportunity.state, AdvisoryOpportunityState::NoCall);
}

#[tokio::test]
async fn context_singleton_revalidation_denies_missing_expired_changed_and_untrusted_material() {
    let (revision, context, record, snapshot) = fixture(1);
    assert!(
        composed(&revision, &context, None, snapshot, true, 20)
            .await
            .is_none()
    );
    assert!(
        composed(
            &revision,
            &context,
            Some(record.clone()),
            snapshot,
            true,
            100
        )
        .await
        .is_none()
    );
    assert!(
        composed(
            &revision,
            &context,
            Some(record.clone()),
            snapshot,
            false,
            20
        )
        .await
        .is_none()
    );
    let mut changed = revision.clone();
    changed.input.actual_exposure = known(true);
    assert!(
        composed(&changed, &context, Some(record.clone()), snapshot, true, 20)
            .await
            .is_none()
    );
    assert!(
        composed(
            &revision,
            &context,
            Some(record.clone()),
            Uuid::new_v4(),
            true,
            20
        )
        .await
        .is_none()
    );
    let mut drift = record.clone();
    drift.bindings[0].content_digest = "b".repeat(64);
    assert!(
        composed(&revision, &context, Some(drift), snapshot, true, 20)
            .await
            .is_none()
    );
}

#[tokio::test]
async fn context_disposition_keeps_eligible_preimage_and_denies_zero_invalid_or_drift() {
    let (revision, context, record, snapshot) = fixture(2);
    let (composition, _) = composed(
        &revision,
        &context,
        Some(record.clone()),
        snapshot,
        true,
        20,
    )
    .await
    .unwrap();
    let set = revision.choice_set.as_ref().unwrap();
    let material = matrix_evaluation_digest(&revision.input, composition.composition(), set)
        .unwrap()
        .unwrap();
    let old_bytes = serde_json::to_vec(&(
        "tect.context-matrix-verified-evaluation/1",
        material,
        &record.digest,
        &record.frozen_snapshot_id,
        &record.authority_schema,
        &record.requirements_semantic_digest,
    ))
    .unwrap();
    let old_digest = format!("{:x}", sha2::Sha256::digest(old_bytes));
    assert_eq!(
        crate::context_matrix_verified_disposition_digest(
            &revision.input,
            &composition,
            set,
            &record
        )
        .unwrap(),
        old_digest
    );
    assert_eq!(
        crate::context_matrix_verified_disposition_digest(
            &revision.input,
            &composition,
            set,
            &record
        ),
        crate::context_matrix_verified_evaluation_digest(
            &revision.input,
            &composition,
            set,
            &record
        )
    );
    let mut zero = set.clone();
    zero.candidates.clear();
    assert_eq!(
        crate::context_matrix_verified_disposition_digest(
            &revision.input,
            &composition,
            &zero,
            &record
        ),
        Err(Error::InvalidArguments)
    );
    let mut invalid = set.clone();
    invalid.candidates.truncate(1);
    invalid.candidates[0].candidate_id.clear();
    assert_eq!(
        crate::context_matrix_verified_disposition_digest(
            &revision.input,
            &composition,
            &invalid,
            &record
        ),
        Err(Error::InvalidArguments)
    );
    let mut singleton = set.clone();
    singleton.candidates.truncate(1);
    assert!(
        crate::context_matrix_verified_evaluation_digest(
            &revision.input,
            &composition,
            &singleton,
            &record
        )
        .is_err()
    );
    let digest = crate::context_matrix_verified_disposition_digest(
        &revision.input,
        &composition,
        &singleton,
        &record,
    )
    .unwrap();
    singleton.candidates[0].approach.push_str(" changed");
    assert_ne!(
        crate::context_matrix_verified_disposition_digest(
            &revision.input,
            &composition,
            &singleton,
            &record
        )
        .unwrap(),
        digest
    );
    let mut foreign = record.clone();
    foreign.task_revision = "3".into();
    assert!(
        crate::context_matrix_verified_disposition_digest(
            &revision.input,
            &composition,
            &singleton,
            &foreign
        )
        .is_err()
    );
}

#[path = "context_singleton/current_policy_revalidation.rs"]
mod current_policy_revalidation;
