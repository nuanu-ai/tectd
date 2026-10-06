use super::*;

#[test]
fn local_result_pair_requires_current_successful_consumed_proof_and_no_deployment() {
    let mut request = waiting_request(None);
    request.phase_id = "slice-deployment-or-handoff-gate".into();
    request
        .output
        .fields
        .insert("deployment_required".into(), "false".into());
    let binding = |phase: &str, revision| PipelineConsumedOutput {
        phase_id: phase.into(),
        output_revision: revision,
        digest: "a".repeat(64),
    };
    let mut verified = request.clone();
    verified.phase_id = "slice-verification-runner".into();
    verified.outcome = PipelinePhaseOutcome::Completed;
    verified.output.verdict = Some("completed_local_verified".into());
    verified.output.dispositions = vec!["proof_gate".into()];
    for (k, v) in [
        ("verification_complete", "true"),
        ("failed_check_count", "0"),
        ("focused_exit_code", "0"),
        ("affected_exit_code", "0"),
        ("focused_command", "cargo test focused"),
        ("affected_command", "cargo test affected"),
    ] {
        verified.output.fields.insert(k.into(), v.into());
    }
    // Legal rework need not give different phases the same revision number.
    let verification = (binding("slice-verification-runner", 7), verified);
    let mut shaped = request.clone();
    shaped.phase_id = "slice-validation-deployment-contract-shaper".into();
    shaped.outcome = PipelinePhaseOutcome::Completed;
    shaped.output.verdict = Some("deployment_not_required".into());
    shaped.output.dispositions = vec!["deployment_not_required".into()];
    shaped.consumed_outputs = vec![verification.0.clone()];
    let policy = (
        binding("slice-validation-deployment-contract-shaper", 2),
        shaped,
    );
    request.consumed_outputs = vec![verification.0.clone(), policy.0.clone()];
    assert!(validate_local_result_pair(&request, &verification, &policy).is_ok());
    for (key, value) in [
        ("verification_complete", "false"),
        ("failed_check_count", "1"),
        ("focused_exit_code", "1"),
        ("affected_exit_code", "2"),
        ("focused_command", "not_run"),
        ("affected_command", ""),
    ] {
        let mut bad = verification.clone();
        bad.1.output.fields.insert(key.into(), value.into());
        assert!(
            validate_local_result_pair(&request, &bad, &policy).is_err(),
            "{key}"
        );
    }
    let mut required = policy.clone();
    required
        .1
        .output
        .fields
        .insert("deployment_required".into(), "true".into());
    assert!(validate_local_result_pair(&request, &verification, &required).is_err());
    let mut stale = policy.clone();
    stale.1.consumed_outputs[0].output_revision -= 1;
    assert!(validate_local_result_pair(&request, &verification, &stale).is_err());
    let mut unmatched = request.clone();
    unmatched.consumed_outputs[0].digest = "b".repeat(64);
    assert!(validate_local_result_pair(&unmatched, &verification, &policy).is_err());
    let mut blocked = verification.clone();
    blocked.1.outcome = PipelinePhaseOutcome::Blocked;
    assert!(validate_local_result_pair(&request, &blocked, &policy).is_err());
    let mut revisit = policy.clone();
    revisit.1.revisit_phase_id = Some("slice-verification-runner".into());
    assert!(validate_local_result_pair(&request, &verification, &revisit).is_err());
}

#[test]
fn local_execution_must_complete_the_exact_current_planned_tasks() {
    let reference = |phase: &str, revision| PipelineConsumedOutput {
        phase_id: phase.into(),
        output_revision: revision,
        digest: "a".repeat(64),
    };
    let mut planned = waiting_request(None);
    planned.outcome = PipelinePhaseOutcome::Completed;
    planned.output.verdict = Some("planned".into());
    planned.output.dispositions = vec!["plan_gate".into()];
    planned
        .output
        .fields
        .insert("task_count".into(), "3".into());
    let plan = (reference("slice-plan-builder", 2), planned);
    let mut implemented = plan.1.clone();
    implemented.output.verdict = Some("implemented_locally".into());
    implemented.output.dispositions = vec!["execution_gate".into()];
    implemented.consumed_outputs = vec![plan.0.clone()];
    for (key, value) in [
        ("completed_task_count", "3"),
        ("blocked_task_count", "0"),
        ("reviews_complete", "true"),
        ("no_hidden_lifecycle", "true"),
    ] {
        implemented.output.fields.insert(key.into(), value.into());
    }
    let execution = (reference("slice-execution-runner", 7), implemented);
    let mut verified = waiting_request(None);
    verified.consumed_outputs = vec![execution.0.clone()];
    let verification = (reference("slice-verification-runner", 4), verified);
    assert!(validate_local_execution(&plan, &execution, &verification).is_ok());
    let mut partial = execution.clone();
    partial
        .1
        .output
        .fields
        .insert("task_count".into(), "2".into());
    partial
        .1
        .output
        .fields
        .insert("completed_task_count".into(), "2".into());
    assert!(validate_local_execution(&plan, &partial, &verification).is_err());
    let mut old = verification.clone();
    old.1.consumed_outputs[0].output_revision -= 1;
    assert!(validate_local_execution(&plan, &execution, &old).is_err());
}
