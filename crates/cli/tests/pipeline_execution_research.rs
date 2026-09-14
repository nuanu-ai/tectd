#[path = "pipeline_execution/full_support.rs"]
mod pipeline_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use pipeline_support::{completion, refresh_knowledge, successful_route};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

fn research_draft() -> Value {
    json!({"coverage_summary":"Bounded evidence synthesis and proposal-only durable handoff","nodes":[{
        "kind":"work","identity":{"local":"research"},"title":"Research a bounded technical question",
        "outcome":"Claims, negative knowledge, contradictions and gaps are traceable without publication",
        "includes":["sources","provenance","freshness","claims","negative knowledge","proposal"],
        "excludes":["canonical write","automatic promotion"],"dependencies":[],
        "proof":["Cited claim ledger and proposal-only handoff"],"pipeline":"slice.research-to-durable-knowledge",
        "pipeline_reason":"The bounded question can start whole, then deepen as evidence and gaps accumulate",
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
async fn research_preserves_provenance_negative_knowledge_and_proposal_boundary() {
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
    let socket = root.join("pipeline-research.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-research-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-research-{}", Uuid::new_v4());
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
    let saved = save(&mut client, &scope["created"]["planning"], research_draft()).await;
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
            "slice_id":slice["id"],"slice_revision":slice["revision"],"delivery_mode":"whole",
            "qualification_reason":"The bounded research question fits whole delivery before evidence deepening."}),
    )
    .await;
    let mut context = begun["created"].clone();
    assert!(!id(&context["run"]["id"]).is_nil());
    assert_eq!(context["run"]["delivery_mode"], "whole");
    assert_eq!(
        context["run"]["definition_digest"],
        "374987b7516fe57c4de4282ace1a0fd80712e0bcac664ee08057ab340fc0b8ce"
    );
    assert_eq!(
        context["definition"]["phases"].as_array().unwrap().len(),
        22
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
    context = advance(&mut client, context).await;
    context = route(
        &mut client,
        "command",
        "slice.pipeline.delivery.escalate",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
            "reason":"Evidence provenance, contradictions and gaps now warrant phase-local delivery."}),
    )
    .await["context"]
        .clone();
    assert_eq!(context["run"]["delivery_mode"], "phasewise");
    context = refresh_knowledge(&mut client, &context).await;

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 8 {
        context = advance(&mut client, context).await;
    }
    let (verdict, outcome, transition) = successful_route(&context);
    let mut dispatched = completion(&context, verdict, outcome, transition, None, None);
    dispatched["output"]["fields"]["direct_dispatch_performed"] = json!("true");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            dispatched
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = advance(&mut client, context).await;
    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 17 {
        context = advance(&mut client, context).await;
    }
    let (verdict, outcome, transition) = successful_route(&context);
    let mut canonical_write = completion(&context, verdict, outcome, transition, None, None);
    canonical_write["output"]["fields"]["canonical_write_performed"] = json!("true");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            canonical_write
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = advance(&mut client, context).await;
    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 20 {
        context = advance(&mut client, context).await;
    }
    let waiting = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "decision_waiting",
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
            "input":"The durable owner records no promotion is required; retain the proposal and evidence."}),
    )
    .await["context"]
        .clone();
    context = refresh_knowledge(&mut client, &context).await;
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "promotion_not_required",
            "completed",
            "continue",
            None,
            None,
        ),
    )
    .await["context"]
        .clone();
    context = advance(&mut client, context).await;
    assert_eq!(context["run"]["current_phase_ordinal"], 22);
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "research_synthesized_not_promoted",
            "completed",
            "complete",
            None,
            Some(json!({
                "summary":"The bounded research is synthesized with provenance, gaps and negative knowledge.",
                "evidence":[{"kind":"integration_test","reference":"pipeline_execution_research.rs",
                    "observation":"Claim, freshness, contradiction, proposal and promotion-boundary receipts completed."}],
                "scope_impact":"A durable owner may review the proposal without any canonical write.",
                "remaining_work":"Promotion remains a separately governed durable-domain action."
            })),
        ),
    )
    .await;
    assert_eq!(completed["context"]["run"]["status"], "completed");
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        23
    );
    let outputs = completed["context"]["outputs"].as_array().unwrap();
    assert!(outputs.iter().all(
        |output| output["fields"]["canonical_write_performed"].as_str() != Some("true")
            && output["fields"]["durable_write_performed"].as_str() != Some("true")
            && output["fields"]["index_update_performed"].as_str() != Some("true")
    ));
    assert!(outputs.iter().any(|output| output["phase_id"]
        == "slice-research-negative-knowledge-capturer"
        && output["fields"]["negative_record_count"] == "1"));
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
