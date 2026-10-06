use super::*;

pub(super) fn scope_read(
    scope: tect_domain::NativeScope,
    params: Value,
    window: &crate::planning_read::Window,
    capacity: usize,
) -> Result<Value> {
    crate::json_fragment::encode(
        &scope,
        vec![],
        capacity,
        window.borrowed(),
        json!({"scope_id":scope.id,"scope_revision":scope.revision}),
        "scope_context",
        params,
    )
}
pub(super) fn candidate_page_read(
    context: SliceCandidateContext,
    query: &SliceCandidateContextQuery,
    params: Value,
    window: &crate::planning_read::Window,
    capacity: usize,
) -> Result<Value> {
    let source = json!({"scope_id":context.scope.id,"candidate_set_id":context.candidate_set.id,"snapshot_id":context.snapshot.id});
    // Details is the complete authorized aggregate; its representation pin also
    // covers inputs, history and knowledge that can change without a draft revision.
    let mut value = candidate_page(context, query, usize::MAX)?;
    let actions = value
        .as_object_mut()
        .unwrap()
        .remove("actions")
        .and_then(|value| value.as_array().cloned())
        .unwrap_or_default();
    value.as_object_mut().unwrap().remove("recommended_action");
    let full = responses::with_actions(value.clone(), actions.clone(), Some(0));
    if window.offset_bytes.is_none()
        && window.limit_bytes.is_none()
        && window.representation_digest.is_none()
        && responses::encoded_len(&full)? <= capacity.min(crate::json_fragment::READ_BUDGET)
    {
        return Ok(full);
    }
    let terminal = if let Some(after) = value["next_after"].as_i64() {
        let mut next = params.clone();
        next["after"] = json!(after);
        for key in ["offset_bytes", "limit_bytes", "representation_digest"] {
            next.as_object_mut().unwrap().remove(key);
        }
        vec![crate::api::ready_action("slice_candidate_context", next)?]
    } else {
        vec![]
    };
    crate::json_fragment::encode(
        &value,
        terminal,
        capacity,
        window.borrowed(),
        source,
        "slice_candidate_context",
        params,
    )
}

pub(super) fn compact_slice_planning(context: &SliceCandidateContext) -> Result<Value> {
    let details = crate::api::ready_action(
        "slice_candidate_context",
        json!({"scope_id":context.scope.id,"view":"details","limit":25}),
    )?;
    Ok(responses::with_actions(
        json!({"scope_id":context.scope.id,"candidate_set":context.candidate_set,"snapshot":{"id":context.snapshot.id,"sequence":context.snapshot.sequence},"counts":{"inputs":context.inputs.len(),"reviews":context.reviews.len(),"history":context.history.len(),"slices":context.slices.len(),"results":context.results.len()},"field_destinations":{"complete_planning_context":details}}),
        vec![crate::api::ready_action(
            "slice_candidate_context",
            json!({"scope_id":context.scope.id,"view":"details","limit":25}),
        )?],
        Some(0),
    ))
}
pub(super) fn compact_open_scope(outcome: &OpenScopeOutcome) -> Result<Value> {
    let (disposition, context) = match outcome {
        OpenScopeOutcome::Created(context) => ("created", context),
        OpenScopeOutcome::Replay(context) => ("replay", context),
    };
    let scope = crate::api::ready_action("scope_context", json!({"scope_id":context.scope.id}))?;
    let planning = crate::api::ready_action(
        "slice_candidate_context",
        json!({"scope_id":context.scope.id,"view":"details","limit":25}),
    )?;
    Ok(responses::with_actions(
        json!({"disposition":disposition,"scope":{"id":context.scope.id,"revision":context.scope.revision,"source_candidate_set_id":context.scope.source_candidate_set_id,"source_candidate_id":context.scope.source_candidate_id,"slice_candidate_set_id":context.scope.slice_candidate_set_id},"planning":{"candidate_set":context.planning.candidate_set,"snapshot_id":context.planning.snapshot.id},"field_destinations":{"scope":scope,"planning":planning}}),
        vec![
            crate::api::ready_action("scope_context", json!({"scope_id":context.scope.id}))?,
            crate::api::ready_action(
                "slice_candidate_context",
                json!({"scope_id":context.scope.id,"view":"details","limit":25}),
            )?,
        ],
        Some(0),
    ))
}

/// The catalogue's logical view is stable across all byte windows.
pub(crate) fn pipelines_read(
    view: PipelineView,
    window: &crate::planning_read::Window,
    capacity: usize,
) -> Result<Value> {
    let (view, value) = match view {
        PipelineView::Full => ("full", crate::slice_pipeline_catalog::value()),
        PipelineView::Summary => ("summary", crate::slice_pipeline_catalog::summary_value()),
    };
    crate::json_fragment::encode(
        &value,
        vec![],
        capacity,
        window.borrowed(),
        json!({"tool":"query","route":"slice.pipelines","view":view}),
        "slice_pipelines",
        json!({"view":view}),
    )
}
