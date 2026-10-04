use super::*;
use knowledge_lifecycle_support::{
    advance_create_to_review, begin_create_request, commit_create, complete_review,
};

fn document(scope: &Value, run: &Value, title: &str) -> Value {
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let mut document = fixture["document"].clone();
    document["title"] = json!(title);
    document["bindings"][0]["target"] = json!({"kind":"slice_phase",
        "scope_id":scope,"slice_id":run["slice_id"],"phase_id":run["current_phase_id"]});
    document
}

async fn publication_action(client: &mut Mcp, document: &Value) -> Value {
    let begun = route(
        client,
        "command",
        "knowledge.change_begin",
        begin_create_request(document, json!({"kind":"workspace"}), Uuid::new_v4()),
    )
    .await;
    let reviewed = advance_create_to_review(client, document, begun).await;
    let reviewed = complete_review(client, &reviewed, "ready").await;
    let publication = route(
        client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&reviewed["actions"][0]).clone(),
    )
    .await;
    action_params(&publication["actions"][0]).clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn migrated_slice_binding_uses_current_head_and_rejects_stale_preparation() {
    let pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("current-binding.sock");
    let _daemon = Daemon::start(&tagged_url(&runtime_url, "current-binding"), socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("current-binding-{}", Uuid::new_v4()),
    )
    .await;
    let (context, scope) = run_fixture(&mut client, &repo).await;
    let predecessor = context["run"].clone();
    let committed = commit_create(
        &mut client,
        document(&scope, &predecessor, "Historical predecessor binding"),
    )
    .await;
    let historical_unit = committed.receipt["applied_operations"][0]["unit_id"].clone();
    // Publication advances knowledge generation; refresh the predecessor's
    // manifest through its actual action before testing retained consumption.
    let current = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":predecessor["id"]}),
    )
    .await;
    let refresh = current["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action_name(action) == Some("pipeline.knowledge_refresh"))
        .unwrap();
    route(
        &mut client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    let refreshed = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":predecessor["id"]}),
    )
    .await;
    let predecessor = refreshed["run"].clone();
    assert!(
        refreshed["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit["unit_id"] == historical_unit)
    );

    let historical_unit_id = Uuid::parse_str(historical_unit.as_str().unwrap()).unwrap();
    let bindings_before: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(b) FROM knowledge_bindings b WHERE unit_id=$1 ORDER BY id",
    )
    .bind(historical_unit_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(bindings_before.len(), 1);
    let stale = publication_action(
        &mut client,
        &document(&scope, &predecessor, "Prepared predecessor binding"),
    )
    .await;
    let migrated = client
        .call(
            "pipeline_run_migrate",
            json!({"request_id":Uuid::new_v4(),
        "predecessor_run_id":predecessor["id"],"expected_revision":predecessor["revision"],
        "idempotency_key":format!("binding-migration-{}",Uuid::new_v4()),
        "successor_definition_version":"0.7.0-native.k1k5","mappings":mapping()}),
        )
        .await;
    let refused = route_error(&mut client, "command", "knowledge.change_commit", stale).await;
    assert_eq!(refused["error"]["code"], "invalid_arguments");
    let successor = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":migrated["successor_run_id"]}),
    )
    .await;
    let fresh = commit_create(
        &mut client,
        document(&scope, &successor["run"], "Fresh successor binding"),
    )
    .await;
    let fresh_unit = Uuid::parse_str(
        fresh.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let pin: (String,String,String) = sqlx::query_as(
        "SELECT definition_kind,definition_version,definition_digest FROM knowledge_bindings WHERE unit_id=$1")
        .bind(fresh_unit).fetch_one(&pool).await.unwrap();
    assert_eq!(pin.0, successor["run"]["definition_kind"].as_str().unwrap());
    assert_eq!(
        pin.1,
        successor["run"]["definition_version"].as_str().unwrap()
    );
    assert_eq!(
        pin.2,
        successor["run"]["definition_digest"].as_str().unwrap()
    );
    let bindings_after: Vec<Value> = sqlx::query_scalar(
        "SELECT to_jsonb(b) FROM knowledge_bindings b WHERE unit_id=$1 ORDER BY id",
    )
    .bind(historical_unit_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(
        bindings_before, bindings_after,
        "committed historical pin is immutable"
    );
    let old = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":predecessor["id"]}),
    )
    .await;
    assert_eq!(old["run"]["status"], "superseded");
    assert_eq!(
        old["run"]["definition_digest"],
        predecessor["definition_digest"]
    );
    assert!(
        old["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|unit| unit["unit_id"] == historical_unit),
        "exact historical run consumes its committed binding"
    );
}
