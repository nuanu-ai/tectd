use crate::responses;
use crate::scope_guidance::StaticCandidateGuidance;
use crate::slice_guidance::StaticSliceGuidance;
use crate::slice_tools::SliceInvocation;
use serde::Serialize;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::{NativePlanningOutputGuard, WorkspaceService};
use tect_domain::{
    OpenScopeOutcome, OpenSliceOutcome, RecordSliceResultOutcome, RequestContext, Result,
    SliceCandidateContext, SliceCandidateContextQuery, SliceCandidateContextView,
    SliceCandidateNode, SliceCandidateSetStatus, SliceState, WorkspaceState,
};
use uuid::Uuid;

pub(crate) async fn state(
    context: &RequestContext,
    service: &WorkspaceService,
) -> Result<WorkspaceState> {
    let mut state = service.get_state(context).await?;
    let guidance = StaticSliceGuidance;
    for summary in &mut state.native_planning {
        let current = service
            .slice_candidate_context(
                context,
                &SliceCandidateContextQuery {
                    scope_id: summary.scope_id,
                    view: SliceCandidateContextView::Overview,
                    after: None,
                    limit: 1,
                },
                &guidance,
            )
            .await?;
        summary.stale = !current.stale_reasons.is_empty();
        if summary.stale {
            summary.eligible_work.clear();
        }
    }
    Ok(state)
}

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: SliceInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let guidance = StaticSliceGuidance;
    let guard = NativePlanningEncoding { capacity };
    match invocation {
        SliceInvocation::ScopeContext { scope_id } => {
            output(service.scope_context(context, scope_id).await?, vec![])
        }
        SliceInvocation::Pipelines => {
            service.authenticate_host(context).await?;
            output(crate::slice_pipeline_catalog::value(), vec![])
        }
        SliceInvocation::CandidateContext(query) => {
            let value = service
                .slice_candidate_context(context, &query, &guidance)
                .await?;
            candidate_page(value, &query, capacity)
        }
        SliceInvocation::OpenScope(request) => {
            let source_guidance = StaticCandidateGuidance;
            let value = service
                .scope_open(context, &request, &source_guidance, &guidance, &guard)
                .await?;
            let context = match &value {
                OpenScopeOutcome::Created(context) | OpenScopeOutcome::Replay(context) => {
                    context.planning.clone()
                }
            };
            output(value, candidate_actions(&context)?)
        }
        SliceInvocation::SaveDraft(request) => candidate_context(
            service
                .save_slice_candidate_draft(context, &request, &guidance, &guard)
                .await?,
        ),
        SliceInvocation::Review(request) => candidate_context(
            service
                .review_slice_candidate_set(context, &request, &guidance, &guard)
                .await?,
        ),
        SliceInvocation::RecordInput(request) => candidate_context(
            service
                .record_slice_candidate_input(context, &request)
                .await?,
        ),
        SliceInvocation::Refresh(request) => candidate_context(
            service
                .refresh_slice_candidate_set(context, &request, &guidance, &guard)
                .await?,
        ),
        SliceInvocation::OpenSlice(request) => {
            let value = service.slice_open(context, &request).await?;
            let slice = match &value {
                OpenSliceOutcome::Created(slice) | OpenSliceOutcome::Replay(slice) => slice,
            };
            let mut actions = vec![responses::action(
                "slice_context",
                json!({"slice_id":slice.id}),
            )?];
            if crate::pipeline_definitions::delivery_modes(slice.pipeline).is_some() {
                actions.insert(0, pipeline_begin_action(slice)?);
            }
            output(value, actions)
        }
        SliceInvocation::SliceContext { slice_id } => {
            let slice = service.slice_context(context, slice_id).await?;
            let actions = if let Some(run_id) = slice.pipeline_run_id {
                vec![responses::action(
                    "slice_pipeline_context",
                    json!({"run_id":run_id}),
                )?]
            } else if crate::pipeline_definitions::delivery_modes(slice.pipeline).is_some() {
                vec![pipeline_begin_action(&slice)?]
            } else {
                Vec::new()
            };
            output(slice, actions)
        }
        SliceInvocation::RecordResult(request) => {
            let value = service.slice_result_record(context, &request).await?;
            let planning = match &value {
                RecordSliceResultOutcome::Created { context, .. }
                | RecordSliceResultOutcome::Replay { context, .. } => context,
            };
            let actions = candidate_actions(planning)?;
            output(value, actions)
        }
    }
}

