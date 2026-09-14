#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    advance_create_to_review, advance_create_to_review_with_identity, begin_create_request,
    commit_create_from_current, complete_review, context, method_reads, omit_nulls, query_current,
};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, save};
use tect_postgres::admin;
use uuid::Uuid;

fn promotion_draft(label: &str) -> Value {
    json!({"coverage_summary":"One bounded Knowledge Change outcome is managed by Promotion.",
        "nodes":[{"kind":"work","identity":{"local":label},"title":"Resolve the exact fixture declaration",
            "outcome":"The reviewed Knowledge Change reaches a typed terminal outcome",
            "includes":["qualified review","managed result"],"excludes":["new research"],
            "dependencies":[],"proof":["Knowledge Change terminal result"],
            "pipeline":"slice.promote-to-durable-knowledge",
            "pipeline_reason":"The bounded outcome requires the dedicated Knowledge Change owner.",
            "source_result_ids":[]}],"supersessions":[]})
}

async fn open_promotion(client: &mut Mcp, repo: &std::path::Path, label: &str) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope = route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(
        client,
        &scope["created"]["planning"],
        promotion_draft(label),
    )
    .await;
    let promotion = saved["draft"]["nodes"][0].clone();
    let reviewed = review(client, &saved).await;
    route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &promotion, Uuid::new_v4()),
    )
    .await["created"]
        .clone()
}

async fn finish(
    client: &mut Mcp,
    current: &Value,
    canonical: &str,
    user_outcome: &str,
    remaining_work: Value,
    publisher: Option<Value>,
    effects: Value,
) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    params["output"]["method_reads"] = method_reads(action);
    params["output"]["body"] = json!("Backend-validated terminal Promotion outcome.");
    params["output"]["data"] = json!({"phase":"kc-result-handoff","data":{
        "canonical":canonical,"user_outcome":user_outcome,
        "summary":"The exact reviewed Knowledge Change reached its terminal outcome.",
        "remaining_work":remaining_work,"publisher_receipt_id":publisher,"effects":effects}});
    params["output"]["verdict"] = json!("complete");
    params["output"]["outcome"] = json!("completed");
    params["output"]["transition"] = json!("complete");
    params["output"]["findings"] = json!([]);
    params["output"]["dispositions"] = json!([]);
    omit_nulls(&mut params);
    route(client, "command", "knowledge.change_phase_complete", params).await
}

async fn assert_result(
    pool: &PgPool,
    slice: &Value,
    change: &Value,
    expected_outcome: &str,
    expected_origin: &str,
    has_publisher: bool,
) {
    let row: (String, String, Option<Uuid>, i64, i64, i64) = sqlx::query_as(
        "SELECT r.outcome,r.knowledge_result_origin,r.knowledge_publisher_receipt_id, \
         s.revision,(SELECT count(*) FROM slice_planning_inputs i WHERE i.source_result_id=r.id), \
         (SELECT count(*) FROM slice_pipeline_runs p WHERE p.slice_id=r.slice_id) \
         FROM slice_results r JOIN native_slices s ON s.tenant_id=r.tenant_id \
         AND s.workspace_id=r.workspace_id AND s.id=r.slice_id WHERE r.knowledge_change_id=$1",
    )
    .bind(Uuid::parse_str(change.as_str().unwrap()).unwrap())
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(row.0, expected_outcome);
    assert_eq!(row.1, expected_origin);
    assert_eq!(row.2.is_some(), has_publisher);
    assert_eq!(row.3, slice["revision"].as_i64().unwrap() + 1);
    assert_eq!((row.4, row.5), (1, 0));
}

