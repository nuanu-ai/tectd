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
    commit_create, context, method_reads, omit_nulls, settle_and_finish, settle_and_finish_receipt,
};
use knowledge_operation_support::{SingleOperation, commit_single};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn request(unit: &Value, owner: Value, request_id: Uuid, marker: &str, erasure: &str) -> Value {
    json!({"request_id":request_id,"intent":format!("{marker}:intent"),
        "desired_outcome":format!("{marker}:outcome"),"sources":[],
        "operation_hints":[{"client_label":"already-erased","operation":"erase","unit_id":unit,
            "expected_revision":1,"expected_lifecycle":"erased","reason":format!("{marker}:reason"),
            "authority_basis":"Current authenticated workspace owner.","depends_on_labels":[]}],
        "owner":owner,"completion":{"canonical_result":true,"exact_delivery":true,
            "impact_recorded":true,"search":"not_required","erasure":erasure},
        "delivery_mode":"whole"})
}

fn terminal_params(current: &Value, marker: &str) -> Value {
    let action = &current["actions"][0];
    let mut params = action_params(action).clone();
    params["output"]["method_reads"] = method_reads(action);
    params["output"]["body"] = json!(format!("{marker}:body"));
    params["output"]["data"] = json!({"phase":"kc-result-handoff","data":{
        "canonical":"no_change","user_outcome":"achieved","summary":format!("{marker}:summary"),
        "remaining_work":[],"effects":[]}});
    params["output"]["verdict"] = json!("complete");
    params["output"]["outcome"] = json!("completed");
    params["output"]["transition"] = json!("complete");
    params["output"]["findings"] = json!([]);
    params["output"]["dispositions"] = json!([]);
    omit_nulls(&mut params);
    params
}

async fn finish(client: &mut Mcp, current: &Value, marker: &str) -> (Value, Value) {
    let params = terminal_params(current, marker);
    let result = route(
        client,
        "command",
        "knowledge.change_phase_complete",
        params.clone(),
    )
    .await;
    (result, params)
}

fn promotion_draft() -> Value {
    json!({"coverage_summary":"Exercise already-erased Promotion NoChange.","nodes":[{
        "kind":"work","identity":{"local":"erased-no-change"},"title":"Close fulfilled erasure",
        "outcome":"The already fulfilled erase closes without semantic resurrection",
        "includes":["opaque proof"],"excludes":["new publication"],"dependencies":[],
        "proof":["existing suppression ledger"],"pipeline":"slice.promote-to-durable-knowledge",
        "pipeline_reason":"Exercise the exact Promotion-owned shortcut.","source_result_ids":[]}],
        "supersessions":[]})
}

async fn open_promotion(client: &mut Mcp, repo: &std::path::Path) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope=route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(client, &scope["created"]["planning"], promotion_draft()).await;
    let reviewed = review(client, &saved).await;
    route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await["created"]
        .clone()
}

