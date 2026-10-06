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
use recovery_support::native_reads::ScopeOpenFixture;
use recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{
    id, open_slice, ready_source_candidate, repository, review, route, route_error, save,
};
use tect_postgres::admin;
use uuid::Uuid;

fn mutation_result_id(context: &ResolvedPipeline) -> &Value {
    context
        .raw_payload
        .get("result_reference")
        .and_then(Value::as_object)
        .expect("actual mutation result reference object")
        .get("result_id")
        .expect("actual mutation result ID key")
}

fn execution_draft() -> Value {
    json!({"coverage_summary":"Bounded operational execution with exact authority and recovery","nodes":[{
        "kind":"work","identity":{"local":"execution"},"title":"Execute the authorized bounded operation",
        "outcome":"The exact target reaches its proven final state or stops and recovers safely",
        "includes":["authority","target","preflight","one action","checkpoint","proof","recovery"],
        "excludes":["unapproved target","unbounded action","silent effect retry"],"dependencies":[],
        "proof":["Exact target-bound action and recovery receipts"],"pipeline":"slice.operational-execution",
        "pipeline_reason":"A separately reviewed operation is authorized for bounded phasewise execution",
        "source_result_ids":[]
    }],"supersessions":[]})
}

async fn advance(client: &mut Mcp, context: ResolvedPipeline) -> ResolvedPipeline {
    let (verdict, outcome, transition) = successful_route(&context);
    let raw = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&context, verdict, outcome, transition, None, None),
    )
    .await;
    resolve_pipeline(client, raw).await.unwrap()
}

