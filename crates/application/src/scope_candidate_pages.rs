use crate::UnitOfWork;
use tect_domain::{
    CandidateContextPage, CandidateContextView, Error, Result, ScopeCandidatePageItem,
    StoredCandidateContext,
};

pub(crate) async fn context_page(
    tx: &mut dyn UnitOfWork,
    workspace_id: uuid::Uuid,
    stored: StoredCandidateContext,
    view: CandidateContextView,
    after: Option<i64>,
    limit: u32,
) -> Result<CandidateContextPage> {
    let offset = after.unwrap_or(0);
    let mut items = match view {
        CandidateContextView::Inputs => tx
            .candidate_inputs(
                workspace_id,
                stored.context.candidate_set.id,
                offset,
                limit + 1,
            )
            .await?
            .into_iter()
            .map(ScopeCandidatePageItem::Input)
            .collect(),
        CandidateContextView::Candidates => candidate_items(stored.draft.as_ref()),
        CandidateContextView::Reviews => stored
            .reviews
            .iter()
            .cloned()
            .map(ScopeCandidatePageItem::Review)
            .collect(),
        CandidateContextView::Overview | CandidateContextView::Program => Vec::new(),
        CandidateContextView::Fragment => return Err(Error::InvalidArguments),
    };
    if !matches!(view, CandidateContextView::Inputs) {
        items = items
            .into_iter()
            .skip(offset as usize)
            .take(limit as usize + 1)
            .collect();
    }
    let next_after = if items.len() > limit as usize {
        items.pop();
        Some(offset + limit as i64)
    } else {
        None
    };
    let required_protected_changes = stored
        .draft
        .as_ref()
        .into_iter()
        .flat_map(|draft| &draft.protected_changes)
        .map(|change| tect_domain::ProtectedObjectRef {
            accepted_evidence_id: change.accepted_evidence_id,
            prior_candidate_id: change.prior_candidate_id,
        })
        .collect();
    let terminal_note = matches!(view, CandidateContextView::Reviews)
        .then(|| match stored.context.candidate_set.status {
            tect_domain::CandidateSetStatus::Ready => "Candidate set is ready for user selection. Native Scope opening is not available; record input only for an explicit amendment.",
            tect_domain::CandidateSetStatus::Blocked => "Candidate set remains blocked. Inspect the preserved findings and record input only when new authority or context is available.",
            _ => "Continue with the schema-described draft or critical review action.",
        }.to_owned());
    let program = matches!(view, CandidateContextView::Program).then(|| {
        let program = stored.program;
        let mut field_refs = stored
            .context
            .snapshot
            .source_refs
            .iter()
            .filter(|source| {
                matches!(
                    source.kind,
                    tect_domain::CandidateSourceKind::ProgramField
                        | tect_domain::CandidateSourceKind::ProgramSuccess
                )
            })
            .cloned()
            .collect::<Vec<_>>();
        field_refs.sort_by_key(|source| match source.program_field.as_deref() {
            Some("name") => 1,
            Some("intent") => 2,
            Some("basis") => 3,
            Some("boundaries") => 4,
            Some("constraints") => 5,
            Some("success") => 6,
            _ => 7,
        });
        tect_domain::CandidateProgramSummary {
            id: program.id,
            status: program.status,
            revision: program.revision,
            current_step: program.current_step,
            input_cursor: program.input_cursor,
            latest_input: program.latest_input,
            field_refs,
        }
    });
    Ok(CandidateContextPage {
        context: stored.context,
        view,
        program,
        items,
        next_after,
        required_protected_changes,
        terminal_note,
    })
}

fn candidate_items(
    draft: Option<&tect_domain::ResolvedCandidateDraft>,
) -> Vec<ScopeCandidatePageItem> {
    let Some(draft) = draft else {
        return Vec::new();
    };
    draft
        .goals
        .iter()
        .cloned()
        .map(ScopeCandidatePageItem::Goal)
        .chain(
            draft
                .evidence
                .iter()
                .cloned()
                .map(ScopeCandidatePageItem::Evidence),
        )
        .chain(
            draft
                .blockers
                .iter()
                .cloned()
                .map(ScopeCandidatePageItem::Blocker),
        )
        .chain(
            draft
                .candidates
                .iter()
                .cloned()
                .map(ScopeCandidatePageItem::Candidate),
        )
        .collect()
}

pub(crate) fn validate_page(id: uuid::Uuid, after: Option<i64>, limit: u32) -> Result<()> {
    if id.is_nil() || after.is_some_and(|value| value < 0) || !(1..=100).contains(&limit) {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
