use super::*;

pub(super) fn validate_completion_tail(
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    if let Some(verdict) = &request.output.verdict {
        let route = phase
            .verdict_routes
            .iter()
            .find(|route| {
                &route.verdict == verdict
                    && route.outcome == request.outcome
                    && route.transition == request.transition
                    && match &request.revisit_phase_id {
                        Some(id) => route.revisit_to.contains(id),
                        None => route.revisit_to.is_empty(),
                    }
            })
            .ok_or_else(|| {
                completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-11",
                    "arguments.params.output.verdict",
                    "verdict route matching outcome, transition and revisit phase",
                    "no matching route",
                    "align_completion_with_verdict_route",
                    "valid_verdict_route",
                )
            })?;
        let actual = request.output.dispositions.iter().collect::<BTreeSet<_>>();
        let expected = route.dispositions.iter().collect::<BTreeSet<_>>();
        if actual != expected {
            return Err(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-COMPLETE-OUTPUT-12",
                "arguments.params.output.dispositions",
                "exact disposition set of matched verdict route",
                "set mismatch",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
    }
    validate_followup_proposal(
        definition,
        phase,
        &request.output,
        request.outcome,
        request.transition,
        &request.consumed_outputs,
    )?;
    let reads = request
        .output
        .skill_reads
        .iter()
        .map(|read| (&read.instruction_id, &read.version, &read.digest))
        .collect::<BTreeSet<_>>();
    let expected_reads = phase
        .skills
        .iter()
        .map(|skill| (&skill.id, &skill.version, &skill.digest))
        .collect::<BTreeSet<_>>();
    if reads != expected_reads || reads.len() != request.output.skill_reads.len() {
        let expected_values = expected_reads
            .iter()
            .map(|(id, version, digest)| (id.as_str(), version.as_str(), digest.as_str()))
            .collect::<Vec<_>>();
        let actual_values = request
            .output
            .skill_reads
            .iter()
            .map(|read| {
                (
                    read.instruction_id.as_str(),
                    read.version.as_str(),
                    read.digest.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let diff = FullReceiptDiff::between(&expected_values, &actual_values);
        return Err(phase_read_receipt_refusal(
            "skill",
            "WP6-SKILL-READ-01",
            "arguments.params.output.skill_reads",
            &diff,
        )?);
    }
    let resource_reads = request
        .output
        .resource_reads
        .iter()
        .map(|read| (&read.instruction_id, &read.version, &read.digest))
        .collect::<BTreeSet<_>>();
    let expected_resource_reads = phase
        .resources
        .iter()
        .map(|resource| (&resource.id, &resource.version, &resource.digest))
        .collect::<BTreeSet<_>>();
    if resource_reads != expected_resource_reads
        || resource_reads.len() != request.output.resource_reads.len()
    {
        let expected_values = expected_resource_reads
            .iter()
            .map(|(id, version, digest)| (id.as_str(), version.as_str(), digest.as_str()))
            .collect::<Vec<_>>();
        let actual_values = request
            .output
            .resource_reads
            .iter()
            .map(|read| {
                (
                    read.instruction_id.as_str(),
                    read.version.as_str(),
                    read.digest.as_str(),
                )
            })
            .collect::<Vec<_>>();
        let diff = FullReceiptDiff::between(&expected_values, &actual_values);
        return Err(phase_read_receipt_refusal(
            "resource",
            "WP6-RESOURCE-READ-01",
            "arguments.params.output.resource_reads",
            &diff,
        )?);
    }
    match request.transition {
        PipelineTransition::Continue => {
            if request.terminal_result.is_some() || request.escalation_target.is_some() {
                return Err(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-14",
                    if request.terminal_result.is_some() {
                        "arguments.params.terminal_result"
                    } else {
                        "arguments.params.escalation_target"
                    },
                    "continue has no terminal result or escalation target",
                    format!(
                        "terminal_present={}; escalation_present={}; publish={}; outcome={:?}; transition={:?}",
                        request.terminal_result.is_some(),
                        request.escalation_target.is_some(),
                        request.publish_blocked_result,
                        request.outcome,
                        request.transition
                    ),
                    "align_transition_result_fields",
                    "valid_transition_result",
                ));
            }
        }
        PipelineTransition::Complete => {
            if request.terminal_result.is_none() || request.escalation_target.is_some() {
                return Err(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-15",
                    if request.terminal_result.is_none() {
                        "arguments.params.terminal_result"
                    } else {
                        "arguments.params.escalation_target"
                    },
                    "complete has terminal result and no escalation target",
                    format!(
                        "terminal_present={}; escalation_present={}; publish={}; outcome={:?}; transition={:?}",
                        request.terminal_result.is_some(),
                        request.escalation_target.is_some(),
                        request.publish_blocked_result,
                        request.outcome,
                        request.transition
                    ),
                    "align_transition_result_fields",
                    "valid_transition_result",
                ));
            }
        }
        PipelineTransition::Block => {
            if request.escalation_target.is_some()
                || request.publish_blocked_result != request.terminal_result.is_some()
            {
                return Err(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-16",
                    if request.escalation_target.is_some() {
                        "arguments.params.escalation_target"
                    } else {
                        "arguments.params.terminal_result"
                    },
                    "block has no escalation target and terminal presence equals publication flag",
                    format!(
                        "terminal_present={}; escalation_present={}; publish={}; outcome={:?}; transition={:?}",
                        request.terminal_result.is_some(),
                        request.escalation_target.is_some(),
                        request.publish_blocked_result,
                        request.outcome,
                        request.transition
                    ),
                    "align_transition_result_fields",
                    "valid_transition_result",
                ));
            }
        }
        PipelineTransition::Escalate => {
            if request.terminal_result.is_none() || request.escalation_target.is_none() {
                return Err(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-COMPLETE-OUTPUT-17",
                    if request.terminal_result.is_none() {
                        "arguments.params.terminal_result"
                    } else {
                        "arguments.params.escalation_target"
                    },
                    "escalate has terminal result and escalation target",
                    format!(
                        "terminal_present={}; escalation_present={}; publish={}; outcome={:?}; transition={:?}",
                        request.terminal_result.is_some(),
                        request.escalation_target.is_some(),
                        request.publish_blocked_result,
                        request.outcome,
                        request.transition
                    ),
                    "align_transition_result_fields",
                    "valid_transition_result",
                ));
            }
        }
    }
    if request.publish_blocked_result
        && (request.outcome != PipelinePhaseOutcome::Blocked
            || request.transition != PipelineTransition::Block)
    {
        return Err(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-18",
            "arguments.params.publish_blocked_result",
            "publication requires blocked outcome and block transition",
            format!(
                "terminal_present={}; escalation_present={}; publish={}; outcome={:?}; transition={:?}",
                request.terminal_result.is_some(),
                request.escalation_target.is_some(),
                request.publish_blocked_result,
                request.outcome,
                request.transition
            ),
            "align_transition_result_fields",
            "valid_transition_result",
        ));
    }
    if let Some(result) = &request.terminal_result {
        validate_terminal(result)?;
    }
    Ok(())
}
