use super::recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
use super::*;

pub(super) async fn validate(
    client: &mut Mcp,
    pool: &PgPool,
    mut c: ResolvedPipeline,
) -> ResolvedPipeline {
    c = advance(client, c).await; // Current successful execution, P13.
    c = advance(client, c).await; // Current successful local proof, P14.
    let policy = completion(
        &c,
        "deployment_not_required",
        "completed",
        "continue",
        None,
        None,
    );
    let raw_policy = route(client, "command", "slice.pipeline.phase.complete", policy).await;
    c = resolve_pipeline(client, raw_policy)
        .await
        .expect("resolve actual deployment policy completion");
    assert_eq!(c.run()["current_phase_ordinal"], 16);
    let local = completion(
        &c,
        "completed_local_verified",
        "completed",
        "continue",
        None,
        None,
    );
    for (key, value) in [
        ("route", "user_handoff"),
        ("highest_validated_truth", "live_verified"),
        ("terminal_state_candidate", "completed_deploy_verified"),
        ("deployment_required", "true"),
    ] {
        let mut bad = local.clone();
        bad["request_id"] = json!(Uuid::new_v4());
        bad["output"]["fields"][key] = json!(value);
        refuses_without_persistence(client, &c, bad).await;
    }
    let mut missing_policy = local.clone();
    missing_policy["request_id"] = json!(Uuid::new_v4());
    missing_policy["output"]["fields"]
        .as_object_mut()
        .unwrap()
        .remove("deployment_required");
    refuses_without_persistence(client, &c, missing_policy).await;
    let mut mismatch = local.clone();
    mismatch["request_id"] = json!(Uuid::new_v4());
    mismatch["consumed_outputs"][0]["digest"] = json!("b".repeat(64));
    refuses_with_code_without_persistence(client, &c, mismatch, "DEPENDENCY_STALE").await;
    let mut fake_handoff = completion(&c, "handoff_required", "completed", "continue", None, None);
    fake_handoff["output"]["fields"]["route"] = json!("result_local_only");
    refuses_without_persistence(client, &c, fake_handoff).await;
    let run = support::id(&c.run()["id"]);
    for phase in [
        "slice-plan-builder",
        "slice-execution-runner",
        "slice-verification-runner",
        "slice-validation-deployment-contract-shaper",
    ] {
        sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=true,stale_reason='isolated local gate stale fixture' WHERE run_id=$1 AND phase_id=$2").bind(run).bind(phase).execute(pool).await.unwrap();
        let stale = current(client, &c).await;
        // Current consumed list now excludes the stale tuple: semantic guard must still refuse.
        let r = completion(
            &stale,
            "completed_local_verified",
            "completed",
            "continue",
            None,
            None,
        );
        refuses_without_persistence(client, &stale, r).await;
        sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=false,stale_reason=NULL WHERE run_id=$1 AND phase_id=$2").bind(run).bind(phase).execute(pool).await.unwrap();
    }
    c = current(client, &c).await;
    for (phase, key, value) in [
        ("slice-plan-builder", "task_count", "99"),
        (
            "slice-verification-runner",
            "verification_complete",
            "false",
        ),
        ("slice-verification-runner", "failed_check_count", "1"),
        (
            "slice-validation-deployment-contract-shaper",
            "deployment_required",
            "true",
        ),
        ("slice-execution-runner", "completed_task_count", "0"),
    ] {
        let output:Value=sqlx::query_scalar("SELECT o.fields FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id=$2").bind(run).bind(phase).fetch_one(pool).await.unwrap();
        // Change both persisted output and immutable fixture attempt together to exercise policy predicates.
        let previous_payload:Value=sqlx::query_scalar("SELECT a.request_payload FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id JOIN slice_pipeline_phase_attempts a ON a.id=o.attempt_id WHERE b.run_id=$1 AND b.phase_id=$2").bind(run).bind(phase).fetch_one(pool).await.unwrap();
        let mut changed = output.clone();
        changed[key] = json!(value);
        let mut payload = previous_payload.clone();
        payload["output"]["fields"] = changed.clone();
        sqlx::query("UPDATE slice_pipeline_phase_outputs SET fields=$3 WHERE id=(SELECT output_id FROM slice_pipeline_output_bindings WHERE run_id=$1 AND phase_id=$2)").bind(run).bind(phase).bind(changed).execute(pool).await.unwrap();
        sqlx::query("UPDATE slice_pipeline_phase_attempts SET request_payload=$3 WHERE id=(SELECT o.attempt_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id=$2)").bind(run).bind(phase).bind(payload).execute(pool).await.unwrap();
        let changed_context = current(client, &c).await;
        refuses_without_persistence(
            client,
            &changed_context,
            completion(
                &changed_context,
                "completed_local_verified",
                "completed",
                "continue",
                None,
                None,
            ),
        )
        .await;
        sqlx::query("UPDATE slice_pipeline_phase_outputs SET fields=$3 WHERE id=(SELECT output_id FROM slice_pipeline_output_bindings WHERE run_id=$1 AND phase_id=$2)").bind(run).bind(phase).bind(output).execute(pool).await.unwrap();
        sqlx::query("UPDATE slice_pipeline_phase_attempts SET request_payload=$3 WHERE id=(SELECT o.attempt_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id=$2)").bind(run).bind(phase).bind(previous_payload).execute(pool).await.unwrap();
    }
    let previous_payload: Value = sqlx::query_scalar("SELECT a.request_payload FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id JOIN slice_pipeline_phase_attempts a ON a.id=o.attempt_id WHERE b.run_id=$1 AND b.phase_id='slice-validation-deployment-contract-shaper'").bind(run).fetch_one(pool).await.unwrap();
    let mut old_proof = previous_payload.clone();
    let proof = old_proof["consumed_outputs"]
        .as_array_mut()
        .unwrap()
        .iter_mut()
        .find(|v| v["phase_id"] == "slice-verification-runner")
        .unwrap();
    proof["digest"] = json!("f".repeat(64));
    sqlx::query("UPDATE slice_pipeline_phase_attempts SET request_payload=$2 WHERE id=(SELECT o.attempt_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id='slice-validation-deployment-contract-shaper')").bind(run).bind(old_proof).execute(pool).await.unwrap();
    c = current(client, &c).await;
    refuses_without_persistence(
        client,
        &c,
        completion(
            &c,
            "completed_local_verified",
            "completed",
            "continue",
            None,
            None,
        ),
    )
    .await;
    sqlx::query("UPDATE slice_pipeline_phase_attempts SET request_payload=$2 WHERE id=(SELECT o.attempt_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id='slice-validation-deployment-contract-shaper')").bind(run).bind(previous_payload).execute(pool).await.unwrap();
    sqlx::query("UPDATE slice_pipeline_phase_attempts SET outcome='waiting_input' WHERE id=(SELECT o.attempt_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id='slice-validation-deployment-contract-shaper')").bind(run).execute(pool).await.unwrap();
    c = current(client, &c).await;
    refuses_without_persistence(
        client,
        &c,
        completion(
            &c,
            "completed_local_verified",
            "completed",
            "continue",
            None,
            None,
        ),
    )
    .await;
    sqlx::query("UPDATE slice_pipeline_phase_attempts SET outcome='completed' WHERE id=(SELECT o.attempt_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.id=b.output_id WHERE b.run_id=$1 AND b.phase_id='slice-validation-deployment-contract-shaper')").bind(run).execute(pool).await.unwrap();
    c = current(client, &c).await;
    let raw_local = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &c,
            "completed_local_verified",
            "completed",
            "continue",
            None,
            None,
        ),
    )
    .await;
    c = resolve_pipeline(client, raw_local)
        .await
        .expect("resolve actual local verified completion");
    assert_eq!(c.run()["current_phase_ordinal"], 17);
    let raw_promotion = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&c, "not_required", "completed", "continue", None, None),
    )
    .await;
    c = resolve_pipeline(client, raw_promotion)
        .await
        .expect("resolve actual promotion completion");
    assert_eq!(c.run()["current_phase_ordinal"], 18);
    c
}
