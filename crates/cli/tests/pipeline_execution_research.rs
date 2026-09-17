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

fn research_draft() -> Value {
    json!({"coverage_summary":"Bounded standalone evidence synthesis","nodes":[{
        "kind":"work","identity":{"local":"research"},"title":"Research a bounded technical question",
        "outcome":"A bounded negative result with traceable evidence and limitations",
        "includes":["sources","provenance","claims","contradictions","negative knowledge"],
        "excludes":["canonical write","automatic promotion","implementation"],"dependencies":[],
        "proof":["Cited claim ledger and bounded negative result"],"pipeline":"slice.research",
        "pipeline_reason":"The explicit question requires substantial evidence collection and synthesis.",
        "source_result_ids":[]
    }],"supersessions":[]})
}

async fn advance(client: &mut Mcp, context: Value) -> Value {
    let (verdict, outcome, transition) = successful_route(&context);
    let mut request = completion(&context, verdict, outcome, transition, None, None);
    if context["run"]["current_phase_id"] == "R01" {
        request["output"]["fields"]["topic_level"] = json!("scope");
        request["output"]["fields"]["allow_inconclusive"] = json!("false");
    }
    route(client, "command", "slice.pipeline.phase.complete", request).await["context"].clone()
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
            "qualification_reason":"The bounded scope research question fits whole delivery before evidence deepening.",
            "inquiry":{"topic_level":"scope","task_context":{"target_iris":[]},
                "completion":{"kind":"research","allow_inconclusive":false}}}),
    )
    .await;
    let mut context = begun["created"].clone();
    assert!(!id(&context["run"]["id"]).is_nil());
    assert_eq!(context["run"]["delivery_mode"], "whole");
    assert_eq!(context["inquiry"]["topic_level"], "scope");
    assert_eq!(
        context["inquiry"]["completion"]["allow_inconclusive"],
        false
    );
    assert_eq!(
        context["run"]["definition_digest"],
        "7d9a817dbbd4560aca33f46522027cf2aefad483bf5837bb98b494d533f115af"
    );
    assert_eq!(
        context["definition"]["phases"].as_array().unwrap().len(),
        12
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

    assert_non_coding_definition(&context, "slice.research");
    let (verdict, outcome, transition) = successful_route(&context);
    let mut first = completion(&context, verdict, outcome, transition, None, None);
    first["output"]["fields"]["topic_level"] = json!("scope");
    first["output"]["fields"]["allow_inconclusive"] = json!("false");
    assert_forged_implementation_phase_rejected(
        &mut client,
        &context,
        first.clone(),
        "slice.research",
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
    assert_non_coding_definition(&context, "slice.research");
    assert_eq!(context["outputs"][0]["fields"]["topic_level"], "scope");
    assert_eq!(
        context["outputs"][0]["fields"]["allow_inconclusive"],
        "false"
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.delivery.escalate",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],"phase_id":context["run"]["current_phase_id"],
            "reason":"Evidence provenance, contradictions and negative findings now warrant phase-local delivery."}),
    )
    .await["context"]
        .clone();
    assert_eq!(context["run"]["delivery_mode"], "phasewise");
    context = refresh_knowledge(&mut client, &context).await;

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 6 {
        context = advance(&mut client, context).await;
    }
    let mut incomplete_custody = completion(&context, "ready", "completed", "continue", None, None);
    incomplete_custody["output"]["fields"]["custody_complete"] = json!("false");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            incomplete_custody
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let mut unequal_custody = completion(&context, "ready", "completed", "continue", None, None);
    unequal_custody["output"]["fields"]["collected_count"] = json!("1");
    unequal_custody["output"]["fields"]["accounted_count"] = json!("0");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            unequal_custody
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = advance(&mut client, context).await;

    let mut incomplete_trace = completion(&context, "ready", "completed", "continue", None, None);
    incomplete_trace["output"]["fields"]["claim_trace_complete"] = json!("false");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            incomplete_trace
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = advance(&mut client, context).await;

    let reviewed_negative = completion(&context, "ready", "completed", "continue", None, None);
    let artifact_names = reviewed_negative["output"]["artifacts"]
        .as_array()
        .unwrap()
        .iter()
        .map(|artifact| artifact["name"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(artifact_names.contains(&"negative-knowledge.md"));
    assert!(artifact_names.contains(&"contradictions-and-gaps.md"));
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        reviewed_negative,
    )
    .await["context"]
        .clone();

    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &context,
                "bounded_inconclusive",
                "completed",
                "continue",
                None,
                None
            ),
        )
        .await["error"]["code"],
        "forbidden"
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "waiting_source",
            "waiting_input",
            "continue",
            None,
            None,
        ),
    )
    .await["context"]
        .clone();
    assert_eq!(context["run"]["status"], "waiting_input");
    context = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],"phase_id":"R09",
            "input":"The bounded negative probe now has its exact source response."}),
    )
    .await["context"]
        .clone();
    context = refresh_knowledge(&mut client, &context).await;
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&context, "ready", "completed", "continue", None, None),
    )
    .await["context"]
        .clone();
    context = advance(&mut client, context).await;
    context = advance(&mut client, context).await;
    assert_eq!(context["run"]["current_phase_id"], "R12");

    let terminal = json!({
        "summary":"The inspected boundary supports a bounded negative result.",
        "evidence":[{"kind":"integration_test","reference":"pipeline_execution_research.rs",
            "observation":"The current Research contract preserved traceability, contradiction and negative-knowledge evidence."}],
        "scope_impact":"The scope can rely on the bounded negative finding within its recorded limits.",
        "remaining_work":"Any durable publication remains a separate governed action."
    });
    let mut performed = completion(
        &context,
        "negative_result",
        "completed",
        "complete",
        None,
        Some(terminal.clone()),
    );
    performed["output"]["fields"]["publication_status"] = json!("performed");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            performed
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let mut failed_publication = completion(
        &context,
        "negative_result",
        "completed",
        "complete",
        None,
        Some(terminal.clone()),
    );
    failed_publication["output"]["fields"]["publication_status"] = json!("failed");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            failed_publication,
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let mut direct_publish = completion(
        &context,
        "negative_result",
        "completed",
        "complete",
        None,
        Some(terminal.clone()),
    );
    direct_publish["output"]["knowledge_publication"] = json!({
        "change_id":Uuid::new_v4(),
        "publisher_receipt_id":Uuid::new_v4(),
        "publisher_receipt_digest":"0123456789abcdef0123456789abcdef0123456789abcdef0123456789abcdef",
        "operation_ids":[Uuid::new_v4()]
    });
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            direct_publish,
        )
        .await["error"]["code"],
        "invalid_source"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &context,
                "inconclusive",
                "completed",
                "complete",
                None,
                Some(terminal.clone()),
            ),
        )
        .await["error"]["code"],
        "forbidden"
    );
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "negative_result",
            "completed",
            "complete",
            None,
            Some(terminal),
        ),
    )
    .await;
    assert_eq!(completed["context"]["run"]["status"], "completed");
    let outputs = completed["context"]["outputs"].as_array().unwrap();
    assert_eq!(outputs.len(), 12);
    assert_eq!(
        outputs
            .iter()
            .map(|output| output["phase_ordinal"].as_u64().unwrap())
            .collect::<Vec<_>>(),
        (1..=12).collect::<Vec<_>>()
    );
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        13
    );
    let r08 = outputs
        .iter()
        .find(|output| output["phase_id"] == "R08")
        .unwrap();
    assert!(
        r08["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["name"] == "negative-knowledge.md")
    );
    assert!(
        r08["artifacts"]
            .as_array()
            .unwrap()
            .iter()
            .any(|artifact| artifact["name"] == "contradictions-and-gaps.md")
    );
    assert!(
        completed["context"]["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|output| {
                output["fields"]["publication_status"].as_str() != Some("performed")
                    && output["fields"]["canonical_write_performed"].as_str() != Some("true")
                    && output["fields"]["durable_write_performed"].as_str() != Some("true")
            })
    );
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(enrollment.tenant_id)
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap();
    let knowledge_changes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(knowledge_changes, 0);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
