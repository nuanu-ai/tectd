#[path = "pipeline_execution/full_support.rs"]
mod pipeline_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use pipeline_support::{
    add_opaque_authority_labels, assert_forged_implementation_phase_rejected,
    assert_non_coding_definition, completion, refresh_knowledge, successful_route,
};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

fn preparation_draft() -> Value {
    json!({"coverage_summary":"Operational preparation without execution","nodes":[{
        "kind":"work","identity":{"local":"preparation"},"title":"Prepare the bounded operation",
        "outcome":"Target, authority, preflight, rollback, proof and handoff are ready without effects",
        "includes":["target baseline","authority","risk","preflight","rollback","dry run","handoff"],
        "excludes":["operation execution","mutating command"],"dependencies":[],
        "proof":["Read-only validation and prepared-not-executed receipt"],
        "pipeline":"slice.operational-preparation",
        "pipeline_reason":"The operation requires a complete safe package before separate execution",
        "source_result_ids":[]
    }],"supersessions":[]})
}

async fn advance(client: &mut Mcp, context: Value) -> Value {
    let (verdict, outcome, transition) = successful_route(&context);
    route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&context, verdict, outcome, transition, None, None),
    )
    .await["context"]
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn operational_preparation_builds_safe_handoff_without_executing() {
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
    let socket = root.join("pipeline-preparation.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-preparation-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-preparation-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let scope=route(&mut client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(
        &mut client,
        &scope["created"]["planning"],
        preparation_draft(),
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let opened = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = &opened["created"];
    let begun=route(&mut client,"command","slice.pipeline.begin",json!({"request_id":Uuid::new_v4(),
        "scope_id":reviewed["scope"]["id"],"slice_id":slice["id"],"slice_revision":slice["revision"],
        "qualification_reason":"Begin whole and deepen phasewise for the complex operational package."})).await;
    let mut context = begun["created"].clone();
    assert!(!id(&context["run"]["id"]).is_nil());
    assert_eq!(context["run"]["delivery_mode"], "whole");
    assert_eq!(
        context["run"]["definition_digest"],
        "db83e347ee7970d2122dc999ec6cefd8e2e88ac9e6a944e3be3fc1554cbc414a"
    );
    assert_eq!(
        context["definition"]["phases"].as_array().unwrap().len(),
        16
    );
    assert!(
        context["definition"]["phases"]
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

    let (verdict, outcome, transition) = successful_route(&context);
    let mut forbidden_effect = completion(&context, verdict, outcome, transition, None, None);
    forbidden_effect["output"]["fields"]["operation_executed"] = json!("true");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            forbidden_effect
        )
        .await["error"]["code"],
        "INVALID_OUTPUT"
    );
    assert_non_coding_definition(&context, "slice.operational-preparation");
    let mut first = completion(&context, verdict, outcome, transition, None, None);
    assert_forged_implementation_phase_rejected(
        &mut client,
        &context,
        first.clone(),
        "slice.operational-preparation",
    )
    .await;
    add_opaque_authority_labels(&mut first);
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        first,
    )
    .await["context"]
        .clone();
    assert_non_coding_definition(&context, "slice.operational-preparation");
    context=route(&mut client,"command","slice.pipeline.delivery.escalate",json!({
        "request_id":Uuid::new_v4(),"run_id":context["run"]["id"],"run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],
        "reason":"The remaining authority, rollback and proof contracts warrant phase-local delivery."})).await["context"].clone();
    assert_eq!(context["run"]["delivery_mode"], "phasewise");
    context = refresh_knowledge(&mut client, &context).await;

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 16 {
        context = advance(&mut client, context).await;
    }
    let (verdict, outcome, transition) = successful_route(&context);
    let completed=route(&mut client,"command","slice.pipeline.phase.complete",
        completion(&context,verdict,outcome,transition,None,Some(json!({
            "summary":"The operational package is prepared without executing the operation.",
            "evidence":[{"kind":"integration_test","reference":"pipeline_execution_operational_preparation.rs",
                "observation":"Target, authority, risk, preflight, rollback, proof, dry-run and handoff receipts completed."}],
            "scope_impact":"A separately authorized Operational Execution Slice may consume the package.",
            "remaining_work":"Execute only through the separately reviewed operational pipeline."
        })))).await;
    assert_eq!(completed["context"]["run"]["status"], "completed");
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        16
    );
    let outputs = completed["context"]["outputs"].as_array().unwrap();
    assert!(
        outputs
            .iter()
            .all(
                |output| output["fields"]["operation_executed"].as_str() != Some("true")
                    && output["fields"]["mutating_command_executed"].as_str() != Some("true")
            )
    );
    for phase in [
        "slice-op-dry-run-or-readonly-validator",
        "slice-op-prep-promotion-router",
        "slice-op-prep-maintenance-check-requester",
    ] {
        let output = outputs
            .iter()
            .find(|output| output["phase_id"] == phase)
            .unwrap();
        assert!(!output["dispositions"].as_array().unwrap().is_empty());
    }
    let dry = outputs
        .iter()
        .find(|output| output["phase_id"] == "slice-op-dry-run-or-readonly-validator")
        .unwrap();
    assert_eq!(dry["fields"]["non_mutating"], "true");
    let result = outputs
        .iter()
        .find(|output| output["phase_id"] == "slice-op-prep-result-writer")
        .unwrap();
    assert_eq!(result["fields"]["prepared_not_executed"], "true");
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
