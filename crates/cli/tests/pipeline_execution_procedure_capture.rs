#[path = "pipeline_execution/full_support.rs"]
mod pipeline_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use pipeline_support::{completion, successful_route};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

fn capture_draft(local: &str) -> Value {
    json!({"coverage_summary":"Source-backed procedure candidate with no automatic durable effect","nodes":[{
        "kind":"work","identity":{"local":local},"title":"Capture a reusable procedure candidate",
        "outcome":"A scrubbed, validated proposal or existing-match update is handed to its owner",
        "includes":["source event","match search","normalization","secret scrub","proposal"],
        "excludes":["skill activation","automatic promotion","procedure execution"],"dependencies":[],
        "proof":["Source, match, scrub, validation and proposal receipts"],
        "pipeline":"slice.custom-procedure-capture",
        "pipeline_reason":"A completed source event may contain a reusable procedure worth explicit review",
        "source_result_ids":[]
    }],"supersessions":[]})
}

async fn advance(client: &mut Mcp, context: Value) -> Value {
    let (verdict, outcome, transition) = successful_route(&context);
    let route = (
        verdict.to_owned(),
        outcome.to_owned(),
        transition.to_owned(),
    );
    complete(client, context, &route.0, &route.1, &route.2).await
}

async fn complete(
    client: &mut Mcp,
    context: Value,
    verdict: &str,
    outcome: &str,
    transition: &str,
) -> Value {
    route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&context, verdict, outcome, transition, None, None),
    )
    .await["context"]
        .clone()
}

async fn begin(client: &mut Mcp, repo: &std::path::Path, local: &str, mode: &str) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope = route(
        client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(client, &scope["created"]["planning"], capture_draft(local)).await;
    let reviewed = review(client, &saved).await;
    let opened = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = &opened["created"];
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],"delivery_mode":mode,
            "qualification_reason":"Capture is source-bound and proposal-only; discovery may deepen phasewise."}),
    )
    .await["created"]
        .clone()
}

fn terminal_result(reference: &str) -> Value {
    json!({
        "summary":"The source-backed procedure candidate was scrubbed and proposed without activation.",
        "evidence":[{"kind":"integration_test","reference":reference,
            "observation":"Source, match, secret-safety, validation and proposal receipts completed."}],
        "scope_impact":"The named durable owner may review the proposal or existing-match update.",
        "remaining_work":"Activation, promotion and procedure execution remain separate owner actions."
    })
}

fn handoff_result(gate: &str, verdict: &str) -> Value {
    json!({
        "summary":format!("Procedure capture stopped at {gate} with {verdict}; no downstream capture or promotion ran."),
        "evidence":[{"kind":"integration_test","reference":"pipeline_execution_procedure_capture.rs",
            "observation":format!("The {gate} receipt and {verdict} disposition are preserved for the external owner.")}],
        "scope_impact":"The managed run remains at the source gate for an explicit owner decision or handoff.",
        "remaining_work":"An external owner must resolve the reported gate before any fresh capture may proceed."
    })
}

async fn stop_with_result(client: &mut Mcp, context: Value, verdict: &str) -> Value {
    let phase_id = context["run"]["current_phase_id"].as_str().unwrap();
    let mut request = completion(
        &context,
        verdict,
        "blocked",
        "block",
        None,
        Some(handoff_result(phase_id, verdict)),
    );
    request["publish_blocked_result"] = json!(true);
    route(client, "command", "slice.pipeline.phase.complete", request).await
}

