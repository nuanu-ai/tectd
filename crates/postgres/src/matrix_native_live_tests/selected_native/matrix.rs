use super::*;
use fixtures::{EvidenceCase, SyntheticProvider, SyntheticValidator};

pub(super) fn configured(
    store: Arc<PgStore>,
    workspace: Uuid,
    source: &MatrixTaskSource,
    effective: &EffectiveMatrixRequirements,
) -> (
    WorkspaceService,
    Vec<MatrixEvidenceReference>,
    AdvisoryProviderProfileRef,
    AdvisoryModelConfiguration,
    Arc<AtomicUsize>,
) {
    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let task = source.revision.task_id;
    let required = required_matrix_operating_facts(effective, &source.revision.input).unwrap();
    assert!(!required.is_empty());
    let mut cases = BTreeMap::new();
    let evidence = required
        .iter()
        .map(|fact| {
            let reference = format!("synthetic-selected:{workspace}:{task}:{}", fact.path);
            cases.insert(
                reference.clone(),
                EvidenceCase {
                    workspace,
                    task,
                    revision: 1,
                    binding: MatrixEvidenceBinding {
                        fact_path: fact.path.clone(),
                        value_digest: fact.value_digest.clone(),
                        evidence_ref: reference.clone(),
                        content_digest: format!("{:x}", Sha256::digest(reference.as_bytes())),
                        source: "synthetic-fixture-registry".into(),
                        subject: format!("{workspace}/{task}/1"),
                        observed_at: now,
                        expires_at: now + 3600,
                        validation_outcome: EvidenceValidationOutcome::Accepted,
                    },
                },
            );
            MatrixEvidenceReference {
                fact_path: fact.path.clone(),
                evidence_ref: reference,
            }
        })
        .collect();
    let profile = AdvisoryProviderProfileRef {
        id: "synthetic-selected".into(),
    };
    let model = AdvisoryModelConfiguration {
        model: "synthetic-no-call".into(),
    };
    let sends = Arc::new(AtomicUsize::new(0));
    let service = service(store)
        .with_matrix_evidence_validator(Arc::new(SyntheticValidator { cases }))
        .with_matrix_advice_provider(Arc::new(SyntheticProvider {
            identity: MatrixProviderIdentity {
                provider_profile_ref: profile.clone(),
                model_configuration: model.clone(),
                destination: "synthetic:no-transport".into(),
                wire_version: "tect.matrix-typesafe-native/1".into(),
                ranking_policy: MatrixRankingPolicy::StrictV1,
            },
            prepares: Arc::new(AtomicUsize::new(0)),
            sends: Arc::clone(&sends),
        }));
    (service, evidence, profile, model, sends)
}

pub(super) async fn selected(
    store: Arc<PgStore>,
    pool: &PgPool,
    owner: &admin::Enrollment,
    context: &RequestContext,
    workspace: Uuid,
    source: &MatrixTaskSource,
    effective: &EffectiveMatrixRequirements,
) -> (WorkspaceService, MatrixPlanningSelection, Arc<AtomicUsize>) {
    let task = source.revision.task_id;
    let (service, evidence, profile, model, sends) =
        configured(store, workspace, source, effective);
    let verifier = admin::prepare_verifier_enrollment(pool, owner.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_context = fixtures::context(&verifier.auth, &context.workspace_key);
    service.open_workspace(&verifier_context).await.unwrap();
    let VerifiedMatrixTask::Context(record) = service
        .verify_matrix_task(
            &verifier_context,
            &VerifyMatrixTask {
                task_id: task,
                expected_revision: 1,
                input_digest: source.revision.input_digest.clone(),
                evidence,
            },
        )
        .await
        .unwrap()
    else {
        panic!("requires ContextV2");
    };
    let binding = source.requirements_binding.as_ref().unwrap();
    assert_eq!(record.frozen_snapshot_id, binding.snapshot_id.to_string());
    service
        .configure_advisory(
            context,
            &ConfigureWorkspaceAdvisory {
                expected_revision: 0,
                mode: WorkspaceAdvisoryMode::Optional,
                provider_profile_ref: Some(profile),
                model_configuration: Some(model),
            },
        )
        .await
        .unwrap();
    let opportunity = service
        .request_engineering_advisory(
            context,
            &RequestEngineeringAdvisory {
                task_id: task,
                expected_task_revision: 1,
                request_key: format!("selected-skip-{}", Uuid::new_v4()),
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: AdvisoryRequestPreference::Skip,
            },
        )
        .await
        .unwrap();
    assert_eq!(opportunity.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(opportunity.primary_reason, AdvisoryReason::RequestSkip);
    assert_eq!(
        opportunity.matrix_verification_digest.as_deref(),
        Some(record.digest.as_str())
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    let disposition = service
        .record_matrix_disposition(
            context,
            &RecordMatrixDisposition {
                request_id: Uuid::new_v4(),
                task_id: task,
                expected_task_revision: 1,
                expected_input_digest: source.revision.input_digest.clone(),
                expected_choice_set_digest: source.revision.choice_set_digest.clone(),
                opportunity_id: opportunity.id,
                basis: MatrixDispositionBasis::NoCall,
                advice_id: None,
                advice_digest: None,
                decision: MatrixDispositionDecision::Selected {
                    selected_choice_id: "a".into(),
                },
            },
        )
        .await
        .unwrap();
    let selection = MatrixPlanningSelection {
        task_id: task,
        task_revision: 1,
        disposition_id: disposition.disposition_id,
        selected_choice_id: "a".into(),
        expected_input_digest: source.revision.input_digest.clone(),
        expected_choice_set_digest: source.revision.choice_set_digest.clone().unwrap(),
        expected_verification_digest: record.digest,
        mapped_draft_node_indices: vec![0],
    };
    (service, selection, sends)
}

pub(super) async fn change_semantics(
    service: &WorkspaceService,
    context: &RequestContext,
    program: Uuid,
) {
    let locator = MatrixRequirementsLocator::Program {
        program_id: program,
    };
    let proposal = service
        .propose_matrix_requirements_context(
            context,
            &ProposeMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                expected_context_revision: 1,
                patches: vec![RequirementDeclarationPatch::Set {
                    value: DeclaredRequirementValue::Mode(EngineeringMode::Production),
                }],
            },
        )
        .await
        .unwrap();
    service
        .confirm_matrix_requirements_context(
            context,
            &ConfirmMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator,
                proposal_revision: proposal.proposal.revision(),
                proposal_digest: proposal.proposal.digest().into(),
                owner_response_ref: format!("semantic-change-{program}"),
            },
        )
        .await
        .unwrap();
}
