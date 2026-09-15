#[path = "pipeline_execution/full_support.rs"]
mod pipeline_support;
#[allow(dead_code)]
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
use support::{id, open_slice, ready_source_candidate, repository, review, route, save};
use tect_postgres::admin;
use uuid::Uuid;

fn debug_draft() -> Value {
    json!({"coverage_summary":"Evidence-led root-cause lifecycle","nodes":[{
        "kind":"work","identity":{"local":"debug"},"title":"Establish the demonstrated root cause",
        "outcome":"The cause and correction direction are evidenced without changing source",
        "includes":["reproduction","evidence","root cause","follow-up handoff"],
        "excludes":["source fix","deployment"],"dependencies":[],
        "proof":["Reproduction and causal evidence"],"pipeline":"slice.debug-root-cause",
        "pipeline_reason":"Observed behavior conflicts with expectation and the cause is unknown",
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
async fn debug_pipeline_preserves_diagnosis_and_composes_fix_as_future_slice() {
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
    let socket = root.join("pipeline-debug.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-debug-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-debug-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let scope = route(
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
    let saved = save(&mut client, &scope["created"]["planning"], debug_draft()).await;
    let reviewed = review(&mut client, &saved).await;
    let opened = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = &opened["created"];
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "qualification_reason":"Start whole, then deepen phasewise as causal evidence accumulates."}),
    )
    .await;
    let mut context = begun["created"].clone();
    assert!(!id(&context["run"]["id"]).is_nil());
    assert_eq!(context["run"]["delivery_mode"], "whole");
    assert_eq!(
        context["run"]["definition_digest"],
        "ecd89aaae1265455b79b400f200a7f932a596dbd0c06700a90b0116f7aaeb2ac"
    );
    assert_eq!(
        context["definition"]["phases"].as_array().unwrap().len(),
        18
    );
    assert!(
        context["definition"]["phases"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|phase| {
                phase["skills"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .chain(phase["resources"].as_array().into_iter().flatten())
            })
            .all(
                |item| item["body"].as_str().is_some_and(|body| body.len() > 100)
                    && item["digest"]
                        .as_str()
                        .is_some_and(|digest| !digest.is_empty())
            )
    );

    assert_non_coding_definition(&context, "slice.debug-root-cause");
    let (verdict, outcome, transition) = successful_route(&context);
    let mut first = completion(&context, verdict, outcome, transition, None, None);
    assert_forged_implementation_phase_rejected(
        &mut client,
        &context,
        first.clone(),
        "slice.debug-root-cause",
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
    assert_non_coding_definition(&context, "slice.debug-root-cause");
    context = route(
        &mut client,
        "command",
        "slice.pipeline.delivery.escalate",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
            "reason":"The evidence graph now benefits from phase-local delivery."}),
    )
    .await["context"]
        .clone();
    assert_eq!(context["run"]["delivery_mode"], "phasewise");
    assert_eq!(context["definition"]["phases"].as_array().unwrap().len(), 1);
    context = refresh_knowledge(&mut client, &context).await;

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 12 {
        context = advance(&mut client, context).await;
    }
    let strategy = completion(
        &context,
        "fix_plan_ready",
        "completed",
        "continue",
        None,
        None,
    );
    assert_eq!(
        strategy["output"]["fields"]["source_mutation_performed"],
        "false"
    );
    assert!(matches!(
        strategy["output"]["fields"]["future_slice_kind"].as_str(),
        Some("slice.lightweight-tdd-development" | "slice.full-design-to-execution")
    ));
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        strategy,
    )
    .await["context"]
        .clone();

    context = advance(&mut client, context).await;
    let handoff = completion(
        &context,
        "implementation_slice_prepared",
        "completed",
        "continue",
        None,
        None,
    );
    assert_eq!(
        handoff["output"]["fields"]["source_mutation_performed"],
        "false"
    );
    assert_eq!(
        handoff["output"]["fields"]["implementation_slice_required"],
        "true"
    );
    assert!(matches!(
        handoff["output"]["fields"]["implementation_slice_kind"].as_str(),
        Some("slice.lightweight-tdd-development" | "slice.full-design-to-execution")
    ));
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        handoff,
    )
    .await["context"]
        .clone();

    let verification = completion(
        &context,
        "diagnosis_verified",
        "completed",
        "continue",
        None,
        None,
    );
    assert_eq!(
        verification["output"]["fields"]["implementation_fix_verified"],
        "false"
    );
    assert_eq!(
        verification["output"]["fields"]["source_mutation_performed"],
        "false"
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        verification,
    )
    .await["context"]
        .clone();
    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 18 {
        context = advance(&mut client, context).await;
    }
    let (verdict, outcome, transition) = successful_route(&context);
    let completed = route(&mut client,"command","slice.pipeline.phase.complete",
        completion(&context,verdict,outcome,transition,None,Some(json!({
            "summary":"Root cause and future implementation Slice handoff are recorded without a fix claim.",
            "evidence":[{"kind":"integration_test","reference":"pipeline_execution_debug.rs",
                "observation":"Diagnosis completed with source mutation false throughout."}],
            "scope_impact":"A future reviewed implementation candidate may consume this evidence.",
            "remaining_work":"Implement and verify the correction in a separate Lightweight or Full Slice."
        })))).await;
    assert_eq!(completed["context"]["run"]["status"], "completed");
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        18
    );
    assert!(
        completed["context"]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|output| output["fields"]["source_mutation_performed"].as_str() != Some("true"))
    );
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
