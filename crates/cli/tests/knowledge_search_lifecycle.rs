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

use knowledge_lifecycle_support::{
    begin_create_request, commit_create, commit_create_from_current, settle_and_finish,
};
use knowledge_operation_support::{SingleOperation, commit_single};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{future::Future, pin::Pin, sync::Arc};
use tect_application::{KnowledgeEmbeddingProvider, WorkspaceService};
use tect_domain::{
    KnowledgeEmbeddingModelIdentity, KnowledgeEmbeddingRequest, RequestContext, Result,
};
use tect_postgres::{PgStore, admin};
use tokio::sync::{Mutex, Notify};
use uuid::Uuid;

#[derive(Default)]
struct ControlledProvider {
    state: Mutex<ProviderState>,
    entered: Notify,
    release: Notify,
}

#[derive(Default)]
struct ProviderState {
    block_next: bool,
    calls: u32,
}

impl ControlledProvider {
    async fn arm(&self) {
        self.state.lock().await.block_next = true;
    }

    async fn wait_for_call(&self) {
        self.entered.notified().await;
    }

    fn release(&self) {
        self.release.notify_one();
    }
}

impl KnowledgeEmbeddingProvider for ControlledProvider {
    fn model(&self) -> Option<KnowledgeEmbeddingModelIdentity> {
        Some(KnowledgeEmbeddingModelIdentity::pinned())
    }

    fn embed<'life0, 'life1, 'async_trait>(
        &'life0 self,
        _: &'life1 KnowledgeEmbeddingRequest,
    ) -> Pin<Box<dyn Future<Output = Result<Vec<f32>>> + Send + 'async_trait>>
    where
        'life0: 'async_trait,
        'life1: 'async_trait,
        Self: 'async_trait,
    {
        Box::pin(async move {
            let block = {
                let mut state = self.state.lock().await;
                state.calls += 1;
                std::mem::take(&mut state.block_next)
            };
            if block {
                self.entered.notify_one();
                self.release.notified().await;
            }
            let mut values = vec![0.0; 384];
            values[0] = 1.0;
            Ok(values)
        })
    }
}

fn document(title: &str) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    value["document"]["title"] = json!(title);
    value["document"]["canonical_text"] = json!(format!("Canonical body for {title}."));
    value["document"]["sources"][0]["snapshot"]["uri"] =
        json!(format!("urn:dk3-lifecycle:{title}"));
    value["document"]["sources"][0]["snapshot"]["text"] = json!(title);
    value["document"].clone()
}

