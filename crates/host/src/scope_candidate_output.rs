use crate::responses::{encoded_len, with_actions};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_domain::{
    BeginCandidateSetOutcome, CandidateContext, CandidateContextPage, CandidateContextView,
    CandidateSetStatus, CandidateTextFragment, Error, Result, StoredCandidateContext,
};
use uuid::Uuid;

pub(crate) fn begin(outcome: BeginCandidateSetOutcome, capacity: usize) -> Result<Value> {
    let (disposition, context) = match outcome {
        BeginCandidateSetOutcome::Created(context) => ("created", context),
        BeginCandidateSetOutcome::Replay(context) => ("replay", context),
        BeginCandidateSetOutcome::Existing(context) => ("existing", context),
    };
    within(
        compact(&context, disposition, None)?,
        capacity.min(crate::json_fragment::READ_BUDGET),
    )
}

/// Refresh receipt: the exact Details read preserves the whole refreshed snapshot and draft.
pub(crate) fn stored(stored: StoredCandidateContext, capacity: usize) -> Result<Value> {
    within(
        compact(&stored.context, "refreshed", stored.draft.as_ref())?,
        capacity.min(crate::json_fragment::READ_BUDGET),
    )
}

/// Save, review and input replies: snapshot bodies came with the begin or context read.
pub(crate) fn stored_mutation(stored: StoredCandidateContext, capacity: usize) -> Result<Value> {
    within(
        compact(&stored.context, "saved", stored.draft.as_ref())?,
        capacity.min(crate::json_fragment::READ_BUDGET),
    )
}

pub(crate) fn page(mut page: CandidateContextPage, after: i64, capacity: usize) -> Result<Value> {
    let original_count = page.items.len();
    loop {
        if page.items.len() < original_count {
            page.next_after = Some(after + page.items.len() as i64);
        }
        let (actions, recommended) = page_actions(&page)?;
        let data = json!(&page);
        let value = with_actions(data, actions, recommended);
        if encoded_len(&value)? <= capacity {
            return Ok(value);
        }
        if page.items.len() <= 1 {
            return Err(Error::RequestTooLarge);
        }
        page.items.pop();
    }
}

pub(crate) fn fragment(
    candidate_set_id: Uuid,
    draft_revision: Option<i64>,
    fragment: CandidateTextFragment,
    capacity: usize,
) -> Result<Value> {
    let id = fragment.source_ref.id;
    let next = fragment.next_cursor;
    let historical = draft_revision.is_some();
    let continuation = |source_ref_id, cursor| {
        let mut params = json!({
            "candidate_set_id":candidate_set_id,"view":"fragment",
            "source_ref_id":source_ref_id,"cursor":cursor
        });
        if let Some(revision) = draft_revision {
            params["draft_revision"] = json!(revision);
        }
        crate::api::ready_action("candidate_context", params)
    };
    let actions = if let Some(cursor) = next {
        vec![continuation(id, cursor)?]
    } else if let Some(next_source) = fragment.next_source_ref_id {
        vec![continuation(next_source, 0)?]
    } else if historical {
        vec![current_context_action(candidate_set_id)?]
    } else {
        vec![crate::api::ready_action(
            "candidate_context",
            json!({"candidate_set_id":candidate_set_id,"view":"inputs","after":fragment.source_ref.input_sequence.unwrap_or(0),"limit":25}),
        )?]
    };
    within(
        with_actions(json!({"fragment":fragment}), actions, Some(0)),
        capacity,
    )
}

