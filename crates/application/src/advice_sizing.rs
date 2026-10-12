//! The `sizing` advice point on the two draft saves. Each use case runs after its
//! save has committed: it reads the user's recorded words up to what the save
//! consumed, lays the saved draft out for Jev and asks once.
use crate::advice::{Earlier, earlier};
use crate::{AdviceOutputGuard, AdviceRequest, TransactionMode, WorkspaceService};
use tect_domain::*;

const SCOPE_CANDIDATES_SAVE: &str = "scope.candidates.save";
const SLICE_CANDIDATES_SAVE: &str = "slice.candidates.save";

impl WorkspaceService {
    /// Advice on the Scope candidate draft that `scope.candidates.save` has stored.
    /// `None` when Jev is disabled or the draft has no candidate; nothing is read
    /// beyond the setting in the first case and nothing at all in the second.
    pub async fn advise_scope_candidate_sizing(
        &self,
        context: &RequestContext,
        request: &SaveCandidateDraft,
        stored: &StoredCandidateContext,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let Some(draft) = SizingDraft::scope_candidates(&request.draft)? else {
            return Ok(None);
        };
        let set = &stored.context.candidate_set;
        let work = AdviceWorkRefs {
            request_id: Some(request.request_id),
            program_id: Some(set.program_id),
            candidate_set_id: Some(set.id),
            ..AdviceWorkRefs::default()
        };

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (SIZING_POINT, SCOPE_CANDIDATES_SAVE);
        let decided = earlier(&mut *tx, workspace.id, false, names, work.request_id).await?;
        if let Some(early) = early_answer(decided) {
            tx.commit().await?;
            return Ok(early);
        }
        // The Program's words as the save's snapshot had read them, then the words
        // recorded for this candidate set up to the cursor the save consumed.
        let program = tx
            .program_input_texts(
                workspace.id,
                set.program_id,
                stored.context.snapshot.program_latest_input,
            )
            .await?;
        let planning = tx
            .scope_candidate_input_texts(workspace.id, set.id, request.input_cursor)
            .await?;
        tx.commit().await?;

        let mut words = labelled(SizingWordSource::ProgramInput, program);
        words.extend(labelled(SizingWordSource::ScopeCandidateInput, planning));
        self.ask_sizing(
            context,
            SCOPE_CANDIDATES_SAVE,
            &work,
            draft.advice(&words),
            reply,
        )
        .await
    }

    /// Advice on the Slice candidate draft that `slice.candidates.save` has stored.
    /// `None` when Jev is disabled or the draft has no work node.
    pub async fn advise_slice_candidate_sizing(
        &self,
        context: &RequestContext,
        request: &SaveSliceCandidateDraft,
        saved: &SliceCandidateContext,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let scope = &saved.scope;
        let statement = SizingScope {
            title: scope.title.clone(),
            outcome: scope.outcome.clone(),
            includes: scope.includes.clone(),
            excludes: scope.excludes.clone(),
        };
        let Some(draft) = SizingDraft::slice_candidates(&request.draft, statement)? else {
            return Ok(None);
        };

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (SIZING_POINT, SLICE_CANDIDATES_SAVE);
        let request_id = Some(request.request_id);
        let decided = earlier(&mut *tx, workspace.id, false, names, request_id).await?;
        if let Some(early) = early_answer(decided) {
            tx.commit().await?;
            return Ok(early);
        }
        // The Scope candidate snapshot this planning is based on says which Program
        // it is and how far the words behind the Scope had been read.
        let basis = tx
            .scope_candidate_words_basis(
                workspace.id,
                scope.source_candidate_set_id,
                saved.snapshot.source_snapshot_id,
            )
            .await?
            .ok_or(Error::InternalInvariant)?;
        let program = tx
            .program_input_texts(workspace.id, basis.program_id, basis.program_latest_input)
            .await?;
        let scope_planning = tx
            .scope_candidate_input_texts(
                workspace.id,
                scope.source_candidate_set_id,
                basis.planning_latest_input,
            )
            .await?;
        let slice_planning = tx
            .slice_candidate_input_texts(workspace.id, saved.candidate_set.id, request.input_cursor)
            .await?;
        tx.commit().await?;

        let mut words = labelled(SizingWordSource::ProgramInput, program);
        words.extend(labelled(
            SizingWordSource::ScopeCandidateInput,
            scope_planning,
        ));
        words.extend(labelled(
            SizingWordSource::SliceCandidateInput,
            slice_planning,
        ));
        let work = AdviceWorkRefs {
            request_id,
            program_id: Some(basis.program_id),
            candidate_set_id: Some(saved.candidate_set.id),
            scope_id: Some(scope.id),
            ..AdviceWorkRefs::default()
        };
        self.ask_sizing(
            context,
            SLICE_CANDIDATES_SAVE,
            &work,
            draft.advice(&words),
            reply,
        )
        .await
    }

    async fn ask_sizing(
        &self,
        context: &RequestContext,
        route: &str,
        work: &AdviceWorkRefs,
        advice: SizingAdvice,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let request = AdviceRequest {
            point: SIZING_POINT,
            route,
            work,
            questions: advice.questions(),
            shaping: &advice,
            reply,
        };
        self.advise(context, &request).await
    }
}

/// The command's answer when the point is decided before anything is gathered: no
/// field for a disabled Jev, the stored field for a replay. `None` means go on.
fn early_answer(decided: Earlier) -> Option<Option<AdviceField>> {
    match decided {
        Earlier::Disabled => Some(None),
        Earlier::Stored(advice) => Some(Some(advice)),
        Earlier::Undecided(_) => None,
    }
}

fn labelled(source: SizingWordSource, texts: Vec<String>) -> Vec<SizingWord> {
    texts
        .into_iter()
        .map(|text| SizingWord { source, text })
        .collect()
}