async fn finish(client: &mut Mcp, context: Value) -> Value {
    route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "handoff_not_required",
            "completed",
            "complete",
            None,
            Some(terminal_result("pipeline_execution_procedure_capture.rs")),
        ),
    )
    .await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn procedure_capture_completes_no_match_and_stops_at_reuse_gates() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let discovery_repo = root.join("discovery-source");
    let match_repo = root.join("match-source");
    repository(&discovery_repo);
    repository(&match_repo);
    let socket = root.join("pipeline-procedure.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-procedure-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-procedure-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;

    let mut discovery = begin(&mut client, &discovery_repo, "discovery", "whole").await;
    assert!(!id(&discovery["run"]["id"]).is_nil());
    assert_eq!(discovery["run"]["delivery_mode"], "whole");
    assert_eq!(
        discovery["run"]["definition_digest"],
        "30cbe0da5f26c8703341f4203c270ef158babf0672d07bbbbd637a06dbcdb1bc"
    );
    assert_eq!(
        discovery["definition"]["phases"].as_array().unwrap().len(),
        17
    );
    assert!(
        discovery["definition"]["phases"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|phase| phase["skills"].as_array().into_iter().flatten())
            .all(
                |skill| skill["body"].as_str().is_some_and(|body| body.len() > 100)
                    && skill["digest"]
                        .as_str()
                        .is_some_and(|digest| !digest.is_empty())
            )
    );
    discovery = advance(&mut client, discovery).await;
    discovery = route(
        &mut client,
        "command",
        "slice.pipeline.delivery.escalate",
        json!({"request_id":Uuid::new_v4(),"run_id":discovery["run"]["id"],
            "run_revision":discovery["run"]["revision"],"phase_id":discovery["run"]["current_phase_id"],
            "reason":"The no-match discovery path requires phase-local source and scrub evidence."}),
    )
    .await["context"]
        .clone();
    while discovery["run"]["current_phase_ordinal"].as_u64().unwrap() < 6 {
        discovery = advance(&mut client, discovery).await;
    }
    discovery = complete(&mut client, discovery, "no_match", "completed", "continue").await;
    while discovery["run"]["current_phase_ordinal"].as_u64().unwrap() < 10 {
        discovery = advance(&mut client, discovery).await;
    }
    let mut leaking = completion(
        &discovery,
        "safe_to_continue",
        "completed",
        "continue",
        None,
        None,
    );
    leaking["output"]["fields"]["leak_count"] = json!("1");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            leaking
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    while discovery["run"]["current_phase_ordinal"].as_u64().unwrap() < 15 {
        discovery = advance(&mut client, discovery).await;
    }
    let waiting = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &discovery,
            "decision_waiting",
            "waiting_input",
            "continue",
            None,
            None,
        ),
    )
    .await;
    discovery = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":waiting["context"]["run"]["id"],
            "run_revision":waiting["context"]["run"]["revision"],
            "phase_id":waiting["context"]["run"]["current_phase_id"],
            "input":"The durable owner accepts a proposal for review without promotion."}),
    )
    .await["context"]
        .clone();
    discovery = complete(
        &mut client,
        discovery,
        "eligible_for_promotion_review",
        "completed",
        "continue",
    )
    .await;
    discovery = complete(
        &mut client,
        discovery,
        "procedure_proposed",
        "completed",
        "continue",
    )
    .await;
    let discovery_done = finish(&mut client, discovery).await;
    assert_eq!(discovery_done["context"]["run"]["status"], "completed");
    let discovery_outputs = discovery_done["context"]["outputs"].as_array().unwrap();
    assert!(discovery_outputs.iter().all(|output| {
        output["fields"]["durable_write_performed"].as_str() != Some("true")
            && output["fields"]["skill_activation_performed_by_this_slice"].as_str() != Some("true")
            && output["fields"]["procedure_execution_performed_by_this_slice"].as_str()
                != Some("true")
    }));

    let mut matched = begin(&mut client, &match_repo, "existing-match", "phasewise").await;
    while matched["run"]["current_phase_ordinal"].as_u64().unwrap() < 6 {
        matched = advance(&mut client, matched).await;
    }
    let stopped = stop_with_result(&mut client, matched, "exact_match").await;
    assert_eq!(stopped["context"]["run"]["status"], "blocked");
    assert_eq!(stopped["context"]["run"]["current_phase_ordinal"], 6);
    assert_eq!(
        stopped["result"]["pipeline_result_origin"],
        "managed_blocked"
    );
    assert_eq!(stopped["result"]["provenance"], "externally_reported");
    assert_eq!(
        stopped["result"]["pipeline_run_id"],
        stopped["context"]["run"]["id"]
    );
    assert!(
        stopped["context"]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|output| {
                output["phase_ordinal"].as_u64().unwrap() <= 6
                    && output["fields"]["classification"] != "no_match"
            })
    );
    let resumed = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":stopped["context"]["run"]["id"],
            "run_revision":stopped["context"]["run"]["revision"],
            "phase_id":stopped["context"]["run"]["current_phase_id"],
            "input":"External-owner handoff evidence is attached; no source gate is overridden."}),
    )
    .await;
    assert_eq!(resumed["context"]["run"]["status"], "active");
    assert_eq!(resumed["context"]["run"]["current_phase_ordinal"], 6);
    assert_eq!(resumed["context"]["result"]["id"], stopped["result"]["id"]);

    let mut rejected = begin(&mut client, &match_repo, "negative-reuse", "phasewise").await;
    while rejected["run"]["current_phase_ordinal"].as_u64().unwrap() < 11 {
        rejected = advance(&mut client, rejected).await;
    }
    let rejected = stop_with_result(&mut client, rejected, "unsafe").await;
    assert_eq!(rejected["context"]["run"]["current_phase_ordinal"], 11);
    assert_eq!(rejected["context"]["run"]["status"], "blocked");
    assert_eq!(
        rejected["result"]["pipeline_result_origin"],
        "managed_blocked"
    );

    let mut deferred = begin(&mut client, &match_repo, "validation-deferred", "phasewise").await;
    while deferred["run"]["current_phase_ordinal"].as_u64().unwrap() < 12 {
        deferred = advance(&mut client, deferred).await;
    }
    let waiting = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &deferred,
            "deferred-needs-authority",
            "waiting_input",
            "continue",
            None,
            None,
        ),
    )
    .await;
    assert_eq!(waiting["context"]["run"]["status"], "waiting_input");
    assert_eq!(waiting["context"]["run"]["current_phase_ordinal"], 12);
    assert!(waiting["result"].is_null());
    assert!(
        waiting["context"]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|output| { output["phase_ordinal"].as_u64().unwrap() <= 12 })
    );
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
