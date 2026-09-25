#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[path = "pipeline_execution/full_support.rs"]
#[allow(dead_code)]
mod pipeline_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use knowledge_operation_support::{SingleOperation, commit_single};
use pipeline_support::{
    add_opaque_authority_labels, assert_forged_implementation_phase_rejected,
    assert_non_coding_definition, completion as base_completion, successful_route,
};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::{admin, enable_durable_knowledge};
use uuid::Uuid;

#[path = "pipeline_checkpoint/checkpoint_flow.rs"]
mod checkpoint_flow;
#[path = "pipeline_checkpoint/checkpoint_support.rs"]
mod checkpoint_support;
#[path = "pipeline_checkpoint/producer_flow.rs"]
mod producer_flow;

use checkpoint_flow::*;
use checkpoint_support::*;
use producer_flow::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn checkpoint_handoff_is_exact_replayable_and_rework_safe() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    enable_durable_knowledge(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-checkpoint.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("pipeline-checkpoint-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("pipeline-checkpoint-{}", Uuid::new_v4());
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;

    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
            "candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(
        &mut client,
        &scope["created"]["planning"],
        brainstorming_draft(),
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let producer_node = reviewed["draft"]["nodes"][0].clone();
    let opened = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &producer_node, Uuid::new_v4()),
    )
    .await;
    let scope_id = Uuid::parse_str(reviewed["scope"]["id"].as_str().unwrap()).unwrap();
    let program_id: Uuid = sqlx::query_scalar(
        "SELECT sc.program_id FROM native_scopes ns JOIN scope_candidate_sets sc ON sc.tenant_id=ns.tenant_id AND sc.workspace_id=ns.workspace_id AND sc.id=ns.source_candidate_set_id WHERE ns.id=$1",
    )
    .bind(scope_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let public = commit_create(
        &mut client,
        knowledge_document(
            "public-decision",
            "urn:fixture:public-decision",
            "workspace_members",
            program_id,
        ),
    )
    .await;
    let public_unit = public.receipt["applied_operations"][0]["unit_id"].clone();
    let private = commit_create(
        &mut client,
        knowledge_document(
            "private-research",
            "urn:fixture:private-research",
            "owners_only",
            program_id,
        ),
    )
    .await;
    let private_unit = private.receipt["applied_operations"][0]["unit_id"].clone();
    let mut producer = begin_run(
        &mut client,
        &reviewed["scope"],
        &opened["created"],
        decision_inquiry(),
        None,
    )
    .await;
    assert_non_coding_definition(&producer, "slice.deep-brainstorming");
    let (verdict, outcome, transition) = successful_route(&producer);
    let mut first = completion(&producer, verdict, outcome, transition, None, None);
    first["output"]["fields"]["topic_level"] = producer["inquiry"]["topic_level"].clone();
    first["output"]["fields"]["requested_outcome"] = json!("decision");
    assert_forged_implementation_phase_rejected(
        &mut client,
        &producer,
        first.clone(),
        "slice.deep-brainstorming",
    )
    .await;
    add_opaque_authority_labels(&mut first);
    producer = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        first,
    )
    .await["context"]
        .clone();
    assert_non_coding_definition(&producer, "slice.deep-brainstorming");
    assert!(contains_unit(&producer, &public_unit));
    assert!(!contains_unit(&producer, &private_unit));
    while producer["run"]["current_phase_ordinal"].as_u64().unwrap() < 5 {
        producer = advance(&mut client, producer).await;
    }
    let checkpoint = create_checkpoint(&mut client, &producer).await;
    assert_eq!(checkpoint["status"], "open");
    assert_eq!(checkpoint["producer_phase_id"], "B05");
    assert_eq!(
        checkpoint["basis"]["consumed_outputs"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(checkpoint["inquiry"], research_inquiry());
    assert!(checkpoint["basis"]["consumed_knowledge"].is_object());

    let input_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":checkpoint["producer_run_id"],
            "run_revision":checkpoint["producer_run_revision"],"phase_id":"B05",
            "input":"Attempt to bypass the exact checkpoint."}),
    )
    .await;
    assert_eq!(input_error["error"]["code"], "input_pending");

    let stale = route(
        &mut client,
        "query",
        "slice.candidates.context",
        json!({"scope_id":reviewed["scope"]["id"],"view":"overview","limit":100}),
    )
    .await;
    assert!(
        stale["checkpoints"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item == &checkpoint)
    );
    let refreshed = route(
        &mut client,
        "command",
        "slice.candidates.refresh",
        json!({"scope_id":stale["scope"]["id"],"candidate_set_id":stale["candidate_set"]["id"],
            "revision":stale["candidate_set"]["revision"],"request_id":Uuid::new_v4()}),
    )
    .await;
    let source_ref = checkpoint["checkpoint"].clone();
    let invalid = json!({"coverage_summary":"Invalid transitive wait dependency.","nodes":[
        existing_work(&producer_node),research_work("invalid",&source_ref,Some(&producer_node))],
        "supersessions":[]});
    let invalid_error = route_error(
        &mut client,
        "command",
        "slice.candidates.save",
        draft_request(&refreshed, invalid),
    )
    .await;
    assert!(matches!(
        invalid_error["error"]["code"].as_str(),
        Some("forbidden" | "invalid_arguments")
    ));
    let old_kind = json!({"coverage_summary":"A removed combined pipeline must not be newly selectable.",
        "nodes":[existing_work(&producer_node),{"kind":"work","identity":{"local":"old"},
            "title":"Attempt the removed combined research path","outcome":"A result",
            "includes":[],"excludes":[],"dependencies":[],"proof":["No execution"],
            "pipeline":"slice.research-to-durable-knowledge",
            "pipeline_reason":"Exercise the captured catalogue selection guard.",
            "source_result_ids":[]}],"supersessions":[]});
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.candidates.save",
            draft_request(&refreshed, old_kind)
        )
        .await["error"]["code"],
        "forbidden"
    );
    let valid = json!({"coverage_summary":"Two reviewed consumers expose the one-consumer CAS.",
        "nodes":[existing_work(&producer_node),research_work("first",&source_ref,None),
            research_work("second",&source_ref,None)],"supersessions":[]});
    let saved = route(
        &mut client,
        "command",
        "slice.candidates.save",
        draft_request(&refreshed, valid),
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let first = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][1], Uuid::new_v4()),
    )
    .await;
    let research_begin = begin_params(
        &reviewed["scope"],
        &first["created"],
        research_inquiry(),
        Some(&source_ref),
    );
    let mut wrong_continuation = research_begin.clone();
    wrong_continuation["request_id"] = json!(Uuid::new_v4());
    wrong_continuation["source_checkpoint"]["digest"] = json!("wrong-checkpoint-digest");
    assert!(matches!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.begin",
            wrong_continuation,
        )
        .await["error"]["code"]
            .as_str(),
        Some("forbidden" | "invalid_arguments" | "stale_context")
    ));
    let research = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        research_begin.clone(),
    )
    .await["created"]
        .clone();
    assert!(contains_unit(&research, &private_unit));
    assert!(!contains_unit(&research, &public_unit));
    let replayed_begin = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        research_begin.clone(),
    )
    .await;
    assert!(contains_unit(&replayed_begin["replay"], &private_unit));
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.open",
            open_slice(&reviewed, &reviewed["draft"]["nodes"][2], Uuid::new_v4())
        )
        .await["error"]["code"],
        "forbidden"
    );
    assert_eq!(research["source_checkpoint"], checkpoint["checkpoint"]);
    let bound_checkpoint = research["checkpoints"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["checkpoint"] == checkpoint["checkpoint"])
        .unwrap();
    assert_eq!(
        bound_checkpoint["producer_run_id"],
        checkpoint["producer_run_id"]
    );
    assert_eq!(bound_checkpoint["consumer_run_id"], research["run"]["id"]);

    let amended = route(
        &mut client,
        "command",
        "slice.candidates.input",
        json!({"scope_id":reviewed["scope"]["id"],
            "candidate_set_id":reviewed["candidate_set"]["id"],
            "revision":reviewed["candidate_set"]["revision"],
            "request_id":Uuid::new_v4(),
            "input":"Preserve the exact already-opened Research checkpoint lineage."}),
    )
    .await;
    let refreshed_bound = route(
        &mut client,
        "command",
        "slice.candidates.refresh",
        json!({"scope_id":amended["scope"]["id"],
            "candidate_set_id":amended["candidate_set"]["id"],
            "revision":amended["candidate_set"]["revision"],
            "request_id":Uuid::new_v4()}),
    )
    .await;
    let preserved_nodes = vec![
        existing_work(&reviewed["draft"]["nodes"][0]),
        existing_work(&reviewed["draft"]["nodes"][1]),
    ];
    let removed = &reviewed["draft"]["nodes"][2];
    let preserved = route(
        &mut client,
        "command",
        "slice.candidates.save",
        draft_request(
            &refreshed_bound,
            json!({"coverage_summary":"Preserve all exact opened candidates.",
                "nodes":preserved_nodes,"supersessions":[{
                    "candidate_id":removed["id"],"revision":removed["revision"],
                    "reason":"The exact checkpoint already has its one Research consumer.",
                    "replacements":[{"candidate_id":reviewed["draft"]["nodes"][1]["id"],
                        "revision":reviewed["draft"]["nodes"][1]["revision"]}],
                    "source_result_ids":[]}] }),
        ),
    )
    .await;
    let preserved = review(&mut client, &preserved).await;
    assert_eq!(
        preserved["draft"]["nodes"][1]["source_checkpoint"],
        checkpoint["checkpoint"]
    );

    let resolution = complete_research_and_accept(&mut client, research, &checkpoint).await;
    let completed = resolution.completed;
    let accepted = resolution.accepted;
    let replay_resolve = resolution.replay_resolve;

    let decided =
        rework_and_complete_producer(&mut client, &pool, &accepted, &replay_resolve).await;

    let sibling = admin::enroll_host(
        &pool,
        Some(enrollment.tenant_id),
        vec![root.to_string_lossy().into_owned()],
    )
    .await
    .unwrap();
    let sibling_config = root.join("revoked-host.json");
    host_file(&sibling_config, &sibling.auth);
    let mut revoked = Mcp::start(
        &socket,
        &sibling_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    revoked.call("open_workspace", json!({})).await;
    admin::revoke_host(&pool, sibling.auth.host_id)
        .await
        .unwrap();
    for (operation, route_name, params) in [
        (
            "query",
            "slice.pipeline.context",
            json!({"run_id":completed["context"]["run"]["id"]}),
        ),
        ("command", "slice.pipeline.begin", research_begin.clone()),
    ] {
        let denied = revoked
            .call_error(operation, json!({"route":route_name,"params":params}))
            .await;
        assert_eq!(denied["error"]["code"], "unauthorized");
        assert!(!denied.to_string().contains(private_unit.as_str().unwrap()));
    }
    revoked.finish().await;

    let private_id = Uuid::parse_str(private_unit.as_str().unwrap()).unwrap();
    let erased = commit_single(&mut client, erase(private_unit.clone())).await;
    assert_eq!(
        erased["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    let checkpoint_id =
        Uuid::parse_str(checkpoint["checkpoint"]["checkpoint_id"].as_str().unwrap()).unwrap();
    let erased_lineage: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM pipeline_research_checkpoints WHERE id=$1 AND payload_erased AND digest IS NULL AND basis IS NULL),\
         (SELECT count(*) FROM slice_pipeline_inputs WHERE checkpoint_id=$1 AND payload_erased AND input='[erased]' AND input_digest IS NULL AND checkpoint_digest IS NULL AND request_payload IS NULL AND result_payload IS NULL),\
         (SELECT count(*) FROM pipeline_checkpoint_receipts WHERE checkpoint_id=$1 AND payload_erased AND request_payload IS NULL AND result_payload IS NULL),\
         (SELECT count(*) FROM knowledge_owned_copies WHERE unit_id=$2 AND NOT redacted)",
    )
    .bind(checkpoint_id)
    .bind(private_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(erased_lineage, (1, 1, 1, 0));
    assert!(
        sqlx::query_scalar::<_, i64>(
            "SELECT count(*) FROM slice_pipeline_phase_outputs WHERE run_id=$1 AND payload_erased",
        )
        .bind(Uuid::parse_str(decided["context"]["run"]["id"].as_str().unwrap()).unwrap())
        .fetch_one(&pool)
        .await
        .unwrap()
            > 0
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.checkpoint.resolve",
            replay_resolve,
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
