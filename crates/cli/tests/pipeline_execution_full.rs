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
        "INVALID_OUTPUT"
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
        "INVALID_OUTPUT"
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
    let empty_ledger_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        empty_ledger,
    )
    .await;
    assert_eq!(empty_ledger_error["error"]["code"], "invalid_arguments");
    assert_eq!(
        empty_ledger_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        empty_ledger_error["error"]["refusal"]["rule"],
        "WP6-COMPLETE-01"
    );
    assert_eq!(
        empty_ledger_error["error"]["refusal"]["path"],
        "arguments.params"
    );
    assert_eq!(
        empty_ledger_error["error"]["refusal"]["next_action"],
        "correct_output"
    );
    let after_empty_ledger = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"refresh":true}),
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
    let dropped_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        dropped,
    )
    .await;
    assert_eq!(dropped_error["error"]["code"], "invalid_arguments");
    assert_eq!(dropped_error["error"]["refusal"]["code"], "INVALID_OUTPUT");
    assert_eq!(dropped_error["error"]["refusal"]["rule"], "WP6-COMPLETE-01");
    let after_rejection = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"refresh":true}),
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
    let mut blocked_no_revisit = completion(&context, verdict, outcome, transition, None, None);
    blocked_no_revisit["output"]["verdict"] = json!("blocked_unreconciled_findings");
    blocked_no_revisit["output"]["dispositions"] = json!(["blocked_unreconciled_findings"]);
    blocked_no_revisit["outcome"] = json!("blocked");
    blocked_no_revisit["transition"] = json!("block");
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        blocked_no_revisit,
    )
    .await["context"]
        .clone();
    assert_eq!(context["run"]["status"], "blocked");
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-reconciliation-runner"
    );
    assert!(
        context["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|binding| binding["stale"] == false)
    );
    let phase_five_binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-component-decision-interrogator")
        .unwrap();
    let mut phase_five_artifacts: Value =
        sqlx::query_scalar("SELECT artifacts FROM slice_pipeline_phase_outputs WHERE id=$1")
            .bind(id(&phase_five_binding["output_id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    let legacy_ledger = phase_five_artifacts
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    legacy_ledger["body"] = json!("{}");
    legacy_ledger["digest"] =
        json!("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
    sqlx::query("UPDATE slice_pipeline_phase_outputs SET artifacts=$2 WHERE id=$1")
        .bind(id(&phase_five_binding["output_id"]))
        .bind(phase_five_artifacts)
        .execute(&pool)
        .await
        .unwrap();
    context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;

    let (verdict, outcome, transition) = successful_route(&context);
    let forward_with_legacy_phase_five =
        completion(&context, verdict, outcome, transition, None, None);
    let legacy_forward_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        forward_with_legacy_phase_five,
    )
    .await;
    assert_eq!(legacy_forward_error["error"]["code"], "invalid_arguments");
    assert_eq!(
        legacy_forward_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        legacy_forward_error["error"]["refusal"]["rule"],
        "WP6-COMPLETE-01"
    );
    assert_eq!(
        legacy_forward_error["error"]["refusal"]["next_action"],
        "correct_output"
    );
    let after_legacy_forward_rejection = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    for collection in ["run", "attempts", "outputs", "bindings"] {
        assert_eq!(
            after_legacy_forward_rejection[collection], context[collection],
            "{collection} persisted after rejected legacy lineage"
        );
    }

    let recovery_verdict = "blocked_unreconciled_findings";
    let mut invalid_recovery = completion(&context, verdict, outcome, transition, None, None);
    invalid_recovery["output"]["verdict"] = json!(recovery_verdict);
    invalid_recovery["output"]["dispositions"] = json!([recovery_verdict]);
    invalid_recovery["revisit_phase_id"] = json!("slice-component-decision-interrogator");
    let invalid_recovery_ledger = invalid_recovery["output"]["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    invalid_recovery_ledger["body"] = json!("{}");
    invalid_recovery_ledger["digest"] =
        json!("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
    invalid_recovery["output"]["validator_receipts"][0]["artifacts"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap()["digest"] =
        json!("44136fa355b3678a1146ad16f7e8649e94fb4fc21fe77e8310c060f61caaff8a");
    let invalid_recovery_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        invalid_recovery,
    )
    .await;
    assert_eq!(invalid_recovery_error["error"]["code"], "invalid_arguments");
    assert_eq!(
        invalid_recovery_error["error"]["refusal"]["code"], "INVALID_OUTPUT",
        "{invalid_recovery_error}"
    );
    assert_eq!(
        invalid_recovery_error["error"]["refusal"]["rule"],
        "WP6-COMPLETE-01"
    );

    let mut recovery_request = completion(&context, verdict, outcome, transition, None, None);
    recovery_request["output"]["verdict"] = json!(recovery_verdict);
    recovery_request["output"]["dispositions"] = json!([recovery_verdict]);
    recovery_request["revisit_phase_id"] = json!("slice-component-decision-interrogator");
    let recovery = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        recovery_request,
    )
    .await;
    context = recovery["context"].clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-component-decision-interrogator"
    );
    for binding in context["bindings"].as_array().unwrap() {
        let ordinal = binding["phase_ordinal"].as_u64().unwrap();
        if ordinal >= 5 {
            assert_eq!(binding["stale"], true);
            assert_eq!(
                binding["stale_reason"],
                "rework_from:slice-component-decision-interrogator"
            );
        }
    }
    context = advance(&mut client, context).await;
    context = advance(&mut client, context).await;
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-reconciliation-runner"
    );
    context = advance(&mut client, context).await;
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-implementation-spec-synthesizer"
    );

    let phase_five_binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-component-decision-interrogator")
        .unwrap()
        .clone();
    let phase_five_output = context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["id"] == phase_five_binding["output_id"])
        .unwrap()
        .clone();
    let phase_five_artifact = phase_five_output["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    let phase_five_ledger: Value =
        serde_json::from_str(phase_five_artifact["body"].as_str().unwrap()).unwrap();
    let definition_digest_before_amendment = context["run"]["definition_digest"].clone();
    let successor_body = "# Amended design source\n\nThe direct operator instruction authorizes this bounded amendment.";
    let successor_digest = format!("{:x}", Sha256::digest(successor_body.as_bytes()));
    let authority_text = "Direct operator instruction: amend the Full Design source and rerun phase 5 through reconciliation.";
    let amendment_request_id = Uuid::new_v4();
    let amendment = json!({
        "request_id":amendment_request_id,
        "run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],
        "input":authority_text,
        "source_amendment":{
            "target_phase_id":"slice-component-decision-interrogator",
            "predecessor":{
                "output_id":phase_five_binding["output_id"],
                "output_revision":phase_five_binding["output_revision"],
                "output_digest":phase_five_binding["output_digest"],
                "artifact_name":"requirements-ledger.json",
                "artifact_digest":phase_five_artifact["digest"],
                "source_path":phase_five_ledger["source"]["path"],
                "source_digest":phase_five_ledger["source"]["digest"]
            },
            "successor":{
                "path":"source-spec.md",
                "artifact":{
                    "name":"source-spec.md","media_type":"text/markdown",
                    "body":successor_body,"digest":successor_digest
                }
            },
            "authorization_scope":"amend the current Full Design source and rerun phase 5 dependency closure",
            "authorization_provenance":"exact direct operator instruction persisted in input"
        }
    });

    let before_rejections = context.clone();
    for (field, wrong) in [
        ("output_id", json!(Uuid::new_v4())),
        (
            "output_revision",
            json!(phase_five_binding["output_revision"].as_i64().unwrap() + 1),
        ),
        ("output_digest", json!("0".repeat(64))),
        ("artifact_digest", json!("1".repeat(64))),
        ("source_path", json!("other-source.md")),
        ("source_digest", json!("sha256:other-source")),
    ] {
        let mut wrong_predecessor = amendment.clone();
        wrong_predecessor["request_id"] = json!(Uuid::new_v4());
        wrong_predecessor["source_amendment"]["predecessor"][field] = wrong;
        let error = route_error(
            &mut client,
            "command",
            "slice.pipeline.input",
            wrong_predecessor,
        )
        .await;
        assert_eq!(
            error["error"]["refusal"]["code"], "INVALID_OUTPUT",
            "{field}: {error}"
        );
        assert_eq!(error["error"]["refusal"]["rule"], "WP6-INPUT-01");
    }
    let mut wrong_artifact = amendment.clone();
    wrong_artifact["request_id"] = json!(Uuid::new_v4());
    wrong_artifact["source_amendment"]["predecessor"]["artifact_name"] =
        json!("decision-traceability.json");
    let wrong_artifact_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        wrong_artifact,
    )
    .await;
    assert_eq!(
        wrong_artifact_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        wrong_artifact_error["error"]["refusal"]["rule"],
        "WP6-INPUT-01"
    );

    let mut out_of_scope = amendment.clone();
    out_of_scope["request_id"] = json!(Uuid::new_v4());
    out_of_scope["source_amendment"]["target_phase_id"] = json!("slice-design-spec-shaper");
    let out_of_scope_error =
        route_error(&mut client, "command", "slice.pipeline.input", out_of_scope).await;
    assert_eq!(
        out_of_scope_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        out_of_scope_error["error"]["refusal"]["rule"],
        "WP6-INPUT-01"
    );

    let mut digest_mismatch = amendment.clone();
    digest_mismatch["request_id"] = json!(Uuid::new_v4());
    digest_mismatch["source_amendment"]["successor"]["artifact"]["digest"] = json!("0".repeat(64));
    let digest_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        digest_mismatch,
    )
    .await;
    assert_eq!(digest_error["error"]["code"], "invalid_arguments");

    let mut missing_authority = amendment.clone();
    missing_authority["request_id"] = json!(Uuid::new_v4());
    missing_authority["source_amendment"]["authorization_scope"] = json!("");
    let authority_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        missing_authority,
    )
    .await;
    assert_eq!(authority_error["error"]["code"], "invalid_arguments");
    let mut path_name_mismatch = amendment.clone();
    path_name_mismatch["request_id"] = json!(Uuid::new_v4());
    path_name_mismatch["source_amendment"]["successor"]["artifact"]["name"] =
        json!("other-source.md");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.input",
            path_name_mismatch
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let after_rejections = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    for collection in ["run", "inputs", "bindings"] {
        assert_eq!(after_rejections[collection], before_rejections[collection]);
    }
    assert_eq!(
        after_rejections["run"]["definition_digest"],
        definition_digest_before_amendment
    );

    let amended = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        amendment.clone(),
    )
    .await;
    let replay = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        amendment.clone(),
    )
    .await;
    assert_eq!(replay, amended);
    context = amended["context"].clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-component-decision-interrogator"
    );
    assert_eq!(
        context["inputs"].as_array().unwrap().last().unwrap()["input"],
        authority_text
    );
    assert_eq!(
        context["inputs"].as_array().unwrap().last().unwrap()["phase_id"],
        "slice-component-decision-interrogator"
    );
    let persisted: (String, Uuid, Value) = sqlx::query_as(
        "SELECT input,actor_session_id,request_payload FROM slice_pipeline_inputs WHERE request_id=$1",
    )
    .bind(amendment_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted.0, authority_text);
    assert!(!persisted.1.is_nil());
    assert_eq!(persisted.2["input"], amendment["input"]);
    assert_eq!(
        persisted.2["source_amendment"]["authorization_scope"],
        amendment["source_amendment"]["authorization_scope"]
    );
    assert_eq!(
        persisted.2["source_amendment"]["authorization_provenance"],
        amendment["source_amendment"]["authorization_provenance"]
    );

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    client.call("open_workspace", json!({})).await;
    context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":amendment["run_id"]}),
    )
    .await;
    let cold_input = context["inputs"].as_array().unwrap().last().unwrap();
    let cold_amendment = &cold_input["source_amendment"];
    assert_eq!(cold_input["input"], authority_text);
    assert_eq!(
        cold_amendment["authorization_provenance"],
        "exact direct operator instruction persisted in input"
    );
    assert_eq!(
        cold_amendment["successor"]["artifact"]["body"],
        successor_body
    );
    let successor_path = cold_amendment["successor"]["path"]
        .as_str()
        .unwrap()
        .to_owned();
    let successor_digest = cold_amendment["successor"]["artifact"]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    let successor_artifact = cold_amendment["successor"]["artifact"].clone();
    assert_eq!(
        context["run"]["definition_digest"],
        definition_digest_before_amendment
    );
    for binding in context["bindings"].as_array().unwrap() {
        if binding["phase_ordinal"].as_u64().unwrap() >= 5 {
            assert_eq!(binding["stale"], true);
            assert_eq!(binding["stale_reason"], "source_amendment");
        }
    }
    let old_phase_five = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"view":"output",
            "output_id":phase_five_binding["output_id"],"digest":phase_five_binding["output_digest"]}),
    )
    .await;
    assert_eq!(old_phase_five["stale"], true);
    for field in [
        "id",
        "run_id",
        "phase_id",
        "phase_ordinal",
        "revision",
        "body",
        "producer_context_id",
        "digest",
        "reference",
        "fields",
        "verdict",
        "dispositions",
        "skill_reads",
        "resource_reads",
        "artifacts",
        "validator_receipts",
        "followup_proposal",
    ] {
        assert_eq!(old_phase_five[field], phase_five_output[field], "{field}");
    }

    let mut conflict = amendment.clone();
    conflict["input"] = json!("Conflicting replay text");
    assert_eq!(
        route_error(&mut client, "command", "slice.pipeline.input", conflict).await["error"]["code"],
        "input_conflict"
    );
    let mut stale_predecessor = amendment.clone();
    stale_predecessor["request_id"] = json!(Uuid::new_v4());
    stale_predecessor["run_revision"] = context["run"]["revision"].clone();
    stale_predecessor["phase_id"] = context["run"]["current_phase_id"].clone();
    let stale_predecessor_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.input",
        stale_predecessor,
    )
    .await;
    assert_eq!(
        stale_predecessor_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        stale_predecessor_error["error"]["refusal"]["rule"],
        "WP6-INPUT-01"
    );

    let (verdict, outcome, transition) = successful_route(&context);
    let wrong_lineage_phase_five = completion(&context, verdict, outcome, transition, None, None);
    let wrong_lineage_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        wrong_lineage_phase_five,
    )
    .await;
    assert_eq!(
        wrong_lineage_error["error"]["refusal"]["code"],
        "INVALID_OUTPUT"
    );
    assert_eq!(
        wrong_lineage_error["error"]["refusal"]["rule"],
        "WP6-COMPLETE-01"
    );
    let after_wrong_lineage = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"refresh":true}),
    )
    .await;
    for collection in ["run", "attempts", "outputs", "bindings", "inputs"] {
        assert_eq!(after_wrong_lineage[collection], context[collection]);
    }
    context = after_wrong_lineage;
    context = refresh_knowledge(&mut client, &context).await;
    let (verdict, outcome, transition) = successful_route(&context);

    let mut amended_phase_five = completion(&context, verdict, outcome, transition, None, None);
    replace_ledger_source(
        &mut amended_phase_five["output"],
        &successor_path,
        &successor_digest,
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        amended_phase_five,
    )
    .await["context"]
        .clone();
    context = advance(&mut client, context).await;
    let (verdict, outcome, transition) = successful_route(&context);
    let mut amended_phase_seven = completion(&context, verdict, outcome, transition, None, None);
    replace_ledger_source(
        &mut amended_phase_seven["output"],
        &successor_path,
        &successor_digest,
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        amended_phase_seven,
    )
    .await["context"]
        .clone();
    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-implementation-spec-synthesizer"
    );
    let fresh_phase_five = context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| {
            output["phase_id"] == "slice-component-decision-interrogator"
                && output["stale"] == false
        })
        .unwrap();
    let fresh_ledger: Value = serde_json::from_str(
        fresh_phase_five["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .find(|artifact| artifact["name"] == "requirements-ledger.json")
            .unwrap()["body"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(fresh_ledger["source"]["path"], successor_path);
    assert_eq!(fresh_ledger["source"]["digest"], successor_digest);

    let fresh_phase_five_binding = context["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-component-decision-interrogator")
        .unwrap();
    let fresh_phase_five_artifact = fresh_phase_five["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|artifact| artifact["name"] == "requirements-ledger.json")
        .unwrap();
    let no_op = json!({
        "request_id":Uuid::new_v4(),
        "run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],
        "input":"Direct operator instruction for a no-op amendment rejection check.",
        "source_amendment":{
            "target_phase_id":"slice-component-decision-interrogator",
            "predecessor":{
                "output_id":fresh_phase_five_binding["output_id"],
                "output_revision":fresh_phase_five_binding["output_revision"],
                "output_digest":fresh_phase_five_binding["output_digest"],
                "artifact_name":"requirements-ledger.json",
                "artifact_digest":fresh_phase_five_artifact["digest"],
                "source_path":fresh_ledger["source"]["path"],
                "source_digest":fresh_ledger["source"]["digest"]
            },
            "successor":{"path":successor_path,"artifact":successor_artifact},
            "authorization_scope":"amend the current Full Design source",
            "authorization_provenance":"exact direct operator input"
        }
    });
    let before_no_op = context.clone();
    let no_op_error = route_error(&mut client, "command", "slice.pipeline.input", no_op).await;
    assert_eq!(no_op_error["error"]["refusal"]["code"], "INVALID_OUTPUT");
    assert_eq!(no_op_error["error"]["refusal"]["rule"], "WP6-INPUT-01");
    let after_no_op = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"],"refresh":true}),
    )
    .await;
    for collection in ["run", "inputs", "bindings"] {
        assert_eq!(after_no_op[collection], before_no_op[collection]);
    }
    assert_eq!(
        after_no_op["run"]["definition_digest"],
        definition_digest_before_amendment
    );

    let synthesis = advance(&mut client, after_no_op).await;
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
        "1274c531dfd433bf01e6b2354adcd0082c906749e1c8e34a158604f77e77a9a5"
    );
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        32
    );
    assert_eq!(
        completed["context"]["run"]["definition_digest"],
        definition_digest_before_amendment
    );
    admin::revoke_session(&pool, persisted.1).await.unwrap();
    assert_eq!(
        route_error(&mut client, "command", "slice.pipeline.input", amendment).await["error"]["code"],
        "session_revoked"
    );
}
