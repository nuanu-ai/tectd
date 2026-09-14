#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    begin_create_request, commit_create_from_current, context, settle_and_finish,
};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn promotion_draft() -> Value {
    json!({
        "coverage_summary":"One bounded durable publication is owned by Knowledge Change.",
        "nodes":[{
            "kind":"work","identity":{"local":"promotion"},
            "title":"Publish the exact fixture declaration",
            "outcome":"The reviewed declaration is current durable knowledge",
            "includes":["qualified publication","managed result"],
            "excludes":["new research","operational execution"],
            "dependencies":[],"proof":["Knowledge Change publisher receipt"],
            "pipeline":"slice.promote-to-durable-knowledge",
            "pipeline_reason":"The bounded outcome is durable creation from available evidence.",
            "source_result_ids":[]
        }],
        "supersessions":[]
    })
}

#[tokio::test]
async fn promotion_change_writes_one_managed_result_and_planning_input() {
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
    let socket = root.join("dk2-promotion.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk2-promotion-{}", Uuid::new_v4()),
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
        &format!("dk2-promotion-{}", Uuid::new_v4()),
    )
    .await;

    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({
            "request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],
            "candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let planning = &scope["created"]["planning"];
    let saved = save(&mut client, planning, promotion_draft()).await;
    let promotion = saved["draft"]["nodes"][0].clone();
    let reviewed = review(&mut client, &saved).await;
    let opened = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &promotion, Uuid::new_v4()),
    )
    .await;
    let slice = &opened["created"];
    assert_eq!(slice["pipeline"], "slice.promote-to-durable-knowledge");
    assert!(slice["pipeline_run_id"].is_null());

    let external = route_error(
        &mut client,
        "command",
        "slice.result.record",
        json!({
            "request_id":Uuid::new_v4(),"scope_id":slice["scope_id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "outcome":"completed","summary":"External completion must be refused.",
            "evidence":[{"kind":"test","reference":"promotion fixture","observation":"bypass attempt"}],
            "scope_impact":"none","remaining_work":"Knowledge Change remains required."
        }),
    )
    .await;
    assert_eq!(external["error"]["code"], "forbidden");

    let fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let document = fixture["document"].clone();
    let begin_request = begin_create_request(
        &document,
        json!({"kind":"promotion_slice","scope_id":slice["scope_id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"]}),
        Uuid::new_v4(),
    );
    let begun = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_request.clone(),
    )
    .await;
    let replay = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_request,
    )
    .await;
    assert_eq!(replay["replay"], begun["created"]);
    let second = route_error(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_create_request(
            &document,
            json!({"kind":"promotion_slice","scope_id":slice["scope_id"],
                "slice_id":slice["id"],"slice_revision":slice["revision"]}),
            Uuid::new_v4(),
        ),
    )
    .await;
    assert_eq!(second["error"]["code"], "forbidden");

    let committed = commit_create_from_current(&mut client, document, begun).await;
    let change_id = Uuid::parse_str(committed.receipt["change_id"].as_str().unwrap()).unwrap();
    let run_id = Uuid::parse_str(committed.receipt["run_id"].as_str().unwrap()).unwrap();
    let scope_id = Uuid::parse_str(slice["scope_id"].as_str().unwrap()).unwrap();
    let slice_id = Uuid::parse_str(slice["id"].as_str().unwrap()).unwrap();
    let before: (i64, i64, i64) = sqlx::query_as(
        "SELECT s.revision,c.revision,c.latest_input FROM native_scopes s \
         JOIN slice_candidate_sets c ON c.tenant_id=s.tenant_id AND c.workspace_id=s.workspace_id \
         AND c.id=s.slice_candidate_set_id WHERE s.id=$1",
    )
    .bind(scope_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let finished = settle_and_finish(&mut client, &committed).await;
    assert_eq!(context(&finished)["run"]["status"], "completed");

    let result: (Uuid, String, Option<Uuid>, Uuid, Uuid, String, String) = sqlx::query_as(
        "SELECT id,provenance,pipeline_run_id,knowledge_change_id,knowledge_run_id, \
         knowledge_definition_digest,knowledge_publisher_receipt_digest FROM slice_results \
         WHERE knowledge_change_id=$1 AND knowledge_run_id=$2",
    )
    .bind(change_id)
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(result.1, "knowledge_change_managed");
    assert_eq!(result.2, None);
    assert_eq!(result.3, change_id);
    assert_eq!(result.4, run_id);
    let run_definition_digest: String = sqlx::query_scalar(
        "SELECT definition_digest FROM knowledge_change_runs WHERE change_id=$1 AND id=$2",
    )
    .bind(change_id)
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(result.5, run_definition_digest);
    assert_eq!(result.6, committed.receipt["digest"].as_str().unwrap());
    let after: (i64, i64, i64, String, i64, i64, i64) = sqlx::query_as(
        "SELECT s.revision,c.revision,c.latest_input,n.state,n.revision, \
         (SELECT count(*) FROM slice_planning_inputs i WHERE i.source_result_id=$2), \
         (SELECT count(*) FROM slice_pipeline_runs r WHERE r.slice_id=$3) \
         FROM native_scopes s JOIN slice_candidate_sets c \
         ON c.tenant_id=s.tenant_id AND c.workspace_id=s.workspace_id \
         AND c.id=s.slice_candidate_set_id JOIN native_slices n \
         ON n.tenant_id=s.tenant_id AND n.workspace_id=s.workspace_id \
         AND n.scope_id=s.id WHERE s.id=$1 AND n.id=$3",
    )
    .bind(scope_id)
    .bind(result.0)
    .bind(slice_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        after,
        (
            before.0 + 1,
            before.1 + 1,
            before.2 + 1,
            "completed".into(),
            slice["revision"].as_i64().unwrap() + 1,
            1,
            0
        )
    );

    let results = route(
        &mut client,
        "query",
        "slice.candidates.context",
        json!({"scope_id":scope_id,"view":"results","limit":10}),
    )
    .await;
    let managed = results["items"]
        .as_array()
        .unwrap()
        .iter()
        .find(|value| value["id"] == result.0.to_string())
        .unwrap();
    assert_eq!(
        managed["knowledge_provenance"]["change_id"],
        change_id.to_string()
    );
    assert_eq!(
        managed["knowledge_provenance"]["run_id"],
        run_id.to_string()
    );

    let terminal_request: Value = sqlx::query_scalar(
        "SELECT request_payload FROM knowledge_lifecycle_command_receipts \
         WHERE (request_payload->>'change_id')::uuid=$1 AND operation='phase_complete' \
         AND request_payload->>'phase_id'='kc-result-handoff'",
    )
    .bind(change_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let terminal_replay = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        terminal_request,
    )
    .await;
    assert!(terminal_replay.get("replay").is_some());
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM slice_results WHERE knowledge_run_id=$1), \
         (SELECT count(*) FROM slice_planning_inputs WHERE source_result_id=$2), \
         (SELECT count(*) FROM slice_pipeline_runs WHERE slice_id=$3)",
    )
    .bind(run_id)
    .bind(result.0)
    .bind(slice_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(counts, (1, 1, 0));
    client.finish().await;
}