fn pipeline_begin_action(slice: &tect_domain::NativeSlice) -> Result<Value> {
    crate::api::needs_action(
        "needs_context",
        "slice_pipeline_begin",
        json!({"request_id":request_id(slice.id,slice.revision,"pipeline-begin"),
            "scope_id":slice.scope_id,"slice_id":slice.id,"slice_revision":slice.revision}),
        "context_input",
        json!({"fields":[{"path":"arguments.params.qualification_reason",
            "format":"Agent-supplied concrete reason this Slice fits the selected pipeline and its default delivery mode. Do not ask the human unless fit is genuinely ambiguous."}]}),
    )
}

fn candidate_context(value: SliceCandidateContext) -> Result<Value> {
    let actions = candidate_actions(&value)?;
    output(value, actions)
}

fn candidate_page(
    context: SliceCandidateContext,
    query: &SliceCandidateContextQuery,
    capacity: usize,
) -> Result<Value> {
    let actions = candidate_actions(&context)?;
    let offset = usize::try_from(query.after.unwrap_or(0))
        .map_err(|_| tect_domain::Error::InvalidArguments)?;
    let limit = query.limit as usize;
    let common = json!({
        "view":query.view,
        "scope":context.scope,
        "candidate_set":context.candidate_set,
        "snapshot":context.snapshot,
        "stale_reasons":context.stale_reasons,
        "planning_knowledge":context.planning_knowledge,
    });
    let value = match query.view {
        SliceCandidateContextView::Overview => common,
        SliceCandidateContextView::Inputs => page(common, context.inputs, offset, limit)?,
        SliceCandidateContextView::Candidates => page(
            common,
            context.draft.map(|draft| draft.nodes).unwrap_or_default(),
            offset,
            limit,
        )?,
        SliceCandidateContextView::Reviews => page(common, context.reviews, offset, limit)?,
        SliceCandidateContextView::History => page(common, context.history, offset, limit)?,
        SliceCandidateContextView::Results => page(common, context.results, offset, limit)?,
    };
    let recommended = (!actions.is_empty()).then_some(0);
    let value = responses::with_actions(value, actions, recommended);
    ensure_capacity(&value, capacity)?;
    Ok(value)
}

pub struct NativePlanningEncoding {
    pub capacity: usize,
}

impl NativePlanningOutputGuard for NativePlanningEncoding {
    fn check_context(&self, value: &SliceCandidateContext) -> Result<()> {
        ensure_capacity(&candidate_context(value.clone())?, self.capacity)
    }

    fn check_open_scope(&self, value: &OpenScopeOutcome) -> Result<()> {
        let planning = match value {
            OpenScopeOutcome::Created(context) | OpenScopeOutcome::Replay(context) => {
                &context.planning
            }
        };
        ensure_capacity(&output(value, candidate_actions(planning)?)?, self.capacity)
    }
}

fn ensure_capacity(value: &Value, capacity: usize) -> Result<()> {
    if capacity > crate::frame::MAX_FRAME_BYTES {
        return Err(tect_domain::Error::InvalidArguments);
    }
    if responses::encoded_len(value)? > capacity {
        return Err(tect_domain::Error::RequestTooLarge);
    }
    Ok(())
}

fn page<T: Serialize>(
    mut common: Value,
    items: Vec<T>,
    offset: usize,
    limit: usize,
) -> Result<Value> {
    let total = items.len();
    let values = items
        .into_iter()
        .skip(offset)
        .take(limit)
        .collect::<Vec<_>>();
    let returned = values.len();
    common["items"] =
        serde_json::to_value(values).map_err(|_| tect_domain::Error::TransportUnavailable)?;
    common["next_after"] = if offset + returned < total {
        json!(offset + returned)
    } else {
        Value::Null
    };
    Ok(common)
}