fn revise(unit: Value, revision: i64, document: Value) -> SingleOperation {
    let sources = document["sources"].clone();
    SingleOperation {
        operation: "revise",
        unit_id: Some(unit),
        expected_revision: Some(revision),
        expected_lifecycle: Some("active"),
        document: Some(document),
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

fn remove(unit: Value, revision: i64, operation: &'static str) -> SingleOperation {
    SingleOperation {
        operation,
        unit_id: Some(unit),
        expected_revision: Some(revision),
        expected_lifecycle: Some("active"),
        document: None,
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        sources: json!([]),
        knowledge_kind: json!("procedure"),
        profiles: json!(["general", "runbook"]),
        erasure: if operation == "erase" {
            "owned_live_copies"
        } else {
            "not_required"
        },
        authored_followup: false,
    }
}

async fn required_create(
    client: &mut Mcp,
    document: Value,
) -> knowledge_lifecycle_support::CommittedKnowledge {
    let mut request = begin_create_request(&document, json!({"kind":"workspace"}), Uuid::new_v4());
    request["completion"]["search"] = json!("required");
    let begun = support::route(client, "command", "knowledge.change_begin", request).await;
    commit_create_from_current(client, document, begun).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn search_jobs_follow_current_title_access_and_required_completion() {
    if std::env::var("TECT_TEST_DK3_LIFECYCLE").as_deref() != Ok("1") {
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
    let provider = Arc::new(ControlledProvider::default());
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_knowledge_embedding_provider(provider.clone()),
    );
    let workspace_key = format!("dk3-lifecycle-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let request_context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native.clone(),
        workspace_key: workspace_key.clone(),
    };
    service.open_workspace(&request_context).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let socket = root.join("dk3-lifecycle.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk3-lifecycle-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;

    let disabled_required = required_create(&mut client, document("disabled-required")).await;
    assert_eq!(
        disabled_required.receipt["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["kind"] == "search")
            .unwrap()["status"],
        "ready"
    );
    settle_and_finish(&mut client, &disabled_required).await;

    tect_postgres::enable_knowledge_vector_search(&pool, &role)
        .await
        .unwrap();
    let initial = service
        .process_knowledge_search_jobs(&request_context, 8)
        .await
        .unwrap();
    assert!(initial.published >= 1);

    let vector_required = required_create(&mut client, document("vector-required")).await;
    assert_eq!(
        vector_required.receipt["effects"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["kind"] == "search")
            .unwrap()["status"],
        "pending"
    );
    let required_drain = service
        .process_knowledge_search_jobs(&request_context, 1)
        .await
        .unwrap();
    assert_eq!(required_drain.published, 1);
    settle_and_finish(&mut client, &vector_required).await;

    let body = commit_create(&mut client, document("body-stable")).await;
    let body_unit = body.receipt["applied_operations"][0]["unit_id"].clone();
    provider.arm().await;
    let service_task = service.clone();
    let context_task = request_context.clone();
    let processing = tokio::spawn(async move {
        service_task
            .process_knowledge_search_jobs(&context_task, 1)
            .await
            .unwrap()
    });
    provider.wait_for_call().await;
    let mut body_revision = document("body-stable");
    body_revision["canonical_text"] = json!("Revised body with unchanged title and access.");
    let revised = commit_single(&mut client, revise(body_unit.clone(), 1, body_revision)).await;
    assert_eq!(revised["applied"]["applied_operations"][0]["revision"], 2);
    provider.release();
    let reused = processing.await.unwrap();
    assert_eq!((reused.published, reused.obsolete), (1, 0));
    let vector_revision: i64 =
        sqlx::query_scalar("SELECT revision FROM knowledge_search_vectors WHERE unit_id=$1")
            .bind(Uuid::parse_str(body_unit.as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(vector_revision, 2);

    let changed = commit_create(&mut client, document("title-before")).await;
    let changed_unit = changed.receipt["applied_operations"][0]["unit_id"].clone();
    provider.arm().await;
    let service_task = service.clone();
    let context_task = request_context.clone();
    let processing = tokio::spawn(async move {
        service_task
            .process_knowledge_search_jobs(&context_task, 1)
            .await
            .unwrap()
    });
    provider.wait_for_call().await;
    let changed_revision = document("title-after");
    let changed_commit = commit_single(
        &mut client,
        revise(changed_unit.clone(), 1, changed_revision),
    )
    .await;
    assert_eq!(
        changed_commit["applied"]["applied_operations"][0]["revision"],
        2
    );
    provider.release();
    let stale = processing.await.unwrap();
    assert_eq!((stale.published, stale.obsolete), (0, 1));
    let changed_uuid = Uuid::parse_str(changed_unit.as_str().unwrap()).unwrap();
    let counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_search_vectors WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_embedding_jobs WHERE unit_id=$1)",
    )
    .bind(changed_uuid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (0, 1));
    let refreshed = service
        .process_knowledge_search_jobs(&request_context, 1)
        .await
        .unwrap();
    assert_eq!((refreshed.published, refreshed.pending), (1, 0));

    let access = commit_create(&mut client, document("access-current")).await;
    let access_unit = access.receipt["applied_operations"][0]["unit_id"].clone();
    assert_eq!(
        service
            .process_knowledge_search_jobs(&request_context, 1)
            .await
            .unwrap()
            .published,
        1
    );
    let mut access_revision = document("access-current");
    access_revision["access_scope"] = json!("owners_only");
    let access_commit =
        commit_single(&mut client, revise(access_unit.clone(), 1, access_revision)).await;
    assert_eq!(
        access_commit["applied"]["applied_operations"][0]["revision"],
        2
    );
    let access_uuid = Uuid::parse_str(access_unit.as_str().unwrap()).unwrap();
    let access_counts: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_search_vectors WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_embedding_jobs WHERE unit_id=$1)",
    )
    .bind(access_uuid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(access_counts, (0, 1));
    assert_eq!(
        service
            .process_knowledge_search_jobs(&request_context, 1)
            .await
            .unwrap()
            .published,
        1
    );
    let retracted = commit_single(&mut client, remove(access_unit, 2, "retract")).await;
    assert_eq!(
        retracted["applied"]["applied_operations"][0]["operation"],
        "retract"
    );
    let retracted_counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_search_resources WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_embedding_jobs WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_vectors WHERE unit_id=$1)",
    )
    .bind(access_uuid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(retracted_counts, (0, 0, 0));

    let erased = commit_create(&mut client, document("erase-current")).await;
    let erased_unit = erased.receipt["applied_operations"][0]["unit_id"].clone();
    provider.arm().await;
    let service_task = service.clone();
    let context_task = request_context.clone();
    let processing = tokio::spawn(async move {
        service_task
            .process_knowledge_search_jobs(&context_task, 1)
            .await
            .unwrap()
    });
    provider.wait_for_call().await;
    let erased_uuid = Uuid::parse_str(erased_unit.as_str().unwrap()).unwrap();
    let erased_commit = commit_single(&mut client, remove(erased_unit, 1, "erase")).await;
    assert_eq!(
        erased_commit["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    provider.release();
    let stale = processing.await.unwrap();
    assert_eq!((stale.published, stale.obsolete), (0, 1));
    let erased_counts: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_search_resources WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_embedding_jobs WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_search_vectors WHERE unit_id=$1),\
         (SELECT count(*) FROM knowledge_owned_copies WHERE unit_id=$1 AND relation_name LIKE 'knowledge_search_%' AND NOT redacted)",
    )
    .bind(erased_uuid)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(erased_counts, (0, 0, 0, 0));

    let mut due_document = document("freshness-current");
    due_document["review_due_at"] = json!("2021-01-01T00:00:00Z");
    let due = commit_create(&mut client, due_document).await;
    let due_unit = Uuid::parse_str(
        due.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    sqlx::query(
        "UPDATE knowledge_search_resources SET freshness_warnings='[]'::jsonb WHERE unit_id=$1",
    )
    .bind(due_unit)
    .execute(&pool)
    .await
    .unwrap();
    let search = support::route(
        &mut client,
        "query",
        "knowledge.search",
        json!({"mode":"lexical","query":"freshness current","limit":5,
            "corpus_limit":64,"purpose":"Recompute a time-derived freshness warning."}),
    )
    .await;
    let due_result = search["results"]
        .as_array()
        .unwrap()
        .iter()
        .find(|result| result["unit_id"] == due_unit.to_string())
        .unwrap();
    assert_eq!(due_result["freshness_warnings"], json!(["review_due"]));

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    pool.close().await;
}
