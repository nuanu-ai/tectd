#[path = "pipeline_execution/lifecycle_support.rs"]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use lifecycle_support::{
    LIGHTWEIGHT_PHASES, complete, completion_request, lightweight_draft, phase_output, terminal,
};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::json;
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

async fn refresh_pipeline_knowledge(
    client: &mut Mcp,
    context: &serde_json::Value,
) -> serde_json::Value {
    let stale = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["run"]["id"], context["run"]["id"]);
    assert_eq!(stale["run"]["revision"], context["run"]["revision"]);
    assert_eq!(
        stale["run"]["current_phase_id"],
        context["run"]["current_phase_id"]
    );
    if stale["knowledge_resource_status"]["state"] == "inactive" {
        assert!(stale["knowledge_resources"].is_null());
        assert!(stale["knowledge"].is_null());
        assert!(find_action(&stale, "pipeline.knowledge_refresh").is_none());
        return stale;
    }
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    assert_eq!(
        stale["run"]["revision"].as_i64().unwrap(),
        stale["knowledge_resources"]["run_revision"]
            .as_i64()
            .unwrap()
            + 1
    );
    let action = find_action(&stale, "pipeline.knowledge_refresh")
        .expect("stale pipeline knowledge must expose its exact refresh action");
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(action).clone(),
    )
    .await;
    let current = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    current
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn lightweight_pipeline_progresses_replays_recovers_and_records_managed_results() {
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
    let socket = root.join("pipeline-execution.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-execution-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-execution-{}", Uuid::new_v4());
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
    let saved = save(&mut client, planning, lightweight_draft()).await;
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

    let begin_request = json!({"request_id":Uuid::new_v4(),
        "scope_id":reviewed["scope"]["id"],"slice_id":slice["id"],
        "slice_revision":slice["revision"],
        "qualification_reason":"Agent selected the default whole delivery for this bounded fixture."});
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        begin_request.clone(),
    )
    .await;
    let mut context = begun["created"].clone();
    assert!(!id(&context["run"]["id"]).is_nil());
    assert_eq!(context["run"]["delivery_mode"], "whole");
    assert_eq!(
        context["definition"]["phases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|phase| phase["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        LIGHTWEIGHT_PHASES
    );
    assert_eq!(context["delivered_phases"].as_array().unwrap().len(), 15);
    assert!(
        context["definition"]["phases"]
            .as_array()
            .unwrap()
            .iter()
            .flat_map(|phase| phase["instructions"].as_array().unwrap())
            .all(
                |body| body["body"].as_str().is_some_and(|value| !value.is_empty())
                    && body["digest"]
                        .as_str()
                        .is_some_and(|value| !value.is_empty())
            )
    );
    let phase_two = context["definition"]["phases"][1].clone();

    let replay = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        begin_request.clone(),
    )
    .await;
    assert_eq!(replay["replay"], context);
    let mut conflict = begin_request;
    conflict["qualification_reason"] = json!("changed rationale");
    assert_eq!(
        route_error(&mut client, "command", "slice.pipeline.begin", conflict).await["error"]["code"],
        "input_conflict"
    );

    let legacy = route_error(
        &mut client,
        "command",
        "slice.result.record",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],"outcome":"completed",
            "summary":"Attempted bypass","evidence":[{"kind":"test","reference":"fixture",
                "observation":"The bypass must fail."}],"scope_impact":"None","remaining_work":"Managed run"}),
    )
    .await;
    assert_eq!(legacy["error"]["code"], "forbidden");

    let (phase_one, phase_one_request) =
        complete(&mut client, &context, "completed", "continue", None, false).await;
    context = phase_one["context"].clone();
    assert!(phase_one["result"].is_null());
    assert_eq!(context["run"]["current_phase_ordinal"], 2);
    let phase_one_replay = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        phase_one_request,
    )
    .await;
    assert_eq!(phase_one_replay, phase_one);

    let wrong_consumption = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({
            "request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],
            "phase_id":context["run"]["current_phase_id"],
            "outcome":"completed","transition":"continue",
            "output":phase_output(&phase_two, "wrong-consumption", "completed", "continue"),
            "consumed_outputs":[{"phase_id":LIGHTWEIGHT_PHASES[0],
                "output_revision":1,"digest":"not-the-pinned-output-digest"}],
            "consumed_inputs":[],
            "publish_blocked_result":false
        }),
    )
    .await;
    assert_eq!(wrong_consumption["error"]["code"], "stale_context");

    let escalated = route(
        &mut client,
        "command",
        "slice.pipeline.delivery.escalate",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],
            "phase_id":context["run"]["current_phase_id"],
            "reason":"The remaining context now warrants phasewise delivery."}),
    )
    .await;
    context = escalated["context"].clone();
    assert_eq!(context["run"]["delivery_mode"], "phasewise");
    assert_eq!(context["delivered_phases"].as_array().unwrap().len(), 1);
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.delivery.escalate",
            json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
                "run_revision":context["run"]["revision"],
                "phase_id":context["run"]["current_phase_id"],"reason":"repeat"})
        )
        .await["error"]["code"],
        "forbidden"
    );
    context = refresh_pipeline_knowledge(&mut client, &context).await;

    let (waiting, _) = complete(
        &mut client,
        &context,
        "waiting_input",
        "continue",
        None,
        false,
    )
    .await;
    context = waiting["context"].clone();
    assert_eq!(context["run"]["status"], "waiting_input");
    let input = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],
            "phase_id":context["run"]["current_phase_id"],
            "input":"Operator supplied the missing bounded context without changing authority."}),
    )
    .await;
    context = input["context"].clone();
    assert_eq!(context["run"]["status"], "active");
    assert_eq!(context["inputs"].as_array().unwrap().len(), 1);
    context = refresh_pipeline_knowledge(&mut client, &context).await;

    let resume = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(resume["inputs"][0]["input"], context["inputs"][0]["input"]);
    assert_eq!(resume["outputs"][0]["body"], context["outputs"][0]["body"]);
    assert_eq!(
        resume["outputs"][0]["fields"],
        context["outputs"][0]["fields"]
    );
    assert_eq!(
        resume["outputs"][0]["skill_reads"],
        context["outputs"][0]["skill_reads"]
    );
    let run_id = context["run"]["id"].clone();
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    let _restarted_daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    client.call("open_workspace", json!({})).await;
    let cold = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    assert_eq!(cold["run"], context["run"]);
    assert_eq!(cold["inputs"], context["inputs"]);
    assert_eq!(cold["outputs"], context["outputs"]);
    context = cold;

    let (retried, _) = complete(&mut client, &context, "completed", "continue", None, false).await;
    context = retried["context"].clone();
    assert_eq!(
        context["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["phase_ordinal"] == 2)
            .count(),
        2
    );

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 8 {
        let (advanced, _) =
            complete(&mut client, &context, "completed", "continue", None, false).await;
        context = advanced["context"].clone();
    }

    assert_eq!(
        context["run"]["current_phase_id"],
        "slice-lightweight-pre-implementation-review"
    );
    let review_request = completion_request(&context, "completed", "continue", None, false);
    let mut early_code = review_request.clone();
    early_code["request_id"] = json!(Uuid::new_v4());
    early_code["phase_id"] = json!("slice-tdd-cycle-runner");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            early_code,
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let mut substituted_resource = review_request.clone();
    substituted_resource["request_id"] = json!(Uuid::new_v4());
    substituted_resource["output"]["resource_reads"][0]["digest"] = json!("substituted");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            substituted_resource,
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    context = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        review_request,
    )
    .await["context"]
        .clone();
    assert_eq!(context["run"]["current_phase_id"], "slice-tdd-cycle-runner");

    let mut stale_approval = completion_request(&context, "completed", "continue", None, false);
    let plan_review = stale_approval["consumed_outputs"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|binding| binding["phase_id"] == "slice-lightweight-pre-implementation-review")
        .unwrap();
    plan_review["digest"] = json!("stale-reviewed-input-digest");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            stale_approval,
        )
        .await["error"]["code"],
        "stale_context"
    );

    while context["run"]["current_phase_ordinal"].as_u64().unwrap() < 15 {
        let (advanced, _) =
            complete(&mut client, &context, "completed", "continue", None, false).await;
        context = advanced["context"].clone();
    }

    let (blocked, _) = complete(
        &mut client,
        &context,
        "blocked",
        "block",
        Some(terminal("Caller reports the final phase blocked.")),
        true,
    )
    .await;
    let blocked_result = blocked["result"].clone();
    context = blocked["context"].clone();
    assert_eq!(blocked_result["pipeline_result_origin"], "managed_blocked");
    assert_eq!(blocked_result["provenance"], "externally_reported");
    assert_eq!(context["run"]["status"], "blocked");

    let resumed = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
            "run_revision":context["run"]["revision"],
            "phase_id":context["run"]["current_phase_id"],
            "input":"Resume evidence resolves the reported terminal blocker."}),
    )
    .await;
    context = resumed["context"].clone();
    context = refresh_pipeline_knowledge(&mut client, &context).await;
    let (completed, _) = complete(
        &mut client,
        &context,
        "completed",
        "complete",
        Some(terminal(
            "Caller reports the managed Lightweight Slice complete.",
        )),
        false,
    )
    .await;
    let completed_result = &completed["result"];
    assert_eq!(
        completed_result["pipeline_result_origin"],
        "managed_completed"
    );
    assert_eq!(completed_result["provenance"], "externally_reported");
    assert_eq!(completed["context"]["run"]["status"], "completed");
    assert_eq!(
        completed_result["slice_revision"].as_i64().unwrap(),
        blocked_result["slice_revision"].as_i64().unwrap() + 1
    );
    assert_ne!(completed_result["id"], blocked_result["id"]);
    assert_eq!(
        completed["context"]["attempts"].as_array().unwrap().len(),
        17
    );
    assert_eq!(
        completed["context"]["outputs"].as_array().unwrap().len(),
        15
    );

    let read = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":completed["context"]["run"]["id"]}),
    )
    .await;
    assert_eq!(read["run"], completed["context"]["run"]);
    assert_eq!(read["result"]["id"], completed_result["id"]);
    assert!(read["outputs"].as_array().unwrap().iter().all(|output| {
        !output["body"].as_str().unwrap().is_empty()
            && !output["digest"].as_str().unwrap().is_empty()
    }));
}