#[tokio::test]
async fn promotion_terminal_outcomes_resolve_to_one_managed_result() {
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
    let socket = root.join("dk2-promotion-outcomes.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk2-promotion-outcomes-{}", Uuid::new_v4()),
    );
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
        &format!("dk2-promotion-outcomes-{}", Uuid::new_v4()),
    )
    .await;

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let original = fixture["document"].clone();

    let rejected_slice = open_promotion(&mut client, &repo, "rejected").await;
    let mut rejected_document = original.clone();
    rejected_document["title"] = json!("Rejected Promotion fixture proposal");
    let begun=route(&mut client,"command","knowledge.change_begin",begin_create_request(&rejected_document,
        json!({"kind":"promotion_slice","scope_id":rejected_slice["scope_id"],"slice_id":rejected_slice["id"],
            "slice_revision":rejected_slice["revision"]}),Uuid::new_v4())).await;
    let reviewable = advance_create_to_review(&mut client, &rejected_document, begun).await;
    let rejected = complete_review(&mut client, &reviewable, "rejected").await;
    let rejected_change = context(&rejected)["change_id"].clone();
    let rejected_finished = finish(
        &mut client,
        &rejected,
        "rejected",
        "not_achieved",
        json!(["Open a newly scoped Promotion Slice after correcting the rejected proposal."]),
        None,
        json!([]),
    )
    .await;
    assert_eq!(context(&rejected_finished)["run"]["status"], "completed");
    assert_result(
        &pool,
        &rejected_slice,
        &rejected_change,
        "blocked",
        "rejected",
        false,
    )
    .await;

    let partial_slice = open_promotion(&mut client, &repo, "partial").await;
    let mut partial_document = original.clone();
    partial_document["title"] = json!("Partially achieved Promotion fixture declaration");
    let begun=route(&mut client,"command","knowledge.change_begin",begin_create_request(&partial_document,
        json!({"kind":"promotion_slice","scope_id":partial_slice["scope_id"],"slice_id":partial_slice["id"],
            "slice_revision":partial_slice["revision"]}),Uuid::new_v4())).await;
    let committed = commit_create_from_current(&mut client, partial_document, begun).await;
    let current = query_current(&mut client, &committed.receipt["change_id"]).await;
    route(
        &mut client,
        "command",
        "knowledge.change_settle_effects",
        action_params(&current["actions"][0]).clone(),
    )
    .await;
    let current = query_current(&mut client, &committed.receipt["change_id"]).await;
    let ctx = context(&current);
    let partial_finished = finish(
        &mut client,
        &current,
        "applied",
        "partial",
        json!(["Replan the remaining bounded adoption work in a new Slice."]),
        Some(committed.receipt["id"].clone()),
        ctx["effects_report"]["effects"].clone(),
    )
    .await;
    assert_eq!(context(&partial_finished)["run"]["status"], "completed");
    assert_result(
        &pool,
        &partial_slice,
        &committed.receipt["change_id"],
        "blocked",
        "applied",
        true,
    )
    .await;

    let unchanged = knowledge_lifecycle_support::commit_create(&mut client, original.clone()).await;
    knowledge_lifecycle_support::settle_and_finish(&mut client, &unchanged).await;
    let no_change_slice = open_promotion(&mut client, &repo, "no-change").await;
    let begun=route(&mut client,"command","knowledge.change_begin",begin_create_request(&original,
        json!({"kind":"promotion_slice","scope_id":no_change_slice["scope_id"],"slice_id":no_change_slice["id"],
            "slice_revision":no_change_slice["revision"]}),Uuid::new_v4())).await;
    let reviewable = advance_create_to_review_with_identity(
        &mut client,
        &original,
        begun,
        vec![json!({"client_label":"knowledge-document",
            "unit_id":unchanged.receipt["applied_operations"][0]["unit_id"],
            "revision":unchanged.receipt["applied_operations"][0]["revision"],
            "basis":"Exact prior canonical document read and compared field-for-field.",
            "ambiguous":false})],
    )
    .await;
    let no_change = complete_review(&mut client, &reviewable, "no_change").await;
    let no_change_id = context(&no_change)["change_id"].clone();
    let no_change_finished = finish(
        &mut client,
        &no_change,
        "no_change",
        "achieved",
        json!([]),
        None,
        json!([]),
    )
    .await;
    assert_eq!(context(&no_change_finished)["run"]["status"], "completed");
    assert_result(
        &pool,
        &no_change_slice,
        &no_change_id,
        "completed",
        "no_change",
        false,
    )
    .await;
    client.finish().await;
}
