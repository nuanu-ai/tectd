#[path = "pipeline_execution_full/cold_recovery.rs"]
mod cold_recovery;
#[path = "pipeline_execution/full_support.rs"]
mod full_support;
#[path = "pipeline_execution_full/initial_rework.rs"]
mod initial_rework;
mod recovery_support;
#[path = "pipeline_execution_full/source_amendment.rs"]
mod source_amendment;
#[path = "native_planning/support.rs"]
mod support;

use full_support::{completion, refresh_knowledge, replace_ledger, successful_route};
use recovery_support::{
    Daemon, Mcp, host_file, private_temp, public_call, tagged_url, tool_payload,
};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

fn full_draft() -> Value {
    json!({"coverage_summary":"Full design through execution lifecycle","nodes":[{
        "kind":"work","identity":{"local":"full"},
        "title":"Design and implement the complete bounded behavior",
        "outcome":"The behavior is specified, implemented, verified and handed off",
        "includes":["design","implementation","verification","handoff"],
        "excludes":["production deployment","unrelated redesign"],"dependencies":[],
        "proof":["Specification traceability and focused verification"],
        "pipeline":"slice.full-design-to-execution",
        "pipeline_reason":"The task requires the full specification and execution chain",
        "why_lightweight_insufficient":"Cross-cutting specification, review and execution phases are all required.",
        "why_further_vertical_split_not_viable":"The bounded behavior shares one contract and one integrated acceptance boundary.",
        "source_result_ids":[]
    }],"supersessions":[]})
}

async fn advance(client: &mut Mcp, context: Value) -> Value {
    let (verdict, outcome, transition) = successful_route(&context);
    let request = completion(&context, verdict, outcome, transition, None, None);
    let response = client
        .exchange(
            "tools/call",
            public_call(
                "command",
                json!({"route":"slice.pipeline.phase.complete","params":request.clone()}),
            ),
        )
        .await;
    assert_ne!(
        response["result"]["isError"], true,
        "phase={} request={} response={}",
        context["run"]["current_phase_id"], request, response
    );
    let result = tool_payload(&response);
    assert!(result["result"].is_null());
    result["context"].clone()
}

fn replace_ledger_source(output: &mut Value, path: &str, source_digest: &str) {
    let ledger_artifact = output["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    let mut ledger: Value =
        serde_json::from_str(ledger_artifact["body"].as_str().unwrap()).unwrap();
    ledger["source"]["path"] = json!(path);
    ledger["source"]["digest"] = json!(source_digest);
    let ledger_body = serde_json::to_string(&ledger).unwrap();
    let ledger_digest = format!("{:x}", Sha256::digest(ledger_body.as_bytes()));
    ledger_artifact["body"] = json!(ledger_body);
    ledger_artifact["digest"] = json!(ledger_digest.clone());
    for receipt in output["validator_receipts"].as_array_mut().unwrap() {
        if let Some(bound) = receipt["artifacts"]
            .as_array_mut()
            .unwrap()
            .iter_mut()
            .find(|artifact| artifact["name"] == "requirements-ledger.json")
        {
            bound["digest"] = json!(ledger_digest);
        }
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn full_pipeline_reworks_reviews_resumes_and_completes_with_exact_artifacts() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-full.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-full-{}", Uuid::new_v4()),
    );
    let daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-full-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let planning = &opened_scope["created"]["planning"];
    let saved = save(&mut client, planning, full_draft()).await;
    let reviewed = review(&mut client, &saved).await;
    let work = &reviewed["draft"]["nodes"][0];
    let opened_slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, work, Uuid::new_v4()),
    )
    .await;
    let slice = &opened_slice["created"];

    let whole = route_error(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "delivery_mode":"whole","qualification_reason":"Invalid Full whole-mode probe."}),
    )
    .await;
    assert_eq!(whole["error"]["code"], "invalid_arguments");

    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "qualification_reason":"Full phasewise delivery is required for this fixture."}),
    )
    .await;
    let mut context = begun["created"].clone();
    assert!(!id(&context["run"]["id"]).is_nil());
    assert_eq!(context["run"]["delivery_mode"], "phasewise");
    assert_eq!(
        context["run"]["definition_digest"],
        "1274c531dfd433bf01e6b2354adcd0082c906749e1c8e34a158604f77e77a9a5"
    );
    assert_eq!(context["definition"]["phases"].as_array().unwrap().len(), 1);
    assert_eq!(context["delivered_phases"].as_array().unwrap().len(), 1);
    assert_eq!(context["outputs_complete"], true);
    assert!(
        context["definition"]["phases"][0]["instructions"][0]["body"]
            .as_str()
            .unwrap()
            .len()
            > 100
    );

    context = initial_rework::run(&mut client, &pool, context).await;
    let (
        amendment,
        persisted_session_id,
        definition_digest_before_amendment,
        phase_five_binding,
        phase_five_output,
    ) = source_amendment::run(&mut client, &pool, context).await;
    cold_recovery::run(
        client,
        daemon,
        &pool,
        &runtime,
        socket,
        &config,
        &native,
        &key,
        amendment,
        persisted_session_id,
        definition_digest_before_amendment,
        phase_five_binding,
        phase_five_output,
    )
    .await;
}