fn page_actions(page: &CandidateContextPage) -> Result<(Vec<Value>, Option<usize>)> {
    if let Some(after) = page.next_after {
        let action = if page.view == CandidateContextView::Historical {
            historical_action(
                &page.context,
                page.historical
                    .as_ref()
                    .ok_or(Error::InternalInvariant)?
                    .set_revision,
                Some(after),
            )?
        } else {
            read_action(&page.context, page.view, Some(after))?
        };
        return Ok((vec![action], Some(0)));
    }
    if page.view == CandidateContextView::History {
        let mut actions = page
            .items
            .iter()
            .filter_map(|item| match item {
                tect_domain::ScopeCandidatePageItem::History(entry) => Some(historical_action(
                    &page.context,
                    entry.latest_draft_revision,
                    None,
                )),
                _ => None,
            })
            .collect::<Result<Vec<_>>>()?;
        actions.push(current_context_action(page.context.candidate_set.id)?);
        return Ok((actions, Some(0)));
    }
    if page.view == CandidateContextView::Historical {
        let historical = page.historical.as_ref().ok_or(Error::InternalInvariant)?;
        let mut actions = historical
            .snapshot
            .source_refs
            .iter()
            .map(|source| {
                crate::api::ready_action(
                    "candidate_context",
                    json!({
                        "candidate_set_id":page.context.candidate_set.id,
                        "view":"fragment","draft_revision":historical.set_revision,
                        "source_ref_id":source.id,"cursor":0
                    }),
                )
            })
            .collect::<Result<Vec<_>>>()?;
        actions.push(current_context_action(page.context.candidate_set.id)?);
        return Ok((actions, Some(0)));
    }
    if !page.context.stale_reasons.is_empty() {
        return Ok((vec![refresh_action(&page.context)?], Some(0)));
    }
    match page.view {
        CandidateContextView::Details => terminal_actions(page),
        CandidateContextView::Overview => Ok((
            vec![read_action(
                &page.context,
                CandidateContextView::Program,
                None,
            )?],
            Some(0),
        )),
        CandidateContextView::Program => {
            let mut actions = page
                .program
                .as_ref()
                .into_iter()
                .flat_map(|program| &program.field_refs)
                .map(|source| {
                    crate::api::ready_action(
                        "candidate_context",
                        json!({"candidate_set_id":page.context.candidate_set.id,"view":"fragment","source_ref_id":source.id,"cursor":0}),
                    )
                })
                .collect::<Result<Vec<_>>>()?;
            actions.push(read_action(
                &page.context,
                CandidateContextView::Inputs,
                None,
            )?);
            Ok((actions, Some(0)))
        }
        CandidateContextView::Inputs => {
            let mut actions = page
                .items
                .iter()
                .filter_map(|item| match item {
                    tect_domain::ScopeCandidatePageItem::Input(input) => Some(
                        crate::api::ready_action(
                            "candidate_context",
                            json!({"candidate_set_id":page.context.candidate_set.id,"view":"fragment","source_ref_id":input.source_ref_id,"cursor":0}),
                        ),
                    ),
                    _ => None,
                })
                .collect::<Result<Vec<_>>>()?;
            actions.push(read_action(
                &page.context,
                CandidateContextView::Candidates,
                None,
            )?);
            Ok((actions, Some(0)))
        }
        CandidateContextView::Candidates => Ok((
            vec![read_action(
                &page.context,
                CandidateContextView::Reviews,
                None,
            )?],
            Some(0),
        )),
        CandidateContextView::Reviews => terminal_actions(page),
        CandidateContextView::History | CandidateContextView::Historical => {
            unreachable!("handled above")
        }
        CandidateContextView::Fragment => Err(Error::InternalInvariant),
    }
}

fn terminal_actions(page: &CandidateContextPage) -> Result<(Vec<Value>, Option<usize>)> {
    let context = &page.context;
    let set = &context.candidate_set;
    match set.status {
        CandidateSetStatus::Draft => Ok((vec![draft_action(context)?], Some(0))),
        CandidateSetStatus::ReviewRequired => Ok((
            vec![
                review_action(context, &page.required_protected_changes)?,
                draft_action(context)?,
            ],
            Some(0),
        )),
        CandidateSetStatus::Ready | CandidateSetStatus::Blocked => {
            Ok((vec![record_input_action(set.id, set.revision)?], None))
        }
    }
}

fn draft_action(context: &CandidateContext) -> Result<Value> {
    let set = &context.candidate_set;
    let mut params = json!({
        "kind":"draft","candidate_set_id":set.id,"revision":set.revision,
        "snapshot_id":set.current_snapshot_id,"input_cursor":set.latest_input,
        "request_id":request_id(set.id, set.revision, "save_draft")
    });
    add_knowledge_guard(&mut params, context);
    crate::api::needs_action(
        "needs_input",
        "save_candidate_set",
        params,
        "input",
        json!({"fields":[{"path":"arguments.params.draft","format":"Complete schema-valid candidate draft. Reuse backend IDs and revisions; include change_rationale only for changed candidates, never for new or unchanged ones, and an explicit supersession for every omitted ordinary candidate. Each goal is covered by exactly one candidate: list a goal only in the coverage_goals of the candidate its resolution names. Temporary local labels exist only inside this payload; durable UUIDs come from the backend reply."}]}),
    )
}

