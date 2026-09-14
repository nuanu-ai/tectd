#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[path = "pipeline_execution/full_support.rs"]
#[allow(dead_code)]
mod pipeline_support;
#[path = "pipeline_execution/producer_research.rs"]
#[allow(dead_code)]
mod producer_research;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use knowledge_operation_support::{SingleOperation, commit_single};
use pipeline_support::{completion, successful_route};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn draft() -> Value {
    json!({"coverage_summary":"Exact producer receipt fixture","nodes":[{
    "kind":"work","identity":{"local":"producer"},"title":"Capture producer output",
    "outcome":"An exact producer output is published by a separate Knowledge Change",
    "includes":["source output","publisher receipt"],"excludes":["automatic publication"],
    "dependencies":[],"proof":["Exact publisher receipt"],"pipeline":"slice.custom-procedure-capture",
    "pipeline_reason":"Exercise exact producer publication binding","source_result_ids":[]}],"supersessions":[]})
}

async fn begin(client: &mut Mcp, repo: &std::path::Path) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope=route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
    let saved = save(client, &scope["created"]["planning"], draft()).await;
    let reviewed = review(client, &saved).await;
    let opened = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
        "scope_id":reviewed["scope"]["id"],"slice_id":opened["created"]["id"],
        "slice_revision":opened["created"]["revision"],"delivery_mode":"phasewise",
        "qualification_reason":"Exact producer publication integration fixture."}),
    )
    .await["created"]
        .clone()
}

async fn advance(client: &mut Mcp, context: Value) -> Value {
    let (verdict, outcome, transition) = successful_route(&context);
    let mut request = completion(&context, verdict, outcome, transition, None, None);
    acknowledge_knowledge(&mut request, &context);
    route(client, "command", "slice.pipeline.phase.complete", request).await["context"].clone()
}

async fn promotion_context(client: &mut Mcp, repo: &std::path::Path) -> Value {
    let mut current = begin(client, repo).await;
    while current["run"]["current_phase_ordinal"].as_u64().unwrap() < 15 {
        current = advance(client, current).await;
    }
    advance(client, current).await
}

async fn refresh(client: &mut Mcp, context: &Value) -> Value {
    let stale = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    let action = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(action).clone(),
    )
    .await;
    route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await
}

async fn rejects(client: &mut Mcp, request: Value) {
    assert_eq!(
        route_error(client, "command", "slice.pipeline.phase.complete", request).await["error"]["code"],
        "invalid_source"
    );
}

fn terminal() -> Value {
    json!({"summary":"Exact publisher receipt accepted for the producer output.",
    "evidence":[{"kind":"integration_test","reference":"pipeline_execution_knowledge_publication.rs",
    "observation":"The backend validated receipt identity, operation membership and exact output lineage."}],
    "scope_impact":"The producer reports the separate canonical publication.","remaining_work":"None."})
}

fn acknowledge_knowledge(request: &mut Value, context: &Value) {
    let manifest = if context["knowledge_resources"].is_object() {
        &context["knowledge_resources"]
    } else {
        &context["knowledge"]
    };
    let has_selected = manifest["selected"]
        .as_array()
        .is_some_and(|values| !values.is_empty());
    if manifest.is_object() && has_selected {
        request["consumed_knowledge"] =
            json!({"manifest_id":manifest["id"],"digest":manifest["digest"]});
    }
}

