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
        with_actions(
            json!({"disposition":disposition,"context":context}),
            vec![read_action(&context, CandidateContextView::Overview, None)?],
            Some(0),
        ),
        capacity,
    )
}

pub(crate) fn stored(stored: StoredCandidateContext, capacity: usize) -> Result<Value> {
    let latest_review = stored.reviews.last().cloned();
    let actions = if stored.context.stale_reasons.is_empty() {
        vec![read_action(
            &stored.context,
            CandidateContextView::Candidates,
            None,
        )?]
    } else {
        vec![refresh_action(&stored.context)?]
    };
    within(
        with_actions(
            json!({"context":stored.context,"draft":stored.draft,"latest_review":latest_review}),
            actions,
            Some(0),
        ),
        capacity,
    )
}

pub(crate) fn page(mut page: CandidateContextPage, after: i64, capacity: usize) -> Result<Value> {
    let original_count = page.items.len();
    loop {
        if page.items.len() < original_count {
            page.next_after = Some(after + page.items.len() as i64);
        }
        let (actions, recommended) = page_actions(&page)?;
        let value = with_actions(json!(&page), actions, recommended);
        if encoded_len(&value)? <= capacity {
            return Ok(value);
        }
        if page.items.pop().is_none() {
            return Err(Error::RequestTooLarge);
        }
    }
}

pub(crate) fn fragment(
    candidate_set_id: Uuid,
    fragment: CandidateTextFragment,
    capacity: usize,
) -> Result<Value> {
    let id = fragment.source_ref.id;
    let next = fragment.next_cursor;
    let actions = if let Some(cursor) = next {
        vec![crate::api::ready_action(
            "candidate_context",
            json!({"candidate_set_id":candidate_set_id,"view":"fragment","source_ref_id":id,"cursor":cursor}),
        )?]
    } else if let Some(next_source) = fragment.next_source_ref_id {
        vec![crate::api::ready_action(
            "candidate_context",
            json!({"candidate_set_id":candidate_set_id,"view":"fragment","source_ref_id":next_source,"cursor":0}),
        )?]
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
    if !page.context.stale_reasons.is_empty() {
        return Ok((vec![refresh_action(&page.context)?], Some(0)));
    }
    if let Some(after) = page.next_after {
        return Ok((
            vec![read_action(&page.context, page.view, Some(after))?],
            Some(0),
        ));
    }
    match page.view {
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
        CandidateContextView::Fragment => Err(Error::InternalInvariant),
    }
}

fn terminal_actions(page: &CandidateContextPage) -> Result<(Vec<Value>, Option<usize>)> {
    let context = &page.context;
    let set = &context.candidate_set;
    match set.status {
        CandidateSetStatus::Draft => Ok((vec![draft_action(context)?], Some(0))),
        CandidateSetStatus::ReviewRequired => Ok((
            vec![review_action(context, &page.required_protected_changes)?],
            Some(0),
        )),
        CandidateSetStatus::Ready | CandidateSetStatus::Blocked => {
            Ok((vec![record_input_action(set.id, set.revision)?], None))
        }
    }
}

fn draft_action(context: &CandidateContext) -> Result<Value> {
    let set = &context.candidate_set;
    crate::api::needs_action(
        "needs_input",
        "save_candidate_set",
        json!({
            "kind":"draft","candidate_set_id":set.id,"revision":set.revision,
            "snapshot_id":set.current_snapshot_id,"input_cursor":set.latest_input,
            "request_id":request_id(set.id, set.revision, "save_draft")
        }),
        "input",
        json!({"fields":[{"path":"arguments.params.draft","format":"Complete schema-valid candidate draft. Temporary local labels exist only inside this payload; durable UUIDs come from the backend reply."}]}),
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
    crate::api::needs_action(
        "needs_input",
        "save_candidate_set",
        json!({
            "kind":"review","candidate_set_id":set.id,"revision":set.revision,
            "snapshot_id":set.current_snapshot_id,"input_cursor":set.latest_input,
            "request_id":request_id(set.id, set.revision, "save_review"),
            "review":{"protected_change_reviews":protected_change_reviews}
        }),
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
    crate::api::ready_action(
        "refresh_candidate_set",
        json!({
            "candidate_set_id":set.id,"revision":set.revision,
            "request_id":request_id(set.id, set.revision, "refresh"),"program_revision":context.current_program_revision
        }),
    )
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