fn review_action(
    context: &CandidateContext,
    protected: &[tect_domain::ProtectedObjectRef],
) -> Result<Value> {
    let set = &context.candidate_set;
    let protected_change_reviews = protected
        .iter()
        .map(|required| {
            let mut value = json!({"accepted_evidence_id":required.accepted_evidence_id});
            if let Some(id) = required.prior_candidate_id {
                value["prior_candidate_id"] = json!(id);
            }
            value
        })
        .collect::<Vec<_>>();
    let mut params = json!({
        "kind":"review","candidate_set_id":set.id,"revision":set.revision,
        "snapshot_id":set.current_snapshot_id,"input_cursor":set.latest_input,
        "request_id":request_id(set.id, set.revision, "save_review"),
        "review":{"protected_change_reviews":protected_change_reviews}
    });
    add_knowledge_guard(&mut params, context);
    crate::api::needs_action(
        "needs_input",
        "save_candidate_set",
        params,
        "input",
        json!({"fields":[
            {"path":"arguments.params.review.verdict","format":"ready, revise, or blocked after critical semantic review"},
            {"path":"arguments.params.review.summary","format":"Substantive review against the captured method, rules, and source text"},
            {"path":"arguments.params.review.findings","format":"Material or advisory findings; may be empty when the review found none"},
            {"path":"arguments.params.review.candidate_decisions","format":"Exactly one decision for every backend candidate UUID"},
            {"path":"arguments.params.review.protected_change_reviews[].rationale","format":"Explicit review of every backend-filled accepted-work change reference"}
        ]}),
    )
}

fn refresh_action(context: &CandidateContext) -> Result<Value> {
    let set = &context.candidate_set;
    let mut params = json!({
        "candidate_set_id":set.id,"revision":set.revision,
        "request_id":request_id(set.id, set.revision, "refresh"),"program_revision":context.current_program_revision
    });
    if let Some(manifest) = context
        .planning_knowledge
        .as_ref()
        .and_then(|v| v.manifest.as_ref())
    {
        params["task_context"] = json!(manifest.task_context);
    }
    crate::api::ready_action("refresh_candidate_set", params)
}

fn add_knowledge_guard(params: &mut Value, context: &CandidateContext) {
    if let Some(manifest) = context
        .planning_knowledge
        .as_ref()
        .and_then(|v| v.manifest.as_ref())
    {
        params["consumed_knowledge"] = json!({"manifest_id":manifest.id,"digest":manifest.digest,"workspace_generation":manifest.workspace_generation});
    }
}

fn record_input_action(id: Uuid, revision: i64) -> Result<Value> {
    crate::api::needs_action(
        "needs_input",
        "record_candidate_input",
        json!({"candidate_set_id":id,"revision":revision,"request_id":request_id(id, revision, "record_input")}),
        "input",
        json!({"fields":[{"path":"arguments.params.input","format":"Complete original amendment text without trimming or paraphrasing."}]}),
    )
}

fn read_action(
    context: &CandidateContext,
    view: CandidateContextView,
    after: Option<i64>,
) -> Result<Value> {
    let mut params = json!({
        "candidate_set_id":context.candidate_set.id,"view":view,"limit":25
    });
    if let Some(after) = after {
        params["after"] = json!(after);
    }
    crate::api::ready_action("candidate_context", params)
}

fn historical_action(
    context: &CandidateContext,
    draft_revision: i64,
    after: Option<i64>,
) -> Result<Value> {
    let mut params = json!({
        "candidate_set_id":context.candidate_set.id,"view":"historical",
        "draft_revision":draft_revision,"limit":25
    });
    if let Some(after) = after {
        params["after"] = json!(after);
    }
    crate::api::ready_action("candidate_context", params)
}

fn current_context_action(candidate_set_id: Uuid) -> Result<Value> {
    crate::api::ready_action(
        "candidate_context",
        json!({"candidate_set_id":candidate_set_id,"view":"overview","limit":25}),
    )
}

fn within(value: Value, capacity: usize) -> Result<Value> {
    if encoded_len(&value)? <= capacity {
        Ok(value)
    } else {
        Err(Error::RequestTooLarge)
    }
}