#[tokio::test]
async fn already_suppressed_erase_uses_only_opaque_kc12_and_is_idempotent() {
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
    let socket = root.join("erased-no-change.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk2-erased-no-change-{}", Uuid::new_v4()),
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
        &format!("dk2-erased-no-change-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let created = commit_create(&mut client, fixture["document"].clone()).await;
    settle_and_finish(&mut client, &created).await;
    let unit = created.receipt["applied_operations"][0]["unit_id"].clone();
    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(unit.clone()),
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
    settle_and_finish_receipt(&mut client, &erased["applied_erased"]).await;
    let unit_id = Uuid::parse_str(unit.as_str().unwrap()).unwrap();
    let before:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM knowledge_publication_events WHERE unit_id=$1),(SELECT erasure_sequence FROM knowledge_suppression_ledger WHERE unit_id=$1)").bind(unit_id).fetch_one(&pool).await.unwrap();

    let marker = format!("ERASED-NO-CHANGE-{}", Uuid::new_v4());
    let request_id = Uuid::new_v4();
    let begun = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        request(
            &unit,
            json!({"kind":"workspace"}),
            request_id,
            &marker,
            "owned_live_copies",
        ),
    )
    .await;
    let ctx = context(&begun);
    assert_eq!(ctx["run"]["delivery_mode"], "phasewise");
    assert_eq!(ctx["run"]["current_phase_id"], "kc-result-handoff");
    assert_eq!(
        ctx["erased_no_change_proof"]["operations"][0]["unit_id"],
        unit
    );
    assert_eq!(ctx["outputs"], json!([]));
    let change_id = Uuid::parse_str(ctx["change_id"].as_str().unwrap()).unwrap();
    let run_id = Uuid::parse_str(ctx["run"]["id"].as_str().unwrap()).unwrap();
    let safe:(bool,i64,i64)=sqlx::query_as("SELECT c.intent IS NULL AND c.desired_outcome IS NULL AND c.sources IS NULL AND c.operation_hints IS NULL AND c.completion IS NULL AND r.baseline IS NULL AND r.branch_plan IS NULL AND r.ready_to_commit IS NULL,(SELECT count(*) FROM knowledge_change_attempts WHERE run_id=r.id),(SELECT count(*) FROM knowledge_change_outputs WHERE run_id=r.id) FROM knowledge_lifecycle_changes c JOIN knowledge_change_runs r ON r.change_id=c.id WHERE c.id=$1 AND r.id=$2").bind(change_id).bind(run_id).fetch_one(&pool).await.unwrap();
    assert_eq!(safe, (true, 0, 0));
    let replay = route_error(
        &mut client,
        "command",
        "knowledge.change_begin",
        request(
            &unit,
            json!({"kind":"workspace"}),
            request_id,
            &marker,
            "owned_live_copies",
        ),
    )
    .await;
    assert_eq!(replay["error"]["code"], "knowledge_payload_erased");
    let stored_proof = ctx["erased_no_change_proof"].clone();
    sqlx::query("UPDATE knowledge_change_runs SET erased_no_change_proof=jsonb_set(erased_no_change_proof,'{operations,0,erasure_sequence}',to_jsonb((erased_no_change_proof->'operations'->0->>'erasure_sequence')::bigint+1)) WHERE id=$1")
        .bind(run_id).execute(&pool).await.unwrap();
    let refused_cursor: i64 =
        sqlx::query_scalar("SELECT revision FROM knowledge_change_runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let forged = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        terminal_params(&begun, "FORGED-PROOF"),
    )
    .await;
    assert_eq!(forged["error"]["code"], "knowledge_payload_erased");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT revision FROM knowledge_change_runs WHERE id=$1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap(),
        refused_cursor
    );
    sqlx::query("UPDATE knowledge_change_runs SET erased_no_change_proof=$2 WHERE id=$1")
        .bind(run_id)
        .bind(stored_proof)
        .execute(&pool)
        .await
        .unwrap();
    let (completed, terminal_params) = finish(&mut client, &begun, &marker).await;
    assert_eq!(context(&completed)["run"]["status"], "completed");
    let repeated = route_error(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        terminal_params,
    )
    .await;
    assert_eq!(repeated["error"]["code"], "knowledge_payload_erased");
    let after:(i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM knowledge_publication_events WHERE unit_id=$1),(SELECT erasure_sequence FROM knowledge_suppression_ledger WHERE unit_id=$1)").bind(unit_id).fetch_one(&pool).await.unwrap();
    assert_eq!(after, before);
    let leaked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM (SELECT to_jsonb(c)::text body FROM knowledge_lifecycle_changes c WHERE c.id=$1 UNION ALL SELECT to_jsonb(r)::text FROM knowledge_change_runs r WHERE r.id=$2 UNION ALL SELECT to_jsonb(o)::text FROM knowledge_change_operations o WHERE o.change_id=$1 UNION ALL SELECT to_jsonb(a)::text FROM knowledge_change_attempts a WHERE a.run_id=$2 UNION ALL SELECT to_jsonb(o)::text FROM knowledge_change_outputs o WHERE o.run_id=$2 UNION ALL SELECT to_jsonb(q)::text FROM knowledge_lifecycle_command_receipts q WHERE q.erased_change_id=$1) rows WHERE body LIKE '%'||$3||'%')").bind(change_id).bind(run_id).bind(&marker).fetch_one(&pool).await.unwrap();
    assert!(!leaked);

    let mut live_document = fixture["document"].clone();
    live_document["title"] = json!("Still-live mixed target");
    live_document["sources"][0]["snapshot"]["uri"] = json!("urn:tect:dk2:source:still-live");
    live_document["sources"][0]["snapshot"]["text"] =
        json!("This independent target remains live.");
    let live = commit_create(&mut client, live_document).await;
    settle_and_finish(&mut client, &live).await;
    let live_unit = live.receipt["applied_operations"][0]["unit_id"].clone();
    let invalid_count: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_lifecycle_changes")
        .fetch_one(&pool)
        .await
        .unwrap();
    let mut invalid = request(
        &unit,
        json!({"kind":"workspace"}),
        Uuid::new_v4(),
        "invalid",
        "owned_live_copies",
    );
    invalid["operation_hints"][0]["expected_revision"] = json!(2);
    assert_eq!(
        route_error(&mut client, "command", "knowledge.change_begin", invalid).await["error"]["code"],
        "knowledge_payload_erased"
    );
    let restore = route_error(
        &mut client,
        "command",
        "knowledge.change_begin",
        request(
            &unit,
            json!({"kind":"workspace"}),
            Uuid::new_v4(),
            "restore",
            "restore_safe",
        ),
    )
    .await;
    assert_eq!(restore["error"]["code"], "knowledge_payload_erased");
    let mut supplied = request(
        &unit,
        json!({"kind":"workspace"}),
        Uuid::new_v4(),
        "supplied-source",
        "owned_live_copies",
    );
    supplied["sources"] = fixture["document"]["sources"].clone();
    assert_eq!(
        route_error(&mut client, "command", "knowledge.change_begin", supplied).await["error"]["code"],
        "knowledge_payload_erased"
    );
    let mut mixed = request(
        &unit,
        json!({"kind":"workspace"}),
        Uuid::new_v4(),
        "mixed-live",
        "owned_live_copies",
    );
    mixed["operation_hints"].as_array_mut().unwrap().push(
        json!({"client_label":"still-live","operation":"erase","unit_id":live_unit,
            "expected_revision":1,"expected_lifecycle":"active",
            "reason":"This live target makes the shortcut ineligible.",
            "authority_basis":"Current owner.","depends_on_labels":[]}),
    );
    assert_eq!(
        route_error(&mut client, "command", "knowledge.change_begin", mixed).await["error"]["code"],
        "knowledge_payload_erased"
    );
    let live_read = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":live_unit,"revision":1}),
    )
    .await;
    assert_eq!(live_read["document"]["lifecycle"], "active");
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM knowledge_lifecycle_changes")
            .fetch_one(&pool)
            .await
            .unwrap(),
        invalid_count
    );

    let slice = open_promotion(&mut client, &repo).await;
    let promotion_owner = json!({"kind":"promotion_slice","scope_id":slice["scope_id"],"slice_id":slice["id"],"slice_revision":slice["revision"]});
    let promotion = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        request(
            &unit,
            promotion_owner,
            Uuid::new_v4(),
            "PROMOTION-OPAQUE",
            "owned_live_copies",
        ),
    )
    .await;
    let promotion_change =
        Uuid::parse_str(context(&promotion)["change_id"].as_str().unwrap()).unwrap();
    let promotion_run =
        Uuid::parse_str(context(&promotion)["run"]["id"].as_str().unwrap()).unwrap();
    let (finished, _) = finish(&mut client, &promotion, "PROMOTION-KC12-MARKER").await;
    assert_eq!(context(&finished)["run"]["status"], "completed");
    let result:(bool,bool,bool,i64)=sqlx::query_as("SELECT r.payload_erased,r.summary IS NULL AND r.evidence IS NULL AND r.scope_impact IS NULL AND r.remaining_work IS NULL,i.payload_erased AND i.input IS NULL,(SELECT count(*) FROM slice_pipeline_runs p WHERE p.slice_id=r.slice_id) FROM slice_results r JOIN slice_planning_inputs i ON i.source_result_id=r.id WHERE r.knowledge_change_id=$1 AND r.knowledge_run_id=$2").bind(promotion_change).bind(promotion_run).fetch_one(&pool).await.unwrap();
    assert_eq!(result, (true, true, true, 0));
    assert_eq!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM slice_results WHERE knowledge_change_id=$1"
        )
        .bind(promotion_change)
        .fetch_one(&pool)
        .await
        .unwrap(),
        1
    );
    client.finish().await;
    pool.close().await;
}
