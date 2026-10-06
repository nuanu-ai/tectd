#[path = "pipeline_execution_full/cold_recovery.rs"]
mod cold_recovery;
#[path = "pipeline_execution/full_support.rs"]
mod full_support;
#[path = "pipeline_execution_full/initial_rework.rs"]
mod initial_rework;
mod recovery_support;
#[path = "pipeline_execution_full/review_sessions.rs"]
mod review_sessions;
#[path = "pipeline_execution_full/source_amendment.rs"]
mod source_amendment;
#[path = "native_planning/support.rs"]
mod support;

use full_support::{completion, refresh_knowledge, replace_ledger, successful_route};
use recovery_support::native_reads::ScopeOpenFixture;
use recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
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

fn mutation_result_id(context: &ResolvedPipeline) -> &Value {
    context
        .raw_payload
        .get("result_reference")
        .and_then(Value::as_object)
        .expect("actual mutation result reference object")
        .get("result_id")
        .expect("actual mutation result ID key")
}

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

async fn advance(client: &mut Mcp, context: ResolvedPipeline) -> ResolvedPipeline {
    let (verdict, outcome, transition) = successful_route(&context);
    let request = if matches!(
        context.run()["definition_version"].as_str(),
        Some("0.6.0-native.engineering.3" | "0.6.0-native.engineering.4")
    ) && context.run()["current_phase_id"] == "slice-contract-writer"
    {
        let facts = full_support::native_contract_fixture_facts(client).await;
        full_support::completion_with_contract(
            &context,
            completion(&context, verdict, outcome, transition, None, None),
            &facts,
        )
    } else {
        completion(&context, verdict, outcome, transition, None, None)
    };
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
        response["result"]["isError"],
        true,
        "phase={} request={} response={}",
        context.run()["current_phase_id"],
        request,
        response
    );
    let raw = tool_payload(&response);
    let result = resolve_pipeline(client, raw).await.unwrap();
    assert!(mutation_result_id(&result).is_null());
    result
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
    std::fs::write(
        repo.join("native-contract-fixture.txt"),
        b"Private native work contract QA fixture.\n",
    )
    .unwrap();
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
    let opened_scope = ScopeOpenFixture::from_mutation(opened_scope, "created");
    let planning_read = opened_scope.read_planning(&mut client).await;
    let planning = &planning_read.value;
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
    assert_eq!(whole["error"]["code"], "INPUT_SCHEMA_INVALID");
    for (field, expected) in json!({
        "code":"INPUT_SCHEMA_INVALID", "rule":"WP6-BEGIN-DELIVERY-MODE",
        "path":"arguments.params.delivery_mode",
        "expected":"mode allowed by selected definition", "actual":"Whole",
        "next_action":"correct_input_and_retry", "required":"schema_valid_input",
        "message":"the submitted value does not satisfy the selected input schema"
    })
    .as_object()
    .unwrap()
    {
        assert_eq!(whole["error"]["refusal"][field], *expected);
    }

    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "qualification_reason":"Full phasewise delivery is required for this fixture."}),
    )
    .await;
    let begun = resolve_pipeline(&mut client, raw).await.unwrap();
    let mut context = begun;
    assert!(!id(&context.run()["id"]).is_nil());
    assert_eq!(context.run()["delivery_mode"], "phasewise");
    assert_eq!(
        context.run()["definition_version"],
        "0.6.0-native.engineering.4"
    );
    assert_eq!(
        context.run()["definition_digest"],
        "85ec63bae1903fedb0c86ecd5326380ea8d524fe0ee29c5dce6e90b9a30cdd3d"
    );
    assert_eq!(
        context.details_data()["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        context.details_data()["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(context.details_data()["outputs_complete"], true);
    assert!(
        context.current_phase().unwrap()["instructions"][0]["body"]
            .as_str()
            .unwrap()
            .len()
            > 100
    );

    let mut review_sessions =
        review_sessions::ReviewSessions::new(&mut client, &socket, &config, &key).await;
    context = initial_rework::run(&mut client, &pool, context, &mut review_sessions).await;
    let (
        amendment,
        persisted_session_id,
        definition_digest_before_amendment,
        phase_five_binding,
        phase_five_output,
    ) = source_amendment::run(&mut client, &pool, context).await;
    cold_recovery::run(cold_recovery::ColdRecovery {
        review_sessions,
        client,
        daemon,
        pool: &pool,
        runtime: &runtime,
        socket,
        config: &config,
        native: &native,
        key: &key,
        amendment,
        persisted_session_id,
        definition_digest_before_amendment,
        phase_five_binding,
        phase_five_output,
    })
    .await;
}
