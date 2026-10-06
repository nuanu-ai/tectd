//! Synthetic App/PgStore vertical only: no provider transport or external trust proof.
use crate::{PgStore, admin};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    collections::BTreeMap,
    sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    },
    time::{SystemTime, UNIX_EPOCH},
};
use tect_application::{
    ConfirmMatrixRequirementsContext, MatrixAdviceProvider, MatrixEvidenceReference,
    MatrixEvidenceValidator, MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixRankingPolicy, MatrixRequirementsLocator, MatrixStartedDispatchPermit, MatrixTaskSource,
    PreparedMatrixAdviceAttempt, ProposeMatrixRequirementsContext, RecordMatrixTask,
    RequestEngineeringAdvisory, VerifiedMatrixTask, VerifyMatrixTask, WorkspaceService,
};
use tect_domain::{
    AdvisoryAuditQuery, AdvisoryCapability, AdvisoryModelConfiguration, AdvisoryOpportunityState,
    AdvisoryProviderProfileRef, AdvisoryReason, AdvisoryRequestPreference,
    CONTEXT_MATRIX_VERIFICATION_SCHEMA, ConfigureWorkspaceAdvisory, DeclaredRequirementValue,
    EffectiveMatrixRequirements, EngineeringCandidate, EngineeringChoiceSet, EngineeringIntent,
    EngineeringMatrixInput, EngineeringMode, Error, EvidenceValidationOutcome, FactProvenance,
    HostAuth, MATRIX_CHOICE_SET_SCHEMA, MATRIX_REQUIREMENTS_SCHEMA, MatrixEvidenceBinding,
    MatrixFact, OperatingEnvelope, OperationalFacts, RequestContext, RequiredMatrixFact,
    RequirementDeclarationPatch, Result, WorkspaceAdvisoryMode, bind_matrix_requirements_input,
    required_matrix_operating_facts,
};
use uuid::Uuid;