fn request_id(id: Uuid, revision: i64, operation: &str) -> Uuid {
    let digest =
        Sha256::digest(format!("tectd-scope-candidate:{id}:{revision}:{operation}").as_bytes());
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

pub(crate) fn page_read(
    mut page: CandidateContextPage,
    mut params: Value,
    window: &crate::planning_read::Window,
    expected_revision: Option<i64>,
    capacity: usize,
) -> Result<Value> {
    crate::planning_read::revision(
        expected_revision,
        page.context.candidate_set.revision,
        "arguments.params.candidate_set_revision",
    )?;
    let after = params["after"].as_i64().unwrap_or(0);
    match self::page(page.clone(), after, crate::json_fragment::READ_BUDGET) {
        Ok(selected) => {
            let count = selected["items"].as_array().map_or(0, Vec::len);
            page.items.truncate(count);
            page.next_after = selected["next_after"].as_i64();
        }
        Err(Error::RequestTooLarge) => {
            if page.items.len() > 1 {
                page.items.truncate(1);
                page.next_after = Some(after + 1);
            }
        }
        Err(error) => return Err(error),
    }
    let (actions, _) = page_actions(&page)?;
    params["candidate_set_revision"] = json!(page.context.candidate_set.revision);
    let full = with_actions(json!(&page), actions.clone(), Some(0));
    if window.offset_bytes.is_none()
        && window.limit_bytes.is_none()
        && window.representation_digest.is_none()
        && encoded_len(&full)? <= capacity.min(crate::json_fragment::READ_BUDGET)
    {
        return Ok(full);
    }
    let terminal_actions = if page.next_after.is_some() {
        actions
    } else {
        vec![]
    };
    crate::json_fragment::encode(
        &page,
        terminal_actions,
        capacity,
        window.borrowed(),
        json!({"candidate_set_id":page.context.candidate_set.id,"candidate_set_revision":page.context.candidate_set.revision,"snapshot_id":page.context.candidate_set.current_snapshot_id}),
        "candidate_context",
        params,
    )
}

pub(crate) fn fragment_read(
    candidate_set_id: Uuid,
    draft_revision: Option<i64>,
    fragment: CandidateTextFragment,
    mut params: Value,
    window: &crate::planning_read::Window,
    capacity: usize,
) -> Result<Value> {
    let legacy = self::fragment(
        candidate_set_id,
        draft_revision,
        fragment.clone(),
        usize::MAX,
    )?;
    let actions = legacy["actions"].as_array().cloned().unwrap_or_default();
    for key in ["offset_bytes", "limit_bytes", "representation_digest"] {
        params.as_object_mut().unwrap().remove(key);
    }
    crate::json_fragment::encode(
        &json!({"fragment":fragment}),
        actions,
        capacity,
        window.borrowed(),
        json!({"candidate_set_id":candidate_set_id,"draft_revision":draft_revision,"source_ref_id":fragment.source_ref.id}),
        "candidate_context",
        params,
    )
}

fn compact(
    context: &CandidateContext,
    operation: &str,
    draft: Option<&tect_domain::ResolvedCandidateDraft>,
) -> Result<Value> {
    let overview = read_action(context, CandidateContextView::Overview, None)?;
    let candidates = read_action(context, CandidateContextView::Details, None)?;
    let program = read_action(context, CandidateContextView::Program, None)?;
    let reviews = read_action(context, CandidateContextView::Reviews, None)?;
    let next = if !context.stale_reasons.is_empty() {
        refresh_action(context)?
    } else if context.candidate_set.status == CandidateSetStatus::Draft {
        draft_action(context)?
    } else {
        read_action(context, CandidateContextView::Reviews, None)?
    };
    Ok(with_actions(
        json!({"operation":operation,"disposition":operation,"candidate_set":context.candidate_set,"snapshot":{"id":context.snapshot.id,"sequence":context.snapshot.sequence,"program_revision":context.snapshot.program_revision,"method":{"id":context.snapshot.method.id,"revision":context.snapshot.method.revision,"digest":context.snapshot.method.digest}},"current_program_revision":context.current_program_revision,"counts":{"candidates":draft.map_or(0,|draft|draft.candidates.len()),"source_refs":context.snapshot.source_refs.len(),"rules":context.snapshot.rules.len(),"stale_reasons":context.stale_reasons.len()},"field_destinations":{"context_snapshot_and_knowledge":overview,"draft":candidates,"program":program,"latest_review":reviews,"candidate_graph_contract":crate::api::schema_help_action("command",&json!({"route":"scope.candidates.save"}))?.ok_or(Error::InternalInvariant)?},"omitted_fields":["context.snapshot.method.body","context.snapshot.rules[].text","context.snapshot.source_refs","context.planning_knowledge","context.stale_reasons","draft","latest_review","candidate_graph_contract"]}),
        vec![
            read_action(context, CandidateContextView::Overview, None)?,
            next,
        ],
        Some(0),
    ))
}

#[cfg(test)]
mod planning_read_tests;