fn candidate_actions(context: &SliceCandidateContext) -> Result<Vec<Value>> {
    let mut actions = Vec::new();
    if !context.stale_reasons.is_empty() {
        let mut params = json!({
            "scope_id":context.scope.id,
            "candidate_set_id":context.candidate_set.id,
            "revision":context.candidate_set.revision,
            "request_id":request_id(context.candidate_set.id, context.candidate_set.revision, "refresh")
        });
        if let Some(manifest) = context.planning_knowledge.as_ref().and_then(|v|v.manifest.as_ref()) {
            params["task_context"] = json!(manifest.task_context);
        }
        actions.push(responses::action(
            "refresh_slice_candidate_set",
            params,
        )?);
    } else if matches!(context.candidate_set.status, SliceCandidateSetStatus::Draft) {
        actions.push(save_action(context, "draft", "Complete schema-valid Slice-candidate graph covering the whole Scope." )?);
    } else if matches!(
        context.candidate_set.status,
        SliceCandidateSetStatus::ReviewRequired
    ) {
        actions.push(save_action(
            context,
            "review",
            "Critical review verdict, summary, and findings for the complete current graph.",
        )?);
        actions.push(save_action(
            context,
            "draft",
            "Revised complete Slice-candidate graph addressing the review findings.",
        )?);
    } else if matches!(context.candidate_set.status, SliceCandidateSetStatus::Ready)
        && let Some(SliceCandidateNode::Work { id, revision, .. }) =
            context.draft.as_ref().and_then(|draft| {
                draft.nodes.iter().find(|node| {
                    matches!(node, SliceCandidateNode::Work { id, dependencies, .. }
                        if dependencies.iter().all(|dependency| context.slices.iter().any(|slice|
                            slice.candidate_id == *dependency && slice.state == SliceState::Completed))
                            && !context.slices.iter().any(|slice| slice.candidate_id == *id))
                })
            })
    {
        actions.push(responses::action(
            "slice_open",
            json!({"request_id":request_id(*id, *revision, "open"),"scope_id":context.scope.id,
                "scope_revision":context.scope.revision,"candidate_set_id":context.candidate_set.id,
                "candidate_set_revision":context.candidate_set.revision,
                "candidate_snapshot_id":context.snapshot.id,"candidate_id":id,
                "candidate_revision":revision}),
        )?);
    } else if matches!(
        context.candidate_set.status,
        SliceCandidateSetStatus::Ready | SliceCandidateSetStatus::Blocked
    ) {
        actions.push(record_input_action(context)?);
    }
    actions.push(responses::action(
        "slice_candidate_context",
        json!({"scope_id":context.scope.id,"view":"overview","limit":25}),
    )?);
    Ok(actions)
}

fn save_action(context: &SliceCandidateContext, kind: &str, format: &str) -> Result<Value> {
    let mut params = json!({"kind":kind,"scope_id":context.scope.id,
        "candidate_set_id":context.candidate_set.id,
        "revision":context.candidate_set.revision,"snapshot_id":context.snapshot.id,
        "input_cursor":context.candidate_set.latest_input,
        "request_id":request_id(context.candidate_set.id, context.candidate_set.revision, kind)});
    if let Some(manifest) = context
        .planning_knowledge
        .as_ref()
        .and_then(|v| v.manifest.as_ref())
    {
        params["consumed_knowledge"] = json!({"manifest_id":manifest.id,"digest":manifest.digest,"workspace_generation":manifest.workspace_generation});
    }
    crate::api::needs_action(
        "needs_input",
        "save_slice_candidate_set",
        params,
        "input",
        json!({"fields":[{"path":format!("arguments.params.{kind}"),"format":format}]}),
    )
}

fn record_input_action(context: &SliceCandidateContext) -> Result<Value> {
    crate::api::needs_action(
        "needs_input",
        "record_slice_candidate_input",
        json!({"scope_id":context.scope.id,"candidate_set_id":context.candidate_set.id,
            "revision":context.candidate_set.revision,
            "request_id":request_id(context.candidate_set.id, context.candidate_set.revision, "input")}),
        "input",
        json!({"fields":[{"path":"arguments.params.input","format":"Complete exact planning amendment without paraphrasing."}]}),
    )
}

fn request_id(id: Uuid, revision: i64, operation: &str) -> Uuid {
    let digest =
        Sha256::digest(format!("tectd-slice-planning:{id}:{revision}:{operation}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

fn output<T: Serialize>(value: T, actions: Vec<Value>) -> Result<Value> {
    let value =
        serde_json::to_value(value).map_err(|_| tect_domain::Error::TransportUnavailable)?;
    let recommended = (!actions.is_empty()).then_some(0);
    Ok(responses::with_actions(value, actions, recommended))
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{NativeSlice, PipelineKind, SliceState};

    #[test]
    fn opened_slice_output_is_not_started_without_design_guidance_or_execution_claim() {
        let slice = NativeSlice {
            id: Uuid::new_v4(),
            scope_id: Uuid::new_v4(),
            revision: 1,
            candidate_id: Uuid::new_v4(),
            candidate_revision: 1,
            opening_snapshot_id: Uuid::new_v4(),
            title: "Bounded outcome".into(),
            outcome: "Observed result".into(),
            pipeline: PipelineKind::LightweightTddDevelopment,
            state: SliceState::Open,
            pipeline_status: "not_started".into(),
            pipeline_run_id: None,
            knowledge_change_id: None,
            knowledge_run_id: None,
            knowledge_status: None,
            execution_claimed: false,
        };
        let value = output(slice, Vec::new()).unwrap();
        assert_eq!(value["pipeline_status"], "not_started");
        assert_eq!(value["execution_claimed"], false);
        assert!(value.get("rules").is_none());
        assert!(value.get("method").is_none());
        assert!(value.get("execute").is_none());
    }
}
