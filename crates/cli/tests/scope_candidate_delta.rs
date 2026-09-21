//! Live PostgreSQL/MCP acceptance for the normalized WP5 candidate graph.
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, public_call, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use tect_postgres::admin;
use uuid::Uuid;

fn id(value: &Value) -> Uuid {
    Uuid::parse_str(value.as_str().unwrap()).unwrap()
}

fn planning_ref(context: &Value) -> Uuid {
    context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|source| source["kind"] == "planning_input")
        .map(|source| id(&source["id"]))
        .unwrap()
}

fn payload(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    assert_ne!(response["result"]["isError"], true, "{response}");
    serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap()).unwrap()
}

async fn measured_delta(client: &mut Mcp, arguments: Value) -> (Value, usize) {
    let request = public_call("scope_candidate_delta", arguments);
    let request_bytes = serde_json::to_vec(&request).unwrap().len();
    let response = client.exchange("tools/call", request).await;
    let bytes = request_bytes + serde_json::to_vec(&response).unwrap().len();
    (payload(&response), bytes)
}

async fn delta(client: &mut Mcp, arguments: Value) -> Value {
    measured_delta(client, arguments).await.0
}

async fn delta_error(client: &mut Mcp, arguments: Value) -> Value {
    let response = client
        .exchange(
            "tools/call",
            public_call("scope_candidate_delta", arguments),
        )
        .await;
    assert_eq!(response["result"]["isError"], true, "{response}");
    serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn candidate_delta_normalized_graph_is_atomic_cyclic_safe_and_compact() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("candidate-delta.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-candidate-delta-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let native = Uuid::new_v4().to_string();
    let workspace_key = format!("candidate-delta-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;
    client.call("open_workspace", json!({})).await;

    let started = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Normalize the candidate graph."}),
        )
        .await;
    let program = id(&started["program"]["id"]);
    client.call("save_program", json!({
        "program_id":program,"revision":1,"input_cursor":1,"name":"WP5 fixture",
        "intent":"Prove normalized delta semantics","basis":"Live PostgreSQL acceptance",
        "boundaries":"Candidate planning only","constraints":"Atomic graph writes",
        "success":"M:N coverage, blockers, evidence and supersession remain durable","complete":true
    })).await;
    let begun = client
        .call(
            "begin_candidate_set",
            json!({
                "request_id":Uuid::new_v4(),"program_id":program,"program_revision":2,
                "boundary":"finite","input":"Create three candidates and explicit goal relations."
            }),
        )
        .await;
    let set = id(&begun["context"]["candidate_set"]["id"]);
    let source = planning_ref(&begun["context"]);
    let [a, b, c] = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let [g1, g2, g3] = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let evidence = Uuid::new_v4();
    let goal_evidence = Uuid::new_v4();
    let blocker = Uuid::new_v4();

    let request_one = json!({
        "candidate_set_id":set,"expected_revision":1,"idempotency_key":"wp5-graph-1",
        "operations":[
            {"operation":"goal.add","goal_id":g1,"text":"Goal one","finite":true,"source_ref_id":source},
            {"operation":"goal.add","goal_id":g2,"text":"Goal two","finite":true,"source_ref_id":source},
            {"operation":"goal.add","goal_id":g3,"text":"Blocked goal","finite":true,"source_ref_id":source},
            {"operation":"candidate.add","candidate_id":a,"title":"Candidate A"},
            {"operation":"candidate.add","candidate_id":b,"title":"Candidate B"},
            {"operation":"candidate.add","candidate_id":c,"title":"Candidate C"},
            {"operation":"candidate.update","candidate_id":b,"expected_revision":1,"title":"Candidate B refined"},
            {"operation":"coverage.link","candidate_id":a,"goal_id":g1},
            {"operation":"coverage.link","candidate_id":a,"goal_id":g2},
            {"operation":"coverage.link","candidate_id":b,"goal_id":g1},
            {"operation":"evidence.add","evidence_id":evidence,"target_kind":"candidate","target_id":a,
                "summary":"Candidate A evidence","source_ref_id":source},
            {"operation":"evidence.add","evidence_id":goal_evidence,"target_kind":"goal","target_id":g1,
                "summary":"Goal one evidence","source_ref_id":source},
            {"operation":"blocker.add","blocker_id":blocker,"goal_id":g3,
                "summary":"Awaiting authority","source_ref_id":source},
            {"operation":"goal.resolve","goal_id":g1,"expected_revision":1}
        ]
    });
    let (first, bytes_one) = measured_delta(&mut client, request_one.clone()).await;
    assert_eq!(first["to_revision"], 2);
    assert_eq!(delta(&mut client, request_one.clone()).await, first);
    let mut conflict = request_one.clone();
    conflict["operations"][3]["title"] = json!("Changed");
    assert_eq!(
        delta_error(&mut client, conflict).await["error"]["code"],
        "input_conflict"
    );

    let request_two = json!({
        "candidate_set_id":set,"expected_revision":2,"idempotency_key":"wp5-graph-2",
        "operations":[
            {"operation":"candidate.supersede","candidate_id":a,"replacement_candidate_id":b,"expected_revision":1},
            {"operation":"coverage.link","candidate_id":b,"goal_id":g2},
            {"operation":"evidence.update","evidence_id":evidence,"expected_revision":1,
                "summary":"Updated candidate evidence","source_ref_id":source},
            {"operation":"blocker.update","blocker_id":blocker,"expected_revision":1,
                "summary":"Authority still pending","source_ref_id":source},
            {"operation":"evidence.remove","evidence_id":goal_evidence,"expected_revision":1}
        ]
    });
    let (second, bytes_two) = measured_delta(&mut client, request_two).await;
    assert_eq!(second["to_revision"], 3);

    let request_three = json!({
        "candidate_set_id":set,"expected_revision":3,"idempotency_key":"wp5-graph-3",
        "operations":[
            {"operation":"candidate.supersede","candidate_id":b,"replacement_candidate_id":c,"expected_revision":2},
            {"operation":"coverage.link","candidate_id":c,"goal_id":g1},
            {"operation":"coverage.link","candidate_id":c,"goal_id":g2},
            {"operation":"evidence.remove","evidence_id":evidence,"expected_revision":2}
        ]
    });
    let (third, bytes_three) = measured_delta(&mut client, request_three).await;
    assert_eq!(third["to_revision"], 4);
    let compact_bytes = bytes_one + bytes_two + bytes_three;
    assert!(
        compact_bytes <= 12 * 1024,
        "compact MCP exchange used {compact_bytes} bytes"
    );

    let self_cycle = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-self-cycle",
            "operations":[{"operation":"candidate.supersede","candidate_id":c,
                "replacement_candidate_id":c,"expected_revision":1}]
        }),
    )
    .await;
    assert_eq!(self_cycle["error"]["code"], "invalid_arguments");
    let indirect_cycle = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-indirect-cycle",
            "operations":[{"operation":"candidate.supersede","candidate_id":c,
                "replacement_candidate_id":a,"expected_revision":1}]
        }),
    )
    .await;
    assert_eq!(indirect_cycle["error"]["code"], "invalid_arguments");
    let remove_referenced = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-remove-referenced",
            "operations":[{"operation":"candidate.remove","candidate_id":c,"expected_revision":1}]
        }),
    )
    .await;
    assert_eq!(remove_referenced["error"]["code"], "invalid_arguments");
    let orphan = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-orphan",
            "operations":[{"operation":"coverage.link","candidate_id":Uuid::new_v4(),"goal_id":g1}]
        }),
    )
    .await;
    assert_eq!(orphan["error"]["code"], "not_found");
    let stale = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":3,"idempotency_key":"wp5-stale",
            "operations":[{"operation":"coverage.link","candidate_id":c,"goal_id":g3}]
        }),
    )
    .await;
    assert_eq!(stale["error"]["code"], "stale_revision");

    let rolled_back = Uuid::new_v4();
    let atomic = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-atomic",
            "operations":[
                {"operation":"candidate.add","candidate_id":rolled_back,"title":"Must roll back"},
                {"operation":"coverage.link","candidate_id":rolled_back,"goal_id":Uuid::new_v4()}
            ]
        }),
    )
    .await;
    assert_eq!(atomic["error"]["code"], "not_found");

    let incomplete = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-incomplete",
            "operations":[{"operation":"blocker.remove","blocker_id":blocker,"expected_revision":2}]
        }),
    )
    .await;
    assert_eq!(
        incomplete["error"]["refusal"]["code"],
        "COVERAGE_INCOMPLETE"
    );
    let final_receipt = delta(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":4,"idempotency_key":"wp5-unblock",
            "operations":[
                {"operation":"coverage.link","candidate_id":c,"goal_id":g3},
                {"operation":"blocker.remove","blocker_id":blocker,"expected_revision":2}
            ]
        }),
    )
    .await;
    assert_eq!(final_receipt["to_revision"], 5);
    let invalid_source = delta_error(
        &mut client,
        json!({
            "candidate_set_id":set,"expected_revision":5,"idempotency_key":"wp5-invalid-source",
            "operations":[{"operation":"evidence.add","evidence_id":Uuid::new_v4(),
                "target_kind":"candidate","target_id":c,"summary":"Wrong source identity",
                "source_ref_id":Uuid::new_v4()}]
        }),
    )
    .await;
    assert_eq!(invalid_source["error"]["code"], "not_found");

    let candidate_to_goals: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_coverage WHERE candidate_set_id=$1 AND candidate_id=$2")
        .bind(set).bind(a).fetch_one(&pool).await.unwrap();
    let goal_to_candidates: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_coverage WHERE candidate_set_id=$1 AND goal_id=$2")
        .bind(set).bind(g1).fetch_one(&pool).await.unwrap();
    let supersessions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_supersessions WHERE candidate_set_id=$1",
    )
    .bind(set)
    .fetch_one(&pool)
    .await
    .unwrap();
    let live_evidence: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_evidence WHERE candidate_set_id=$1 AND NOT deleted")
        .bind(set).fetch_one(&pool).await.unwrap();
    let live_blockers: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_blockers WHERE candidate_set_id=$1 AND NOT deleted")
        .bind(set).fetch_one(&pool).await.unwrap();
    let rolled_back_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_delta_candidates WHERE candidate_set_id=$1 AND candidate_id=$2")
        .bind(set).bind(rolled_back).fetch_one(&pool).await.unwrap();
    assert_eq!(candidate_to_goals, 2);
    assert!(goal_to_candidates >= 2);
    assert_eq!(supersessions, 2);
    assert_eq!(live_evidence, 0);
    assert_eq!(live_blockers, 0);
    assert_eq!(rolled_back_count, 0);

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut resumed = Mcp::start(&socket, &config, &native, &workspace_key).await;
    let restored_response = resumed
        .exchange(
            "tools/call",
            public_call(
                "scope_candidate_delta_status",
                json!({"candidate_set_id":set,"idempotency_key":"wp5-unblock"}),
            ),
        )
        .await;
    let restored = payload(&restored_response);
    assert_eq!(restored, final_receipt);

    println!(
        "wp5_delta_evidence set={set} revision=5 candidates=3 coverage_a={candidate_to_goals} coverage_g1={goal_to_candidates} supersessions={supersessions} requests=3 compact_bytes={compact_bytes} crash_receipt=restored"
    );
    resumed.finish().await;
    daemon.crash().await;
}