#[path = "matrix_native_live_tests/fixtures.rs"]
mod fixtures;
#[path = "matrix_native_live_tests/selected_native.rs"]
mod selected_native;
use fixtures::{
    EvidenceCase, SyntheticProvider, SyntheticValidator, bound_source, context, counts, service,
};

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 18.6 and explicit TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn matrix_context_verified_budget_no_call_is_persisted_replay_safe_and_tenant_isolated() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("explicit admin URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("explicit runtime URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("explicit runtime role required");
    assert!(
        admin_url != runtime_url,
        "separate admin/runtime identities required"
    );
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, "180006");
    let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
    let initial = service(Arc::clone(&store));
    let owner = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let owner_context = context(&owner.auth, &format!("matrix-live-{}", Uuid::new_v4()));
    let opened = initial.open_workspace(&owner_context).await.unwrap();
    let workspace = opened.workspace.as_ref().unwrap().id;
    let owner_session = opened.session.as_ref().unwrap().id;
    let (program, source, effective) =
        bound_source(&initial, &pool, &owner, &owner_context, workspace).await;
    let task = source.revision.task_id;
    let other = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    assert_ne!(other.tenant_id, owner.tenant_id);
    let other_context = context(&other.auth, &format!("matrix-other-{}", Uuid::new_v4()));
    let other_workspace = initial
        .open_workspace(&other_context)
        .await
        .unwrap()
        .workspace
        .unwrap()
        .id;
    let (_, other_source, _) =
        bound_source(&initial, &pool, &other, &other_context, other_workspace).await;
    let other_before = counts(
        &pool,
        other.tenant_id,
        other_workspace,
        other_source.revision.task_id,
    )
    .await;
    assert!(matches!(
        initial.get_matrix_task_source(&other_context, task).await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert!(matches!(
        initial
            .get_matrix_task_source(&owner_context, other_source.revision.task_id)
            .await,
        Err(Error::NotFound | Error::Forbidden)
    ));

    let now = i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let required = required_matrix_operating_facts(&effective, &source.revision.input).unwrap();
    assert!(!required.is_empty());
    let mut cases = BTreeMap::new();
    let evidence = required
        .iter()
        .map(|fact| {
            let reference = format!("synthetic-matrix:{workspace}:{task}:1:{}", fact.path);
            let binding = MatrixEvidenceBinding {
                fact_path: fact.path.clone(),
                value_digest: fact.value_digest.clone(),
                evidence_ref: reference.clone(),
                content_digest: format!("{:x}", Sha256::digest(reference.as_bytes())),
                source: "synthetic-fixture-registry".into(),
                subject: format!("{workspace}/{task}/1"),
                observed_at: now,
                expires_at: now + 60,
                validation_outcome: EvidenceValidationOutcome::Accepted,
            };
            cases.insert(
                reference.clone(),
                EvidenceCase {
                    workspace,
                    task,
                    revision: 1,
                    binding,
                },
            );
            MatrixEvidenceReference {
                fact_path: fact.path.clone(),
                evidence_ref: reference,
            }
        })
        .collect();
    let prepares = Arc::new(AtomicUsize::new(0));
    let sends = Arc::new(AtomicUsize::new(0));
    let profile = AdvisoryProviderProfileRef {
        id: "synthetic-matrix-profile".into(),
    };
    let model = AdvisoryModelConfiguration {
        model: "synthetic-no-call".into(),
    };
    let service = service(Arc::clone(&store))
        .with_matrix_evidence_validator(Arc::new(SyntheticValidator { cases }))
        .with_matrix_advice_provider(Arc::new(SyntheticProvider {
            identity: MatrixProviderIdentity {
                provider_profile_ref: profile.clone(),
                model_configuration: model.clone(),
                destination: "synthetic:no-transport".into(),
                wire_version: "tect.matrix-typesafe-native/1".into(),
                ranking_policy: MatrixRankingPolicy::StrictV1,
            },
            prepares: Arc::clone(&prepares),
            sends: Arc::clone(&sends),
        }));
    let verification = VerifyMatrixTask {
        task_id: task,
        expected_revision: 1,
        input_digest: source.revision.input_digest.clone(),
        evidence,
    };
    assert_eq!(
        counts(&pool, owner.tenant_id, workspace, task).await,
        (1, 0, 0, 0)
    );
    assert!(matches!(
        service
            .verify_matrix_task(&owner_context, &verification)
            .await,
        Err(Error::Forbidden)
    ));
    assert_eq!(
        counts(&pool, owner.tenant_id, workspace, task).await,
        (1, 0, 0, 0)
    );
    let verifier = admin::prepare_verifier_enrollment(&pool, owner.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_ne!(verifier.principal_id, owner.principal_id);
    assert_ne!(verifier.auth.host_id, owner.auth.host_id);
    let verifier_context = context(&verifier.auth, &owner_context.workspace_key);
    let verifier_open = service.open_workspace(&verifier_context).await.unwrap();
    assert_eq!(verifier_open.workspace.as_ref().unwrap().id, workspace);
    let verifier_session = verifier_open.session.as_ref().unwrap().id;
    let VerifiedMatrixTask::Context(record) = service
        .verify_matrix_task(&verifier_context, &verification)
        .await
        .unwrap()
    else {
        panic!("bound task requires ContextV2 verification")
    };
    assert_eq!(record.schema, CONTEXT_MATRIX_VERIFICATION_SCHEMA);
    assert_eq!(record.task_id, task.to_string());
    assert_eq!(record.task_revision, "1");
    assert_eq!(record.owner_principal, owner.principal_id.to_string());
    assert_eq!(record.verifier_principal, verifier.principal_id.to_string());
    assert_eq!(record.input_digest, source.revision.input_digest);
    let binding = source.requirements_binding.as_ref().unwrap();
    assert_eq!(record.frozen_snapshot_id, binding.snapshot_id.to_string());
    assert_eq!(record.authority_schema, binding.authority_schema);
    assert_eq!(record.requirements_semantic_digest, binding.semantic_digest);
    assert_eq!(record.bindings.len(), required.len());
    assert_eq!(record.digest, record.canonical_digest().unwrap());
    let persisted: (String, Uuid, Uuid, Uuid, String) = sqlx::query_as("SELECT record_digest,owner_principal_id,verifier_principal_id,verifier_session_id,requirements_semantic_digest FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND task_revision=1")
        .bind(owner.tenant_id).bind(workspace).bind(task).fetch_one(&pool).await.unwrap();
    assert_eq!(
        persisted,
        (
            record.digest.clone(),
            owner.principal_id,
            verifier.principal_id,
            verifier_session,
            binding.semantic_digest.clone()
        )
    );
    assert!(matches!(
        service
            .get_matrix_task_source(&verifier_context, task)
            .await,
        Err(Error::Forbidden)
    ));
    assert_eq!(
        service
            .get_matrix_task_source(&owner_context, task)
            .await
            .unwrap(),
        source
    );
    let config = service
        .configure_advisory(
            &owner_context,
            &ConfigureWorkspaceAdvisory {
                expected_revision: 0,
                mode: WorkspaceAdvisoryMode::Optional,
                provider_profile_ref: Some(profile),
                model_configuration: Some(model),
            },
        )
        .await
        .unwrap();
    let policy_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_budget_policies WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(owner.tenant_id)
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(policy_count, 0);
    let request = RequestEngineeringAdvisory {
        task_id: task,
        expected_task_revision: 1,
        request_key: format!("matrix-budget-no-call-{}", Uuid::new_v4()),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let opportunity = service
        .request_engineering_advisory(&owner_context, &request)
        .await
        .unwrap();
    assert_eq!(opportunity.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        opportunity.primary_reason,
        AdvisoryReason::BudgetPolicyInvalid
    );
    assert_eq!(opportunity.target_id, Some(task));
    assert_eq!(opportunity.matrix_task_revision, Some(1));
    assert_eq!(
        opportunity.matrix_choice_set_digest,
        source.revision.choice_set_digest
    );
    assert_eq!(
        opportunity.matrix_verification_digest.as_deref(),
        Some(record.digest.as_str())
    );
    assert_eq!(opportunity.config_revision, config.revision);
    assert_eq!(opportunity.session_id, owner_session);
    assert_eq!(opportunity.authorized_actor_id, owner.principal_id);
    let prepared_once = prepares.load(Ordering::SeqCst);
    assert!(prepared_once >= 1);
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    let read = service
        .get_engineering_advisory(&owner_context, task, &request.request_key)
        .await
        .unwrap();
    assert_eq!(read.opportunity, opportunity);
    assert!(read.current_advice.is_none());
    assert!(matches!(
        service
            .get_engineering_advisory(&other_context, task, &request.request_key)
            .await,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert_eq!(
        service
            .request_engineering_advisory(&owner_context, &request)
            .await
            .unwrap(),
        opportunity
    );
    assert_eq!(prepares.load(Ordering::SeqCst), prepared_once);
    assert_eq!(sends.load(Ordering::SeqCst), 0);
    let audit = service
        .advisory_audit(
            &owner_context,
            &AdvisoryAuditQuery {
                limit: 10,
                scope_id: None,
                after: None,
                capability: Some(AdvisoryCapability::EngineeringProfile),
                decision_point: None,
                reason: Some(AdvisoryReason::BudgetPolicyInvalid),
                state: Some(AdvisoryOpportunityState::NoCall),
            },
        )
        .await
        .unwrap();
    assert_eq!(audit.opportunities.len(), 1);
    assert_eq!(audit.opportunities[0].id, opportunity.id);
    assert!(audit.dispatches.is_empty());
    assert_eq!(audit.aggregate.no_call_opportunities, 1);
    assert_eq!(audit.aggregate.authorized_attempts, 0);
    let final_counts = counts(&pool, owner.tenant_id, workspace, task).await;
    assert_eq!(final_counts, (1, 1, 1, 0));
    assert_eq!(
        counts(
            &pool,
            other.tenant_id,
            other_workspace,
            other_source.revision.task_id
        )
        .await,
        other_before
    );
    println!(
        "MATRIX_NATIVE_NO_CALL tenant={} workspace={} program={} task={} revision=1 snapshot={} owner={} owner_session={} verifier={} verifier_session={} verification_digest={} choice_digest={} input_digest={} opportunity={} config_revision={} facts={} revisions={} verifications={} opportunities={} dispatches={} authorized_attempts=0 prepares={} sends=0 other_tenant={} other_workspace={} other_task={}",
        owner.tenant_id,
        workspace,
        program,
        task,
        binding.snapshot_id,
        owner.principal_id,
        owner_session,
        verifier.principal_id,
        verifier_session,
        record.digest,
        source.revision.choice_set_digest.as_ref().unwrap(),
        source.revision.input_digest,
        opportunity.id,
        config.revision,
        required.len(),
        final_counts.0,
        final_counts.1,
        final_counts.2,
        final_counts.3,
        prepared_once,
        other.tenant_id,
        other_workspace,
        other_source.revision.task_id
    );
}