fn promoted(context: &Value, receipt: &Value) -> Value {
    let mut request = completion(
        context,
        "procedure_promoted_after_approval",
        "completed",
        "continue",
        None,
        None,
    );
    request["output"]["fields"]["external_promotion_owner"] = json!("knowledge.change");
    request["output"]["fields"]["external_promotion_reference"] = receipt["id"].clone();
    request["output"]["fields"]["external_promotion_digest"] = receipt["digest"].clone();
    request["output"]["fields"]["external_promotion_authority_evidence"] = json!(format!(
        "sealed-command:{}",
        receipt["sealed_command_digest"].as_str().unwrap()
    ));
    request["output"]["knowledge_publication"] = json!({"change_id":receipt["change_id"],
        "publisher_receipt_id":receipt["id"],"publisher_receipt_digest":receipt["digest"],
        "operation_ids":[receipt["applied_operations"][0]["operation_id"]]});
    acknowledge_knowledge(&mut request, context);
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn producer_accepts_only_exact_publisher_receipt_and_lineage() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
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
    repository(&repo);
    let socket = root.join("producer.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk2-producer-{}", Uuid::new_v4()));
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("producer-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let mut current = promotion_context(&mut client, &repo).await;
    let source = current["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|v| v["phase_ordinal"] == 13)
        .unwrap();
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let unrelated_document = fixture["document"].clone();
    fixture["document"]["sources"] = json!([{"kind":"pipeline_output","output":{"run_id":current["run"]["id"],
        "output_id":source["id"],"digest":source["digest"],"evidence_kind":"runtime_verification",
        "evidence_scope":"Exact procedure proposal output."}}]);
    let committed = commit_create(&mut client, fixture["document"].clone()).await;
    current = refresh(&mut client, &current).await;
    let mut wrong = promoted(&current, &committed.receipt);
    wrong["request_id"] = json!(Uuid::new_v4());
    wrong["output"]["knowledge_publication"]["operation_ids"] = json!([]);
    rejects(&mut client, wrong).await;
    let mut wrong = promoted(&current, &committed.receipt);
    wrong["request_id"] = json!(Uuid::new_v4());
    wrong["output"]["fields"]["external_promotion_digest"] = json!("0".repeat(64));
    rejects(&mut client, wrong).await;
    let unrelated = commit_create(&mut client, unrelated_document.clone()).await;
    current = refresh(&mut client, &current).await;
    rejects(&mut client, promoted(&current, &unrelated.receipt)).await;
    let foreign = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let foreign_config = root.join("foreign.json");
    host_file(&foreign_config, &foreign.auth);
    let mut foreign_client = Mcp::start(
        &socket,
        &foreign_config,
        &Uuid::new_v4().to_string(),
        &format!("foreign-producer-{}", Uuid::new_v4()),
    )
    .await;
    foreign_client.call("open_workspace", json!({})).await;
    let foreign_committed = commit_create(&mut foreign_client, unrelated_document).await;
    rejects(&mut client, promoted(&current, &foreign_committed.receipt)).await;
    foreign_client.finish().await;
    let accepted_request = promoted(&current, &committed.receipt);
    let accepted = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        accepted_request.clone(),
    )
    .await;
    assert_eq!(accepted["context"]["run"]["current_phase_ordinal"], 17);
    let mut final_request = completion(
        &accepted["context"],
        "handoff_not_required",
        "completed",
        "complete",
        None,
        Some(terminal()),
    );
    acknowledge_knowledge(&mut final_request, &accepted["context"]);
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        final_request,
    )
    .await["context"]
        .clone();
    assert_eq!(completed["run"]["status"], "completed");
    assert_eq!(
        completed["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .find(|value| value["phase_ordinal"] == 16)
            .unwrap()["knowledge_publication"]["publisher_receipt_id"],
        committed.receipt["id"]
    );
    producer_research::prove(&mut client, &repo).await;
    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(committed.receipt["applied_operations"][0]["unit_id"].clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "owned_live_copies",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        erased["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            accepted_request
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn generic_workspace_runbook_requires_exact_shared_manifest_acknowledgement() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
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
    repository(&repo);
    let socket = root.join("generic-consumer.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk2-consumer-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("consumer-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    fixture["document"]["bindings"][0]["purpose"] = json!("procedure");
    let committed = commit_create(&mut client, fixture["document"].clone()).await;
    assert_eq!(committed.exact["document"]["document"], fixture["document"]);

    let current = begin(&mut client, &repo).await;
    assert!(
        current["knowledge"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let resources = &current["knowledge_resources"];
    assert_eq!(resources["id"], current["knowledge"]["id"]);
    assert_eq!(resources["digest"], current["knowledge"]["digest"]);
    assert_eq!(resources["selected"].as_array().unwrap().len(), 1);
    assert_eq!(resources["selected"][0]["knowledge_kind"], "procedure");
    assert_eq!(
        resources["selected"][0]["sections"]["runbook"],
        fixture["document"]["sections"]["runbook"]
    );

    let (verdict, outcome, transition) = successful_route(&current);
    let without_ack = completion(&current, verdict, outcome, transition, None, None);
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            without_ack.clone()
        )
        .await["error"]["code"],
        "needs_context"
    );
    let mut revised = fixture["document"].clone();
    revised["title"] = json!("Pinned TectD package source contract revision two");
    revised["canonical_text"] = json!(
        "Package the verified selected binary into a new owned output and preserve exact review evidence."
    );
    let revision_sources = revised["sources"].clone();
    let revision = commit_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(committed.receipt["applied_operations"][0]["unit_id"].clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(revised),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: revision_sources,
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: true,
        },
    )
    .await;
    assert_eq!(revision["applied"]["applied_operations"][0]["revision"], 2);
    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":current["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    let mut stale_ack = without_ack;
    stale_ack["request_id"] = json!(Uuid::new_v4());
    stale_ack["consumed_knowledge"] = json!({
        "manifest_id":resources["id"], "digest":resources["digest"]
    });
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            stale_ack,
        )
        .await["error"]["code"],
        "context_changed"
    );
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
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
        json!({"run_id":current["run"]["id"]}),
    )
    .await;
    assert_eq!(refreshed["knowledge_resource_status"]["state"], "current");
    assert_eq!(
        refreshed["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        refreshed["knowledge_resources"]["selected"][0]["revision"],
        2
    );
    let mut with_ack = completion(&refreshed, verdict, outcome, transition, None, None);
    with_ack["request_id"] = json!(Uuid::new_v4());
    with_ack["consumed_knowledge"] = json!({
        "manifest_id":refreshed["knowledge_resources"]["id"],
        "digest":refreshed["knowledge_resources"]["digest"]
    });
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        with_ack,
    )
    .await;
    assert_eq!(completed["context"]["run"]["current_phase_ordinal"], 2);
    client.finish().await;
}
