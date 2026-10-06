#[path = "pipeline_execution/delivery_contract.rs"]
mod delivery_contract;
#[path = "pipeline_execution/knowledge_refresh.rs"]
mod knowledge_refresh;
#[path = "pipeline_execution/lifecycle_support.rs"]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "pipeline_execution/resource_refusals.rs"]
mod resource_refusals;
#[path = "native_planning/support.rs"]
mod support;

use knowledge_refresh::refresh_pipeline_knowledge;
use lifecycle_support::{
    LIGHTWEIGHT_PHASES, complete, completion_request, lightweight_draft, phase_output, terminal,
};
use recovery_support::native_reads::ScopeOpenFixture;
use recovery_support::pipeline_reads::resolve_pipeline;
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

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn lightweight_pipeline_progresses_replays_recovers_and_records_managed_results() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    if std::env::var("TECT_TEST_DK2").as_deref() == Ok("1") {
        tect_postgres::enable_durable_knowledge(&pool, &role)
            .await
            .unwrap();
    }
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
    let opened_scope = ScopeOpenFixture::from_mutation(opened_scope, "created");
    let planning_read = opened_scope.read_planning(&mut client).await;
    let planning = &planning_read.value;
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
        "slice_revision":slice["revision"],"delivery_mode":"whole",
        "qualification_reason":"Agent explicitly selected whole delivery to inspect all five current phase contracts for this structural fixture."});
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        begin_request.clone(),
    )
    .await;
    let mut context = resolve_pipeline(&mut client, begun).await.unwrap();
    assert!(!id(&context.run()["id"]).is_nil());
    assert_eq!(context.run()["delivery_mode"], "whole");
    assert_eq!(context.run()["definition_version"], "0.7.1-native.k1k5");
    assert_eq!(context.current_phase().unwrap()["id"], "K1");
    assert_eq!(
        context.definition()["phases"]
            .as_array()
            .unwrap()
            .iter()
            .map(|phase| phase["id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        LIGHTWEIGHT_PHASES
    );
    assert_eq!(
        context.details_data()["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        5
    );
    assert!(
        context.definition()["phases"]
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
    let phase_two_id = context.definition()["phases"][1]["id"].as_str().unwrap();
    let phase_two_read = context
        .read_phase_contract(&mut client, phase_two_id)
        .await
        .unwrap();
    let phase_two = phase_two_read.value["phase"].clone();
    assert_eq!(phase_two["id"], "K2");
    assert_eq!(phase_two, context.definition()["phases"][1]);

    let replay = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        begin_request.clone(),
    )
    .await;
    assert_eq!(replay["replay"], context.compact_context);
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
    context = phase_one;
    assert!(resource_refusals::result_reference_id(&context.raw_payload).is_null());
    assert_eq!(context.run()["current_phase_ordinal"], 2);
    let phase_one_replay = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        phase_one_request,
    )
    .await;
    assert_eq!(phase_one_replay, context.raw_payload);
    // Current proof is backend-owned, even when the supplied digest is wrong.
    let agent_supplied_proof = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({
            "request_id":Uuid::new_v4(),"run_id":context.run()["id"],
            "run_revision":context.run()["revision"],
            "phase_id":context.run()["current_phase_id"],
            "outcome":"completed","transition":"continue",
            "output":phase_output(&phase_two, "wrong-consumption", "completed", "continue"),
            "consumed_outputs":[{"phase_id":LIGHTWEIGHT_PHASES[0],
                "output_revision":1,"digest":"not-the-pinned-output-digest"}],
            "consumed_inputs":[],
            "publish_blocked_result":false
        }),
    )
    .await;
    resource_refusals::assert_backend_proof_refusal(&mut client, &context, &agent_supplied_proof)
        .await;

    context = delivery_contract::escalate_and_verify(&mut client, &context).await;
    assert_eq!(context.run()["delivery_mode"], "phasewise");
    assert_eq!(
        context.details_data()["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.delivery.escalate",
            json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
                "run_revision":context.run()["revision"],
                "phase_id":context.run()["current_phase_id"],"reason":"repeat"})
        )
        .await["error"]["code"],
        "forbidden"
    );
    context = refresh_pipeline_knowledge(&mut client, &context, "stale").await;

    let (waiting, _) = complete(
        &mut client,
        &context,
        "waiting_input",
        "continue",
        None,
        false,
    )
    .await;
    context = waiting;
    assert_eq!(context.run()["status"], "waiting_input");
    let input = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
            "run_revision":context.run()["revision"],
            "phase_id":context.run()["current_phase_id"],
            "input":"Operator supplied the missing bounded context without changing authority."}),
    )
    .await;
    context = resolve_pipeline(&mut client, input.clone()).await.unwrap();
    assert_eq!(context.run()["status"], "active");
    assert_eq!(
        context.details_data()["inputs"].as_array().unwrap().len(),
        1
    );
    context = refresh_pipeline_knowledge(&mut client, &context, "current").await;

    let resume = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"]}),
    )
    .await;
    let resume = resolve_pipeline(&mut client, resume).await.unwrap();
    assert_eq!(
        resume.details_data()["inputs"][0]["input"],
        context.details_data()["inputs"][0]["input"]
    );
    assert_eq!(
        resume.details_data()["outputs"][0]["body"],
        context.details_data()["outputs"][0]["body"]
    );
    assert_eq!(
        resume.details_data()["outputs"][0]["fields"],
        context.details_data()["outputs"][0]["fields"]
    );
    assert_eq!(
        resume.details_data()["outputs"][0]["skill_reads"],
        context.details_data()["outputs"][0]["skill_reads"]
    );
    let run_id = context.run()["id"].clone();
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
    let cold = resolve_pipeline(&mut client, cold).await.unwrap();
    assert_eq!(cold.run(), context.run());
    assert_eq!(
        cold.details_data()["inputs"],
        context.details_data()["inputs"]
    );
    assert_eq!(
        cold.details_data()["outputs"],
        context.details_data()["outputs"]
    );
    context = cold;

    let (retried, _) = complete(&mut client, &context, "completed", "continue", None, false).await;
    context = retried;
    assert_eq!(
        context.details_data()["attempts"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|attempt| attempt["phase_ordinal"] == 2)
            .count(),
        2
    );

    assert_eq!(context.run()["current_phase_id"], "K3");
    let review_request = completion_request(&context, "completed", "continue", None, false);
    let reviewed_phase = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        review_request,
    )
    .await;
    context = resolve_pipeline(&mut client, reviewed_phase).await.unwrap();
    assert_eq!(context.run()["current_phase_id"], "K4");

    let mut agent_supplied_review_proof =
        completion_request(&context, "completed", "continue", None, false);
    agent_supplied_review_proof["consumed_outputs"] =
        resource_refusals::supplied_review_proof(&context);
    let refusal = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        agent_supplied_review_proof,
    )
    .await;
    resource_refusals::assert_backend_proof_refusal(&mut client, &context, &refusal).await;

    let (implemented, _) =
        complete(&mut client, &context, "completed", "continue", None, false).await;
    context = implemented;
    assert_eq!(context.run()["current_phase_id"], "K5");

    let (blocked, _) = complete(
        &mut client,
        &context,
        "blocked",
        "block",
        Some(terminal("Caller reports the final phase blocked.")),
        true,
    )
    .await;
    context = blocked;
    let blocked_result = resource_refusals::published_result(&context).clone();
    assert_eq!(blocked_result["pipeline_result_origin"], "managed_blocked");
    assert_eq!(blocked_result["provenance"], "externally_reported");
    assert_eq!(context.run()["status"], "blocked");

    let resumed = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
            "run_revision":context.run()["revision"],
            "phase_id":context.run()["current_phase_id"],
            "input":"Resume evidence resolves the reported terminal blocker."}),
    )
    .await;
    context = resolve_pipeline(&mut client, resumed.clone())
        .await
        .unwrap();
    context = refresh_pipeline_knowledge(&mut client, &context, "current").await;
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
    let completed_context = completed;
    let completed_result = resource_refusals::published_result(&completed_context);
    assert_eq!(
        completed_result["pipeline_result_origin"],
        "managed_completed"
    );
    assert_eq!(completed_result["provenance"], "externally_reported");
    assert_eq!(completed_context.run()["status"], "completed");
    assert!(completed_context.run()["current_phase_id"].is_null());
    assert!(completed_context.current_phase().is_err());
    assert_eq!(
        completed_result["slice_revision"].as_i64().unwrap(),
        blocked_result["slice_revision"].as_i64().unwrap() + 1
    );
    assert_ne!(completed_result["id"], blocked_result["id"]);
    let completed_bypass = json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":slice["id"],"slice_revision":completed_result["slice_revision"],"outcome":"completed",
        "summary":"Bypass completed managed history","evidence":[{"kind":"test","reference":"fixture","observation":"Must remain managed"}],
        "scope_impact":"None","remaining_work":"None"});
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.result.record",
            completed_bypass
        )
        .await["error"]["code"],
        "forbidden"
    );
    assert_eq!(
        completed_context.details_data()["attempts"]
            .as_array()
            .unwrap()
            .len(),
        7
    );
    assert_eq!(
        completed_context.details_data()["outputs"]
            .as_array()
            .unwrap()
            .len(),
        5
    );

    for (phase_id, attempts) in [("K1", 1), ("K2", 2), ("K3", 1), ("K4", 1), ("K5", 2)] {
        assert_eq!(
            completed_context.details_data()["attempts"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|attempt| attempt["phase_id"] == phase_id)
                .count(),
            attempts
        );
        assert_eq!(
            completed_context.details_data()["outputs"]
                .as_array()
                .unwrap()
                .iter()
                .filter(|output| output["phase_id"] == phase_id)
                .count(),
            1
        );
    }
    assert_eq!(
        completed_context.details_data()["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .map(|output| output["phase_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        LIGHTWEIGHT_PHASES
    );

    let read = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":completed_context.run()["id"]}),
    )
    .await;
    let read = resolve_pipeline(&mut client, read).await.unwrap();
    assert_eq!(read.run(), completed_context.run());
    assert_eq!(read.details_data()["result"]["id"], completed_result["id"]);
    assert!(
        read.details_data()["outputs"]
            .as_array()
            .unwrap()
            .iter()
            .all(|output| {
                !output["body"].as_str().unwrap().is_empty()
                    && !output["digest"].as_str().unwrap().is_empty()
            })
    );
}

#[path = "pipeline_execution/optional_body.rs"]
mod optional_body;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn lightweight_v07_accepts_omitted_body_and_persists_backend_evidence() {
    optional_body::run().await;
}
