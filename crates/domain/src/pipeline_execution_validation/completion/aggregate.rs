use super::*;

pub(super) fn first_aggregate_refusal(
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Option<Error> {
    if !phase.allowed_verdicts.is_empty()
        && request
            .output
            .verdict
            .as_ref()
            .is_none_or(|v| !phase.allowed_verdicts.contains(v))
    {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-05",
            "arguments.params.output.verdict",
            "allowed phase verdict",
            format!("present={}", request.output.verdict.is_some()),
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    if (!definition.version.starts_with("0.7")
        || phase.verdict_routes.is_empty()
        || request.outcome == PipelinePhaseOutcome::Completed)
        && phase
            .required_dispositions
            .iter()
            .any(|required| !request.output.dispositions.contains(required))
    {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-06",
            "arguments.params.output.dispositions",
            "every required disposition",
            format!(
                "required={}; supplied={}",
                phase.required_dispositions.len(),
                request.output.dispositions.len()
            ),
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    if phase.disposition_required && request.output.dispositions.is_empty() {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-07",
            "arguments.params.output.dispositions",
            "at least one required disposition",
            "count=0",
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    if !phase.allowed_dispositions.is_empty()
        && request
            .output
            .dispositions
            .iter()
            .any(|value| !phase.allowed_dispositions.contains(value))
    {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-08",
            "arguments.params.output.dispositions",
            "only allowed phase dispositions",
            format!(
                "disallowed_count={}",
                request
                    .output
                    .dispositions
                    .iter()
                    .filter(|value| !phase.allowed_dispositions.contains(value))
                    .count()
            ),
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    if !phase.allowed_dispositions.is_empty()
        && phase.required_dispositions.is_empty()
        && phase.verdict_routes.is_empty()
        && phase.disposition_required
        && request.output.dispositions.len() != 1
    {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-09",
            "arguments.params.output.dispositions",
            "exactly one disposition",
            request.output.dispositions.len().to_string(),
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    if request
        .output
        .dispositions
        .iter()
        .collect::<BTreeSet<_>>()
        .len()
        != request.output.dispositions.len()
    {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-COMPLETE-OUTPUT-10",
            "arguments.params.output.dispositions",
            "unique dispositions",
            format!(
                "submitted={}; unique={}",
                request.output.dispositions.len(),
                request
                    .output
                    .dispositions
                    .iter()
                    .collect::<BTreeSet<_>>()
                    .len()
            ),
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    if let Some(v) = &request.output.reviewer_context {
        if v.reviewer_identity.trim().is_empty() {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-CONTEXT-01",
                "arguments.params.output.reviewer_context.reviewer_identity",
                "non-empty reviewer identity",
                "empty",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.reviewer_identity.len() > MAX_PIPELINE_CONTEXT_ID_BYTES {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-CONTEXT-02",
                "arguments.params.output.reviewer_context.reviewer_identity",
                "reviewer identity within the byte limit",
                v.reviewer_identity.len().to_string(),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.reviewer_context_id.trim().is_empty() {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-CONTEXT-03",
                "arguments.params.output.reviewer_context.reviewer_context_id",
                "non-empty reviewer context label",
                "empty",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.reviewer_context_id.len() > MAX_PIPELINE_CONTEXT_ID_BYTES {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-CONTEXT-04",
                "arguments.params.output.reviewer_context.reviewer_context_id",
                "reviewer context within the byte limit",
                v.reviewer_context_id.len().to_string(),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.reviewer_context_id != request.output.producer_context_id {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-CONTEXT-05",
                "arguments.params.output.reviewer_context.reviewer_context_id",
                "same label as output producer_context_id",
                "labels_equal=false",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.producer_context_ids.is_empty() {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-PRODUCERS-01",
                "arguments.params.output.reviewer_context.producer_context_ids",
                "at least one producer label",
                "count=0",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.producer_context_ids.len() > 100 {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-PRODUCERS-02",
                "arguments.params.output.reviewer_context.producer_context_ids",
                "at most 100 producer labels",
                v.producer_context_ids.len().to_string(),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        for (index, id) in v.producer_context_ids.iter().enumerate() {
            if id.trim().is_empty() {
                return Some(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-REVIEW-PRODUCERS-03",
                    format!(
                        "arguments.params.output.reviewer_context.producer_context_ids[{index}]"
                    ),
                    "non-empty producer label",
                    "empty",
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
            if id.len() > MAX_PIPELINE_CONTEXT_ID_BYTES {
                return Some(completion_refusal(
                    RefusalCode::InvalidOutput,
                    "WP6-REVIEW-PRODUCERS-04",
                    format!(
                        "arguments.params.output.reviewer_context.producer_context_ids[{index}]"
                    ),
                    "producer label within the byte limit",
                    id.len().to_string(),
                    "correct_phase_completion",
                    "valid_phase_completion",
                ));
            }
        }
        if v.producer_context_ids.contains(&v.reviewer_context_id) {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-PRODUCERS-05",
                "arguments.params.output.reviewer_context.producer_context_ids",
                "producer labels exclude reviewer label",
                "reviewer_included=true",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if v.producer_context_ids.iter().collect::<BTreeSet<_>>().len()
            != v.producer_context_ids.len()
        {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-PRODUCERS-06",
                "arguments.params.output.reviewer_context.producer_context_ids",
                "unique producer labels",
                format!(
                    "submitted={}; unique={}",
                    v.producer_context_ids.len(),
                    v.producer_context_ids.iter().collect::<BTreeSet<_>>().len()
                ),
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
        if !v.fresh_input {
            return Some(completion_refusal(
                RefusalCode::InvalidOutput,
                "WP6-REVIEW-CONTEXT-06",
                "arguments.params.output.reviewer_context.fresh_input",
                "fresh_input true",
                "fresh_input=false",
                "correct_phase_completion",
                "valid_phase_completion",
            ));
        }
    }
    if phase.fresh_reviewer_input && request.output.reviewer_context.is_none() {
        return Some(completion_refusal(
            RefusalCode::InvalidOutput,
            "WP6-REVIEW-CONTEXT-07",
            "arguments.params.output.reviewer_context",
            "fresh reviewer context required by current phase",
            "missing",
            "correct_phase_completion",
            "valid_phase_completion",
        ));
    }
    None
}
