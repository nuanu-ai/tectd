#[path = "pipeline_execution/full_support.rs"]
mod full_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use full_support::{completion, refresh_knowledge, replace_ledger, successful_route};
use recovery_support::{
    Daemon, Mcp, host_file, private_temp, public_call, tagged_url, tool_payload,
};
use serde_json::{Value, json};
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
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
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
        "09f4c903a417537c6059cbccd73b9faafe8f91ee536e43034a883a81d818d7fd"
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

    context = advance(&mut client, context).await;
    let (phase_two_verdict, phase_two_outcome, phase_two_transition) = successful_route(&context);
    let mut wrong_digest = completion(
        &context,
        phase_two_verdict,
        phase_two_outcome,
        phase_two_transition,
        None,
        None,
    );
    wrong_digest["output"]["artifacts"][0]["digest"] = json!("wrong");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            wrong_digest
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = advance(&mut client, context).await;
    context = advance(&mut client, context).await;
    let (phase_four_verdict, phase_four_outcome, phase_four_transition) =
        successful_route(&context);
    let mut malformed_json = completion(
        &context,
        phase_four_verdict,
        phase_four_outcome,
        phase_four_transition,
        None,
        None,
    );
    malformed_json["output"]["artifacts"][0]["body"] = json!("not json");
    malformed_json["output"]["artifacts"][0]["digest"] =
        json!("7ccfa1fb147ea0cb851480c39f28c0f78a2b035aeed0d2cf5e4c13d0d2adca4d");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            malformed_json
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = advance(&mut client, context).await;
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-component-decision-interrogator"
    );
    let mut empty_ledger = completion(
        &context,
        "blocked_unresolved_questions",
        "completed",
        "continue",
        Some("slice-design-spec-shaper"),
        None,
    );
    let ledger = empty_ledger["output"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    ledger["body"] = json!("{}");
    ledger["digest"] = json!("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            empty_ledger
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let after_empty_ledger = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(after_empty_ledger["run"], context["run"]);
    for collection in ["attempts", "outputs", "bindings"] {
        assert_eq!(after_empty_ledger[collection], context[collection]);
    }
    let old_target = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-design-spec-shaper")
        .unwrap()
        .clone();
    let mut wrong_rework = completion(
        &context,
        "blocked_unresolved_questions",
        "completed",
        "continue",
        Some("slice-design-spec-shaper"),
        None,
    );
    wrong_rework["request_id"] = json!(Uuid::new_v4());
    wrong_rework["revisit_phase_id"] = json!("slice-full-dev-entry-gate");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            wrong_rework
        )
        .await["error"]["code"],
        "invalid_arguments"
    );

    let valid_phase_five = completion(
        &context,
        "blocked_unresolved_questions",
        "completed",
        "continue",
        Some("slice-design-spec-shaper"),
        None,
    );
    let ledger = valid_phase_five["output"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    assert_eq!(
        serde_json::from_str::<Value>(ledger["body"].as_str().unwrap()).unwrap()["requirements"]
            .as_array()
            .unwrap()
            .len(),
        20
    );
    let reworked = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        valid_phase_five,
    )
    .await;
    context = reworked["context"].clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-design-spec-shaper"
    );
    for binding in context["bindings"].as_array().unwrap() {
        let ordinal = binding["phase_ordinal"].as_u64().unwrap();
        assert_eq!(binding["stale"], ordinal >= 3);
    }
    let stale_target = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"view":"output",
            "output_id":old_target["output_id"],"digest":old_target["output_digest"]}),
    )
    .await;
    assert_eq!(stale_target["stale"], true);
    assert_eq!(
        stale_target["stale_reason"],
        "rework_from:slice-design-spec-shaper"
    );

    for _ in 0..3 {
        context = advance(&mut client, context).await;
    }
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-cross-cutting-reviewer"
    );
    let review_request = {
        let (verdict, outcome, transition) = successful_route(&context);
        completion(&context, verdict, outcome, transition, None, None)
    };
    let attestation = &review_request["output"]["reviewer_context"];
    assert_eq!(
        attestation["reviewer_context_id"],
        review_request["output"]["producer_context_id"]
    );
    assert_eq!(attestation["fresh_input"], true);
    assert_eq!(
        attestation["producer_context_ids"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        review_request,
    )
    .await["context"]
        .clone();

    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-reconciliation-runner"
    );
    let (verdict, outcome, transition) = successful_route(&context);
    assert_eq!(verdict, "not_required");
    let mut dropped = completion(&context, verdict, outcome, transition, None, None);
    replace_ledger(&mut dropped["output"], 5);
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            dropped
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let after_rejection = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(
        after_rejection["run"]["revision"],
        context["run"]["revision"]
    );
    assert_eq!(
        after_rejection["run"]["current_phase_id"],
        "slice-reconciliation-runner"
    );
    for collection in ["attempts", "outputs", "bindings"] {
        assert_eq!(
            after_rejection[collection], context[collection],
            "{collection} persisted"
        );
    }
    context = advance(&mut client, context).await;
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-implementation-spec-synthesizer"
    );
    let synthesis = advance(&mut client, context).await;
    let synthesis_output = synthesis["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["phase_id"] == "slice-implementation-spec-synthesizer")
        .unwrap()
        .clone();
    assert_eq!(synthesis_output["artifacts"].as_array().unwrap().len(), 6);
    assert_eq!(
        synthesis_output["validator_receipts"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    context = synthesis;

    let run_id = context["run"]["id"].clone();
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    let _restarted_daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    client.call("open_workspace", json!({})).await;
    context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(context["outputs_complete"], true);
    let binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-implementation-spec-synthesizer")
        .unwrap();
    let exact = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id,"view":"output","output_id":binding["output_id"],
            "digest":binding["output_digest"]}),
    )
    .await;
    assert_eq!(exact["artifacts"], synthesis_output["artifacts"]);
    assert_eq!(
        exact["validator_receipts"],
        synthesis_output["validator_receipts"]
    );
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":run_id,"view":"output","output_id":binding["output_id"],
            "digest":"wrong"})
        )
        .await["error"]["code"],
        "not_found"
    );

    while context["run"]["current_phase_id"] != "slice-human-decision-queue-manager" {
        context = advance(&mut client, context).await;
    }
    let waiting = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "blocked_missing_authority",
            "waiting_input",
            "continue",
            None,
            None,
        ),
    )
    .await;
    context = waiting["context"].clone();
    assert_eq!(context["run"]["status"], "waiting_input");
    context = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
            "input":"Recorded authority and bounded resume evidence."}),
    )
    .await["context"]
        .clone();
    context = refresh_knowledge(&mut client, &context).await;
    context = advance(&mut client, context).await;

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 21 {
        context = advance(&mut client, context).await;
    }
    let (verdict, outcome, transition) = successful_route(&context);
    let completed = route(&mut client,"command","slice.pipeline.phase.complete",
        completion(&context,verdict,outcome,transition,None,Some(json!({
            "summary":"Caller reports Full Slice completion after exact phase contracts.",
            "evidence":[{"kind":"integration_test","reference":"pipeline_execution_full.rs",
                "observation":"Twenty-one phases, rework, review, validators and cold retrieval completed."}],
            "scope_impact":"Refresh future planning once.","remaining_work":"No remaining work in this Slice."
        })))).await;
    assert_eq!(completed["context"]["run"]["status"], "completed");
    assert_eq!(
        completed["result"]["pipeline_definition_digest"],
        "09f4c903a417537c6059cbccd73b9faafe8f91ee536e43034a883a81d818d7fd"
    );
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        25
    );
}
