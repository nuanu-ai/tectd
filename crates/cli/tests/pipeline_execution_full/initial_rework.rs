use super::*;

pub(super) async fn run(
    client: &mut Mcp,
    pool: &PgPool,
    mut context: ResolvedPipeline,
    review_sessions: &mut review_sessions::ReviewSessions,
) -> ResolvedPipeline {
    context = advance(client, context).await;
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
            client,
            "command",
            "slice.pipeline.phase.complete",
            wrong_digest
        )
        .await["error"]["code"],
        "INVALID_OUTPUT"
    );
    context = advance(client, context).await;
    context = advance(client, context).await;
    let (phase_four_verdict, phase_four_outcome, phase_four_transition) =
        successful_route(&context);
    let facts = full_support::native_contract_fixture_facts(client).await;
    let mut malformed_json = full_support::completion_with_contract(
        &context,
        completion(
            &context,
            phase_four_verdict,
            phase_four_outcome,
            phase_four_transition,
            None,
            None,
        ),
        &facts,
    );
    malformed_json["output"]["artifacts"][0]["body"] = json!("not json");
    malformed_json["output"]["artifacts"][0]["digest"] =
        json!("7ccfa1fb147ea0cb851480c39f28c0f78a2b035aeed0d2cf5e4c13d0d2adca4d");
    assert_eq!(
        route_error(
            client,
            "command",
            "slice.pipeline.phase.complete",
            malformed_json
        )
        .await["error"]["code"],
        "INVALID_OUTPUT"
    );
    context = advance(client, context).await;
    assert_eq!(
        context.run()["current_phase_id"],
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
        client,
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
    let raw_after_empty_ledger = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"],"refresh":true}),
    )
    .await;
    let after_empty_ledger = resolve_pipeline(client, raw_after_empty_ledger)
        .await
        .unwrap();
    assert_eq!(after_empty_ledger.run(), context.run());
    for collection in ["attempts", "outputs", "bindings"] {
        assert_eq!(
            if collection == "run" {
                after_empty_ledger.run()
            } else {
                &after_empty_ledger.details_data()[collection]
            },
            if collection == "run" {
                context.run()
            } else {
                &context.details_data()[collection]
            }
        );
    }
    let old_target = context.details_data()["bindings"]
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
    let wrong_rework_error = route_error(
        client,
        "command",
        "slice.pipeline.phase.complete",
        wrong_rework,
    )
    .await;
    assert_eq!(wrong_rework_error["error"]["code"], "INVALID_OUTPUT");
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
        assert_eq!(wrong_rework_error["error"]["refusal"][field], *expected);
    }

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
    let raw_reworked = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        valid_phase_five,
    )
    .await;
    let reworked = resolve_pipeline(client, raw_reworked).await.unwrap();
    assert!(mutation_result_id(&reworked).is_null());
    context = reworked;
    assert_eq!(
        context.run()["current_phase_id"],
        "slice-design-spec-shaper"
    );
    for binding in context.details_data()["bindings"].as_array().unwrap() {
        let ordinal = binding["phase_ordinal"].as_u64().unwrap();
        assert_eq!(binding["stale"], ordinal >= 3);
    }
    let stale_target = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"],"view":"output",
            "output_id":old_target["output_id"],"digest":old_target["output_digest"]}),
    )
    .await;
    assert_eq!(stale_target["stale"], true);
    assert_eq!(
        stale_target["stale_reason"],
        "rework_from:slice-design-spec-shaper"
    );

    for _ in 0..3 {
        context = advance(client, context).await;
    }
    assert_eq!(
        context.run()["current_phase_id"],
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
    context = review_sessions.complete(context, review_request).await;

    assert_eq!(
        context.run()["current_phase_id"],
        "slice-reconciliation-runner"
    );
    let (verdict, outcome, transition) = successful_route(&context);
    assert_eq!(verdict, "not_required");
    let mut dropped = completion(&context, verdict, outcome, transition, None, None);
    replace_ledger(&mut dropped["output"], 5);
    let dropped_error =
        route_error(client, "command", "slice.pipeline.phase.complete", dropped).await;
    assert_eq!(dropped_error["error"]["code"], "invalid_arguments");
    assert_eq!(dropped_error["error"]["refusal"]["code"], "INVALID_OUTPUT");
    assert_eq!(dropped_error["error"]["refusal"]["rule"], "WP6-COMPLETE-01");
    let raw_after_rejection = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"],"refresh":true}),
    )
    .await;
    let after_rejection = resolve_pipeline(client, raw_after_rejection).await.unwrap();
    assert_eq!(after_rejection.run()["revision"], context.run()["revision"]);
    assert_eq!(
        after_rejection.run()["current_phase_id"],
        "slice-reconciliation-runner"
    );
    for collection in ["attempts", "outputs", "bindings"] {
        assert_eq!(
            if collection == "run" {
                after_rejection.run()
            } else {
                &after_rejection.details_data()[collection]
            },
            if collection == "run" {
                context.run()
            } else {
                &context.details_data()[collection]
            },
            "{collection} persisted"
        );
    }
    let mut blocked_no_revisit = completion(&context, verdict, outcome, transition, None, None);
    blocked_no_revisit["output"]["verdict"] = json!("blocked_unreconciled_findings");
    blocked_no_revisit["output"]["dispositions"] = json!(["blocked_unreconciled_findings"]);
    blocked_no_revisit["outcome"] = json!("blocked");
    blocked_no_revisit["transition"] = json!("block");
    let raw_context = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        blocked_no_revisit,
    )
    .await;
    context = resolve_pipeline(client, raw_context).await.unwrap();
    assert!(mutation_result_id(&context).is_null());
    assert_eq!(context.run()["status"], "blocked");
    assert_eq!(
        context.run()["current_phase_id"],
        "slice-reconciliation-runner"
    );
    assert!(
        context.details_data()["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .all(|binding| binding["stale"] == false)
    );
    let phase_five_binding = context.details_data()["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "slice-component-decision-interrogator")
        .unwrap();
    let mut phase_five_artifacts: Value =
        sqlx::query_scalar("SELECT artifacts FROM slice_pipeline_phase_outputs WHERE id=$1")
            .bind(id(&phase_five_binding["output_id"]))
            .fetch_one(pool)
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
        .execute(pool)
        .await
        .unwrap();
    let raw_context = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"]}),
    )
    .await;
    context = resolve_pipeline(client, raw_context).await.unwrap();

    let (verdict, outcome, transition) = successful_route(&context);
    let forward_with_legacy_phase_five =
        completion(&context, verdict, outcome, transition, None, None);
    let legacy_forward_error = route_error(
        client,
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
    let raw_after_legacy_forward_rejection = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"]}),
    )
    .await;
    let after_legacy_forward_rejection =
        resolve_pipeline(client, raw_after_legacy_forward_rejection)
            .await
            .unwrap();
    for collection in ["run", "attempts", "outputs", "bindings"] {
        assert_eq!(
            if collection == "run" {
                after_legacy_forward_rejection.run()
            } else {
                &after_legacy_forward_rejection.details_data()[collection]
            },
            if collection == "run" {
                context.run()
            } else {
                &context.details_data()[collection]
            },
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
        client,
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
    let raw_recovery = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        recovery_request,
    )
    .await;
    let recovery = resolve_pipeline(client, raw_recovery).await.unwrap();
    assert!(mutation_result_id(&recovery).is_null());
    context = recovery;
    assert_eq!(
        context.run()["current_phase_id"],
        "slice-component-decision-interrogator"
    );
    for binding in context.details_data()["bindings"].as_array().unwrap() {
        let ordinal = binding["phase_ordinal"].as_u64().unwrap();
        if ordinal >= 5 {
            assert_eq!(binding["stale"], true);
            assert_eq!(
                binding["stale_reason"],
                "rework_from:slice-component-decision-interrogator"
            );
        }
    }
    context = advance(client, context).await;
    context = review_sessions.advance(context).await;
    assert_eq!(
        context.run()["current_phase_id"],
        "slice-reconciliation-runner"
    );
    context = advance(client, context).await;
    assert_eq!(
        context.run()["current_phase_id"],
        "slice-implementation-spec-synthesizer"
    );

    context
}