async fn setup(client: &mut Mcp, repo: &std::path::Path) -> ResolvedPipeline {
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
    let scope = ScopeOpenFixture::from_mutation(scope, "created");
    let planning = scope.read_planning(client).await.value;
    let saved = save(client, &planning, execution_draft()).await;
    let reviewed = review(client, &saved).await;
    let opened = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = &opened["created"];
    let raw = route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "qualification_reason":"The exact authorized action requires mandatory phasewise gates."}),
    ).await;
    resolve_pipeline(client, raw).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn operational_execution_gates_effects_replay_recovery_and_partial_resume() {
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
    let mut context = setup(&mut client, &repo).await;
    assert!(!id(&context.run()["id"]).is_nil());
    assert_eq!(context.run()["delivery_mode"], "phasewise");
    assert_eq!(
        context.run()["definition_digest"],
        "47046a703413f6e3048c6923b87dae6ceb0bbecb3c9e0f9d60ca614562267e79"
    );
    assert_eq!(
        context.details_data()["delivered_phases"]
            .as_array()
            .unwrap()
            .len(),
        1
    );

    assert_non_coding_definition(&context, "slice.operational-execution");
    let (verdict, outcome, transition) = successful_route(&context);
    let mut first = completion(&context, verdict, outcome, transition, None, None);
    assert_forged_implementation_phase_rejected(
        &mut client,
        &context,
        first.clone(),
        "slice.operational-execution",
    )
    .await;
    add_opaque_authority_labels(&mut first);
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        first,
    )
    .await;
    context = resolve_pipeline(&mut client, raw).await.unwrap();
    assert_non_coding_definition(&context, "slice.operational-execution");

    while context.run()["current_phase_ordinal"].as_u64().unwrap() < 8 {
        context = advance(&mut client, context).await;
    }
    let mut missing_authority = completion(
        &context,
        "approval_not_required",
        "completed",
        "continue",
        None,
        None,
    );
    missing_authority["output"]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("target_unchanged");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            missing_authority
        )
        .await["error"]["code"],
        "INVALID_OUTPUT"
    );
    let mut authority = completion(
        &context,
        "approval_not_required",
        "completed",
        "continue",
        None,
        None,
    );
    authority["output"]["fields"]["target_unchanged"] = json!("true");
    authority["output"]["fields"]["risk_unchanged"] = json!("true");
    authority["output"]["fields"]["prior_authority_recognized"] = json!("true");
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        authority,
    )
    .await;
    context = resolve_pipeline(&mut client, raw).await.unwrap();
    context = advance(&mut client, context).await;
    assert_eq!(context.run()["current_phase_ordinal"], 10);

    let unknown = completion(
        &context,
        "unknown_external_outcome",
        "blocked",
        "block",
        None,
        None,
    );
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        unknown.clone(),
    )
    .await;
    let blocked = resolve_pipeline(&mut client, raw).await.unwrap();
    assert_eq!(blocked.run()["status"], "blocked");
    assert!(mutation_result_id(&blocked).is_null());
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        unknown,
    )
    .await;
    let replay = resolve_pipeline(&mut client, raw).await.unwrap();
    assert_eq!(replay.raw_payload, blocked.raw_payload);
    assert_eq!(
        replay.details_data()["attempts"].as_array().unwrap().len(),
        10
    );
    context = replay;
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &context,
                "action_recorded",
                "completed",
                "continue",
                None,
                None,
            )
        )
        .await["error"]["code"],
        "input_pending"
    );
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
            "run_revision":context.run()["revision"],"phase_id":context.run()["current_phase_id"],
            "input":"Reconciled the exact target and effect request; no repeated action occurred."}),
    ).await;
    context = resolve_pipeline(&mut client, raw).await.unwrap();
    context = refresh_knowledge(&mut client, &context).await;
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "action_recorded",
            "completed",
            "continue",
            None,
            None,
        ),
    )
    .await;
    context = resolve_pipeline(&mut client, raw).await.unwrap();
    while context.run()["current_phase_ordinal"].as_u64().unwrap() < 18 {
        context = advance(&mut client, context).await;
    }

    let mut invalid_revisit = completion(
        &context,
        "partial_resume_action_runner",
        "completed",
        "continue",
        Some("slice-op-exec-action-runner"),
        None,
    );
    invalid_revisit["revisit_phase_id"] = json!("slice-op-exec-authority-confirmation");
    let invalid_revisit_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        invalid_revisit,
    )
    .await;
    assert_eq!(invalid_revisit_error["error"]["code"], "INVALID_OUTPUT");
    for (field, expected) in json!({
        "code":"INVALID_OUTPUT", "rule":"WP6-COMPLETE-OUTPUT-11",
        "path":"arguments.params.output.verdict",
        "expected":"verdict route matching outcome, transition and revisit phase", "actual":"no matching route",
        "next_action":"align_completion_with_verdict_route", "required":"valid_verdict_route",
        "message":"the submitted pipeline output violates its contract"
    })
    .as_object()
    .unwrap()
    {
        assert_eq!(invalid_revisit_error["error"]["refusal"][field], *expected);
    }
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "partial_resume_action_runner",
            "completed",
            "continue",
            Some("slice-op-exec-action-runner"),
            None,
        ),
    )
    .await;
    let partial = resolve_pipeline(&mut client, raw).await.unwrap();
    context = partial;
    assert_eq!(context.run()["status"], "active");
    assert_eq!(context.run()["current_phase_ordinal"], 10);
    assert!(
        context.details_data()["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|binding| binding["phase_ordinal"].as_u64().unwrap() >= 10
                && binding["stale"] == true)
    );
    assert!(
        context.details_data()["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|binding| binding["phase_ordinal"].as_u64().unwrap() >= 10
                || binding["stale"] == false)
    );

    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            completion(
                &context,
                "action_recorded",
                "completed",
                "continue",
                None,
                None,
            )
        )
        .await["error"]["code"],
        "input_pending"
    );
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
            "run_revision":context.run()["revision"],"phase_id":context.run()["current_phase_id"],
            "input":"Authorized resume at the exact action after partial-result reconciliation."}),
    )
    .await;
    context = resolve_pipeline(&mut client, raw).await.unwrap();
    context = refresh_knowledge(&mut client, &context).await;

    while context.run()["current_phase_ordinal"].as_u64().unwrap() < 18 {
        if context.run()["current_phase_id"] == "slice-op-exec-rollback-or-recovery-runner" {
            let (verdict, outcome, transition) = successful_route(&context);
            assert_eq!(
                route_error(
                    &mut client,
                    "command",
                    "slice.pipeline.phase.complete",
                    completion(&context, verdict, outcome, transition, None, None)
                )
                .await["error"]["code"],
                "input_pending"
            );
            let raw = route(
                &mut client,
                "command",
                "slice.pipeline.input",
                json!({"request_id":Uuid::new_v4(),"run_id":context.run()["id"],
                    "run_revision":context.run()["revision"],"phase_id":context.run()["current_phase_id"],
                    "input":"Reconciled recovery state and authority before the permitted recovery rerun."}),
            ).await;
            context = resolve_pipeline(&mut client, raw).await.unwrap();
            context = refresh_knowledge(&mut client, &context).await;
        }
        context = advance(&mut client, context).await;
    }
    let raw = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &context,
            "closed",
            "completed",
            "complete",
            None,
            Some(json!({
                "summary":"The exact authorized operation completed with checkpoint and recovery truth.",
                "evidence":[{"kind":"integration_test","reference":"pipeline_execution_operational_execution.rs",
                    "observation":"Authority, effect receipt replay, checkpoint, partial resume and terminal proof passed."}],
                "scope_impact":"The bounded operation has a terminal target-bound record.",
                "remaining_work":"No unproven effect is retried automatically."
            })),
        ),
    ).await;
    let completed = resolve_pipeline(&mut client, raw).await.unwrap();
    assert_eq!(completed.run()["status"], "completed");
    assert!(!mutation_result_id(&completed).is_null());
    assert_eq!(
        &completed.details_data()["result"]["id"],
        mutation_result_id(&completed)
    );
    assert!(completed.details_data()["result"].is_object());
    assert_eq!(
        completed.details_data()["attempts"]
            .as_array()
            .unwrap()
            .len(),
        28
    );
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
