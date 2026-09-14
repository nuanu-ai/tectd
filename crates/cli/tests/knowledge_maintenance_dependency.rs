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
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn document(title: &str, dependencies: &[String]) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    value["document"]["title"] = json!(title);
    value["document"]["canonical_text"] = json!(format!("Canonical body for {title}."));
    value["document"]["sources"][0]["snapshot"]["uri"] =
        json!(format!("urn:dk4-maintenance-dependency:{title}"));
    value["document"]["sources"][0]["snapshot"]["text"] = json!(title);
    value["document"]["sections"]["runbook"]["dependency_iris"] = json!(dependencies);
    value["document"].clone()
}

fn mutation(
    unit: Uuid,
    revision: i64,
    operation: &'static str,
    value: Option<Value>,
) -> SingleOperation {
    let sources = value
        .as_ref()
        .map(|document| document["sources"].clone())
        .unwrap_or_else(|| json!([]));
    SingleOperation {
        operation,
        unit_id: Some(json!(unit)),
        expected_revision: Some(revision),
        expected_lifecycle: Some("active"),
        document: value,
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        sources,
        knowledge_kind: json!("procedure"),
        profiles: json!(["general", "runbook"]),
        erasure: "not_required",
        authored_followup: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn dependency_advance_and_withdrawal_preserve_each_unresolved_review_basis() -> Result<()> {
    if std::env::var("TECT_TEST_DK4_MAINTENANCE").as_deref() != Ok("1") {
        return Ok(());
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
    support::repository(&root.join("source"));
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let workspace_key = format!("dk4-maintenance-dependency-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let request_context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native.clone(),
        workspace_key: workspace_key.clone(),
    };
    service.open_workspace(&request_context).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let socket = root.join("dk4-maintenance-dependency.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk4-maintenance-dependency-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;

    let dependency = commit_create(&mut client, document("dependency-v1", &[])).await;
    let dependency_id = Uuid::parse_str(
        dependency.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let dependency_iri: String = sqlx::query_scalar(
        "SELECT unit_iri FROM knowledge_revisions WHERE unit_id=$1 AND revision=1",
    )
    .bind(dependency_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let dependent = commit_create(
        &mut client,
        document("dependent", std::slice::from_ref(&dependency_iri)),
    )
    .await;
    let dependent_id = Uuid::parse_str(
        dependent.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();

    commit_single(
        &mut client,
        mutation(
            dependency_id,
            1,
            "revise",
            Some(document("dependency-v2", &[])),
        ),
    )
    .await;
    commit_single(
        &mut client,
        mutation(
            dependency_id,
            2,
            "revise",
            Some(document("dependency-v3", &[])),
        ),
    )
    .await;
    commit_single(&mut client, mutation(dependency_id, 3, "retract", None)).await;

    let processed = service
        .process_knowledge_maintenance_tasks(&request_context, 3)
        .await
        .unwrap();
    assert_eq!((processed.claimed, processed.needs_review), (3, 3));
    let bases: Vec<(String, Uuid)> = sqlx::query_as(
        "SELECT t.state,(s.basis->>'observed_event_id')::uuid \
         FROM knowledge_maintenance_tasks t JOIN knowledge_maintenance_signals s ON s.id=t.signal_id \
         WHERE s.unit_id=$1 AND s.reason='dependency_changed' ORDER BY s.observed_at,t.id",
    )
    .bind(dependent_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(bases.len(), 3);
    assert!(bases.iter().all(|(state, _)| state == "needs_review"));
    assert_eq!(
        bases
            .iter()
            .map(|(_, event)| event)
            .collect::<std::collections::BTreeSet<_>>()
            .len(),
        3
    );

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    pool.close().await;
    Ok(())
}
