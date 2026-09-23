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
        "INVALID_OUTPUT"
    );
    let mut substituted_resource = review_request.clone();
    let expected_resource_digest = review_request["output"]["resource_reads"][0]["digest"]
        .as_str()
        .unwrap()
        .to_owned();
    substituted_resource["request_id"] = json!(Uuid::new_v4());
    substituted_resource["output"]["resource_reads"][0]["digest"] = json!("substituted");
    let substituted_resource_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        substituted_resource,
    )
    .await;
    assert_eq!(
        substituted_resource_error["error"]["code"],
        "INVALID_OUTPUT"
    );
    let resource_refusal = &substituted_resource_error["error"]["refusal"];
    assert_eq!(resource_refusal["code"], "INVALID_OUTPUT");
    assert_eq!(resource_refusal["rule"], "WP6-RESOURCE-READ-01");
    assert_eq!(
        resource_refusal["path"],
        "arguments.params.output.resource_reads"
    );
    assert!(
        resource_refusal["expected"]
            .as_str()
            .unwrap()
            .contains(&expected_resource_digest),
        "{resource_refusal}"
    );
    assert!(
        resource_refusal["actual"]
            .as_str()
            .unwrap()
            .contains("substituted")
    );
    assert_eq!(
        resource_refusal["next_action"],
        "supply_exact_phase_resource_reads"
    );
    assert_eq!(resource_refusal["required"], "exact_phase_resource_reads");
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

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn lightweight_v07_accepts_omitted_body_and_persists_backend_evidence() {
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
    let socket = root.join("pipeline-optional-body.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-optional-body-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-optional-body-{}", Uuid::new_v4());
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
    let saved = save(
        &mut client,
        &opened_scope["created"]["planning"],
        lightweight_draft(),
    )
    .await;
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
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
            "scope_id":reviewed["scope"]["id"],"slice_id":slice["id"],
            "slice_revision":slice["revision"],"delivery_mode":"phasewise",
            "definition_version":"0.7.0-native.k1k5",
            "qualification_reason":"Verify the bounded v0.7 optional-body persistence contract."}),
    )
    .await;
    let initial = begun["created"].clone();
    let run_id = id(&initial["run"]["id"]);
    assert_eq!(initial["run"]["definition_version"], "0.7.0-native.k1k5");
    assert_eq!(initial["run"]["current_phase_id"], "K1");

    let mut k1_request = completion_request(&initial, "completed", "continue", None, false);
    assert!(k1_request["output"].get("body").is_some());
    k1_request["output"].as_object_mut().unwrap().remove("body");
    for (field, value) in [
        ("fit", "bounded_understood"),
        ("parent", "current_confirmed"),
        ("preflight", "current_clear"),
        ("authority", "authorized"),
        ("route", "none"),
    ] {
        k1_request["output"]["fields"][field] = json!(value);
    }
    let k1_request_id = id(&k1_request["request_id"]);
    let after_k1 = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        k1_request,
    )
    .await["context"]
        .clone();
    assert_eq!(after_k1["run"]["current_phase_id"], "K2");
    let k1_attempt = after_k1["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["phase_id"] == "K1")
        .unwrap();
    let k1_output_id = id(&k1_attempt["output_id"]);
    let k1_digest = k1_attempt["output_digest"].as_str().unwrap().to_owned();

    let k2 = after_k1["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K2")
        .unwrap();
    let explicit_body = "explicit body survives the pinned output read";
    let mut k2_output = phase_output(k2, "optional-body-K2", "completed", "continue");
    k2_output["body"] = json!(explicit_body);
    for (field, value) in [
        ("isolation", "confirmed"),
        ("ownership", "confirmed"),
        ("overlap", "clear"),
        ("route", "none"),
    ] {
        k2_output["fields"][field] = json!(value);
    }
    let k2_request_id = Uuid::new_v4();
    let after_k2 = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":k2_request_id,"run_id":run_id,
            "run_revision":after_k1["run"]["revision"],"phase_id":"K2",
            "outcome":"completed","transition":"continue","output":k2_output,
            "consumed_outputs":[],"consumed_inputs":[],"publish_blocked_result":false}),
    )
    .await["context"]
        .clone();
    assert_eq!(after_k2["run"]["current_phase_id"], "K3");

    let rows: Vec<(Uuid, String, serde_json::Value, String)> = sqlx::query_as(
        "SELECT id,body,fields,body_digest FROM slice_pipeline_phase_outputs \
         WHERE run_id=$1 AND phase_id IN ('K1','K2') ORDER BY phase_ordinal",
    )
    .bind(run_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, k1_output_id);
    assert_eq!(rows[0].1, "");
    assert!(rows[0].2["fit"].as_str().is_some());
    assert_eq!(rows[0].3, k1_digest);
    assert_eq!(rows[1].1, explicit_body);
    assert!(rows[1].2["source_provenance"].as_str().is_some());

    let evidence: serde_json::Value = sqlx::query_scalar(
        "SELECT evidence_refs FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND request_id=$2",
    )
    .bind(run_id)
    .bind(k2_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(evidence.as_array().unwrap().iter().any(|reference| {
        reference["kind"] == "output"
            && reference["reference"] == k1_output_id.to_string()
            && reference["phase_id"] == "K1"
            && reference["revision"] == 1
            && reference["digest"] == k1_digest
    }));
    let k1_attempt_count: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND request_id=$2",
    )
    .bind(run_id)
    .bind(k1_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(k1_attempt_count, 1);

    let empty_output = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id,"view":"output","output_id":k1_output_id,"digest":k1_digest}),
    )
    .await;
    assert!(empty_output.get("body").is_none());
    let explicit_output = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id,"view":"output","output_id":rows[1].0,"digest":rows[1].3}),
    )
    .await;
    assert_eq!(explicit_output["body"], explicit_body);

    let k3 = after_k2["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K3")
        .unwrap();
    let mut k3_rework_output = phase_output(k3, "rework-K3", "waiting_input", "continue");
    k3_rework_output["fields"]["review_mode"] = json!("self");
    let k3_rework_request_id = Uuid::new_v4();
    let k3_rework_request = json!({
        "request_id":k3_rework_request_id,"run_id":run_id,
        "run_revision":after_k2["run"]["revision"],"phase_id":"K3",
        "outcome":"waiting_input","transition":"continue","output":k3_rework_output,
        "consumed_outputs":[],"consumed_inputs":[],"revisit_phase_id":"K2",
        "publish_blocked_result":false
    });
    let reworked = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        k3_rework_request.clone(),
    )
    .await;
    let after_rework = reworked["context"].clone();
    assert_eq!(after_rework["run"]["status"], "active");
    assert_eq!(after_rework["run"]["current_phase_id"], "K2");
    assert_eq!(after_rework["run"]["current_phase_ordinal"], 2);
    assert_eq!(
        after_rework["run"]["revision"].as_i64().unwrap(),
        after_k2["run"]["revision"].as_i64().unwrap() + 1
    );

    let replayed_rework = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        k3_rework_request,
    )
    .await;
    assert_eq!(replayed_rework, reworked);
    let persisted_rework: (String, String, Option<String>, i64) = sqlx::query_as(
        "SELECT outcome,transition,revisit_phase_id,attempt FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND request_id=$2",
    )
    .bind(run_id)
    .bind(k3_rework_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted_rework,
        (
            "waiting_input".into(),
            "continue".into(),
            Some("K2".into()),
            1
        )
    );

    let k2 = after_rework["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K2")
        .unwrap();
    let mut k2_retry_output = phase_output(k2, "retry-K2", "completed", "continue");
    for (field, value) in [
        ("isolation", "confirmed"),
        ("ownership", "confirmed"),
        ("overlap", "clear"),
        ("route", "none"),
    ] {
        k2_retry_output["fields"][field] = json!(value);
    }
    let after_k2_retry = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":Uuid::new_v4(),"run_id":run_id,
            "run_revision":after_rework["run"]["revision"],"phase_id":"K2",
            "outcome":"completed","transition":"continue","output":k2_retry_output,
            "consumed_outputs":[],"consumed_inputs":[],"publish_blocked_result":false}),
    )
    .await["context"]
        .clone();
    assert_eq!(after_k2_retry["run"]["status"], "active");
    assert_eq!(after_k2_retry["run"]["current_phase_id"], "K3");

    let k3 = after_k2_retry["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K3")
        .unwrap();
    let mut k3_pass_output = phase_output(k3, "fresh-K3", "completed", "continue");
    k3_pass_output["fields"]["review_mode"] = json!("self");
    let after_k3_pass = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":Uuid::new_v4(),"run_id":run_id,
            "run_revision":after_k2_retry["run"]["revision"],"phase_id":"K3",
            "outcome":"completed","transition":"continue","output":k3_pass_output,
            "consumed_outputs":[],"consumed_inputs":[],"publish_blocked_result":false}),
    )
    .await["context"]
        .clone();
    assert_eq!(after_k3_pass["run"]["current_phase_id"], "K4");
    let attempt_counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT phase_id,pg_catalog.count(*) FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND phase_id IN ('K2','K3') GROUP BY phase_id ORDER BY phase_id",
    )
    .bind(run_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(attempt_counts, vec![("K2".into(), 2), ("K3".into(), 2)]);

    let migration_count: i64 =
        sqlx::query_scalar("SELECT pg_catalog.count(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(migration_count, 40);
    let body_check: String = sqlx::query_scalar(
        "SELECT pg_catalog.pg_get_constraintdef(oid) FROM pg_catalog.pg_constraint \
         WHERE conrelid='slice_pipeline_phase_outputs'::regclass \
           AND conname='slice_pipeline_phase_outputs_body_check'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(body_check.contains("2097152"), "{body_check}");
    assert!(!body_check.contains("btrim"), "{body_check}");
    client.finish().await;
}
