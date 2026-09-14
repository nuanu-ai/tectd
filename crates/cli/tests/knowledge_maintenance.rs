#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use knowledge_operation_support::{SingleOperation, commit_single};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tect_application::{
    KnowledgeLifecycleDefinitionProvider, KnowledgeMaintenanceOutputGuard, Store, TransactionMode,
    WorkspaceService,
};
use tect_domain::*;
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

struct AcceptOutput;

impl KnowledgeMaintenanceOutputGuard for AcceptOutput {
    fn observe(&self, _: &ObserveKnowledgeMaintenanceOutcome) -> Result<()> {
        Ok(())
    }

    fn begin(&self, _: &BeginKnowledgeMaintenanceChangeOutcome) -> Result<()> {
        Ok(())
    }
}

struct RejectOutput;

impl KnowledgeMaintenanceOutputGuard for RejectOutput {
    fn observe(&self, _: &ObserveKnowledgeMaintenanceOutcome) -> Result<()> {
        Err(Error::CapacityExceeded)
    }

    fn begin(&self, _: &BeginKnowledgeMaintenanceChangeOutcome) -> Result<()> {
        Err(Error::CapacityExceeded)
    }
}

struct MaintenanceMethod;

impl KnowledgeLifecycleDefinitionProvider for MaintenanceMethod {
    fn definition(&self) -> Result<KnowledgeChangeDefinition> {
        Err(Error::InvalidConfiguration)
    }

    fn registry(&self) -> Result<KnowledgeProfileRegistry> {
        Err(Error::InvalidConfiguration)
    }

    fn maintenance_method(&self) -> Result<PipelineInstructionSnapshot> {
        Ok(PipelineInstructionSnapshot {
            id: KNOWLEDGE_MAINTENANCE_METHOD_ID.into(),
            version: "0.4.0-dk4.1".into(),
            digest: "test-maintenance-method-digest".into(),
            body: "Read the exact maintenance task and resolve its basis through Knowledge Change."
                .into(),
            origin_refs: Vec::new(),
        })
    }
}

fn document(title: &str, review_due_at: Option<&str>) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    value["document"]["title"] = json!(title);
    value["document"]["canonical_text"] = json!(format!("Canonical body for {title}."));
    value["document"]["sources"][0]["snapshot"]["uri"] =
        json!(format!("urn:dk4-maintenance:{title}"));
    value["document"]["sources"][0]["snapshot"]["text"] = json!(title);
    if let Some(due_at) = review_due_at {
        value["document"]["review_due_at"] = json!(due_at);
    }
    value["document"].clone()
}

fn erase(unit: Uuid, revision: i64) -> SingleOperation {
    SingleOperation {
        operation: "erase",
        unit_id: Some(json!(unit)),
        expected_revision: Some(revision),
        expected_lifecycle: Some("active"),
        document: None,
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        sources: json!([]),
        knowledge_kind: json!("procedure"),
        profiles: json!(["general", "runbook"]),
        erasure: "owned_live_copies",
        authored_followup: false,
    }
}

fn operator_request(
    unit: Uuid,
    revision: i64,
    request: Uuid,
    subject_ref: &str,
    marker: &str,
) -> ObserveKnowledgeMaintenanceSignal {
    ObserveKnowledgeMaintenanceSignal {
        request_id: request,
        unit_id: unit,
        unit_revision: revision,
        basis: KnowledgeMaintenanceBasis::OperatorRequested {
            subject_ref: subject_ref.into(),
            observation_digest: marker.into(),
        },
    }
}

fn task(value: &ObserveKnowledgeMaintenanceOutcome) -> &KnowledgeMaintenanceTask {
    match value {
        ObserveKnowledgeMaintenanceOutcome::Created(value)
        | ObserveKnowledgeMaintenanceOutcome::Existing(value)
        | ObserveKnowledgeMaintenanceOutcome::Replay(value) => value,
    }
}

async fn transaction(
    store: &PgStore,
    auth: &HostAuth,
) -> (Box<dyn tect_application::UnitOfWork>, HostIdentity) {
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = tx.authenticate(auth).await.unwrap();
    tx.set_tenant(identity.tenant_id).await.unwrap();
    (tx, identity)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn maintenance_replay_leases_due_sweep_and_erasure_are_bounded() {
    if std::env::var("TECT_TEST_DK4_MAINTENANCE").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let store = Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap());
    let service = Arc::new(WorkspaceService::new(
        store.clone(),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let workspace_key = format!("dk4-maintenance-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let request_context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native.clone(),
        workspace_key: workspace_key.clone(),
    };
    service.open_workspace(&request_context).await.unwrap();
    let workspace: Uuid = sqlx::query_scalar("SELECT id FROM workspaces WHERE key=$1")
        .bind(&workspace_key)
        .fetch_one(&pool)
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let socket = root.join("dk4-maintenance.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk4-maintenance-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;

    let committed = commit_create(&mut client, document("operator", None)).await;
    let unit = Uuid::parse_str(
        committed.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let unit_iri: String = sqlx::query_scalar(
        "SELECT unit_iri FROM knowledge_revisions WHERE unit_id=$1 AND revision=1",
    )
    .bind(unit)
    .fetch_one(&pool)
    .await
    .unwrap();
    let request_id = Uuid::new_v4();
    let request = operator_request(unit, 1, request_id, &unit_iri, "operator-basis-1");
    let created = service
        .observe_knowledge_maintenance(&request_context, &request, &AcceptOutput)
        .await
        .unwrap();
    assert!(matches!(
        created,
        ObserveKnowledgeMaintenanceOutcome::Created(_)
    ));
    let created_task = task(&created).id;
    let replay = service
        .observe_knowledge_maintenance(&request_context, &request, &AcceptOutput)
        .await
        .unwrap();
    assert!(matches!(
        replay,
        ObserveKnowledgeMaintenanceOutcome::Replay(_)
    ));
    assert_eq!(task(&replay).id, created_task);
    let mut conflicting = request.clone();
    conflicting.basis = KnowledgeMaintenanceBasis::OperatorRequested {
        subject_ref: unit_iri.clone(),
        observation_digest: "operator-basis-conflict".into(),
    };
    assert_eq!(
        service
            .observe_knowledge_maintenance(&request_context, &conflicting, &AcceptOutput)
            .await,
        Err(Error::InputConflict)
    );

    let rejected = operator_request(unit, 1, Uuid::new_v4(), &unit_iri, "guarded-output");
    assert_eq!(
        service
            .observe_knowledge_maintenance(&request_context, &rejected, &RejectOutput)
            .await,
        Err(Error::CapacityExceeded)
    );
    let rejected_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_maintenance_signals WHERE request_id=$1",
    )
    .bind(rejected.request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(rejected_count, 0);

    let processed = service
        .process_knowledge_maintenance_tasks(&request_context, 1)
        .await
        .unwrap();
    assert_eq!((processed.claimed, processed.needs_review), (1, 1));
    let context = service
        .knowledge_maintenance(
            &request_context,
            &KnowledgeMaintenanceQuery {
                unit_id: Some(unit),
                states: vec![KnowledgeMaintenanceTaskState::NeedsReview],
                after: None,
                limit: 10,
                fragment: Some(KnowledgeLifecycleFragmentQuery {
                    snapshot_digest: None,
                    offset: 0,
                    limit: 10,
                }),
            },
            &MaintenanceMethod,
        )
        .await
        .unwrap();
    assert_eq!(context.tasks.len(), 1);
    assert!(
        context.tasks[0]
            .current_review
            .as_ref()
            .unwrap()
            .needs_review
    );

    let retry_request = operator_request(unit, 1, Uuid::new_v4(), &unit_iri, "retry-basis");
    service
        .observe_knowledge_maintenance(&request_context, &retry_request, &AcceptOutput)
        .await
        .unwrap();
    for attempt in 1..=5 {
        if attempt > 1 {
            sqlx::query("UPDATE knowledge_maintenance_tasks SET next_retry_at='-infinity' WHERE signal_id=(SELECT id FROM knowledge_maintenance_signals WHERE request_id=$1)")
                .bind(retry_request.request_id)
                .execute(&pool)
                .await
                .unwrap();
        }
        let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
        let claim = tx
            .claim_knowledge_maintenance_task(workspace, identity.principal_id)
            .await
            .unwrap()
            .claim
            .unwrap();
        tx.commit().await.unwrap();
        let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
        let outcome = tx
            .fail_knowledge_maintenance_task(
                workspace,
                identity.principal_id,
                &claim,
                KnowledgeMaintenanceFailureCode::StorageUnavailable,
            )
            .await
            .unwrap();
        assert_eq!(
            outcome,
            if attempt == 5 {
                KnowledgeMaintenanceFailureOutcome::Exhausted
            } else {
                KnowledgeMaintenanceFailureOutcome::RetryScheduled
            }
        );
        tx.commit().await.unwrap();
    }
    let exhausted: (String, i32, Option<String>) = sqlx::query_as(
        "SELECT state,attempts,failure_code FROM knowledge_maintenance_tasks WHERE signal_id=(SELECT id FROM knowledge_maintenance_signals WHERE request_id=$1)",
    )
    .bind(retry_request.request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        exhausted,
        ("exhausted".into(), 5, Some("storage_unavailable".into()))
    );

    let expired_request = operator_request(unit, 1, Uuid::new_v4(), &unit_iri, "expired-lease");
    service
        .observe_knowledge_maintenance(&request_context, &expired_request, &AcceptOutput)
        .await
        .unwrap();
    let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
    let stale = tx
        .claim_knowledge_maintenance_task(workspace, identity.principal_id)
        .await
        .unwrap()
        .claim
        .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("UPDATE knowledge_maintenance_tasks SET lease_expires_at='-infinity' WHERE id=$1")
        .bind(stale.task_id)
        .execute(&pool)
        .await
        .unwrap();
    let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
    assert_eq!(
        tx.prepare_knowledge_maintenance_task(workspace, identity.principal_id, &stale)
            .await,
        Err(Error::ContextChanged)
    );
    drop(tx);
    let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
    let recovery = tx
        .claim_knowledge_maintenance_task(workspace, identity.principal_id)
        .await
        .unwrap();
    assert!(recovery.claim.is_none());
    tx.commit().await.unwrap();
    let recovered: (String, Option<Uuid>, Option<String>) = sqlx::query_as(
        "SELECT state,lease_token,failure_code FROM knowledge_maintenance_tasks WHERE id=$1",
    )
    .bind(stale.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        recovered,
        ("pending".into(), None, Some("lease_expired".into()))
    );

    for index in 0..3 {
        let due = commit_create(
            &mut client,
            document(&format!("due-{index}"), Some("2021-01-01T00:00:00Z")),
        )
        .await;
        assert_eq!(due.receipt["applied_operations"][0]["revision"], 1);
    }
    for _ in 0..3 {
        let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
        assert_eq!(
            tx.sweep_due_knowledge_maintenance(workspace, identity.principal_id, 1)
                .await
                .unwrap(),
            1
        );
        tx.commit().await.unwrap();
    }
    let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
    assert_eq!(
        tx.sweep_due_knowledge_maintenance(workspace, identity.principal_id, 1)
            .await
            .unwrap(),
        0
    );
    tx.commit().await.unwrap();

    let erased = commit_create(&mut client, document("erased-maintenance", None)).await;
    let erased_unit = Uuid::parse_str(
        erased.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let erased_unit_iri: String = sqlx::query_scalar(
        "SELECT unit_iri FROM knowledge_revisions WHERE unit_id=$1 AND revision=1",
    )
    .bind(erased_unit)
    .fetch_one(&pool)
    .await
    .unwrap();
    let erased_request = operator_request(
        erased_unit,
        1,
        Uuid::new_v4(),
        &erased_unit_iri,
        "erase-inflight",
    );
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state='needs_review',next_retry_at=NULL \
         WHERE workspace_id=$1 AND state='pending'",
    )
    .bind(workspace)
    .execute(&pool)
    .await
    .unwrap();
    let erased_observation = service
        .observe_knowledge_maintenance(&request_context, &erased_request, &AcceptOutput)
        .await
        .unwrap();
    let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
    let erased_claim = tx
        .claim_knowledge_maintenance_task(workspace, identity.principal_id)
        .await
        .unwrap()
        .claim
        .unwrap();
    assert_eq!(erased_claim.task_id, task(&erased_observation).id);
    tx.commit().await.unwrap();
    let result = commit_single(&mut client, erase(erased_unit, 1)).await;
    assert_eq!(
        result["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    let (mut tx, identity) = transaction(&store, &enrollment.auth).await;
    assert_eq!(
        tx.prepare_knowledge_maintenance_task(workspace, identity.principal_id, &erased_claim)
            .await,
        Err(Error::ContextChanged)
    );
    drop(tx);
    let erased_rows: (bool, bool, String, Option<Uuid>) = sqlx::query_as(
        "SELECT s.payload_erased,t.payload_erased,t.state,t.lease_token FROM knowledge_maintenance_tasks t JOIN knowledge_maintenance_signals s ON s.id=t.signal_id WHERE t.id=$1",
    )
    .bind(erased_claim.task_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(erased_rows, (true, true, "obsolete".into(), None));

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    pool.close().await;
}
