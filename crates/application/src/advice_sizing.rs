//! The `sizing` advice point on the two draft saves. Each use case runs after its
//! save has committed: it reads the Program's product level and the user's recorded
//! words up to what the save consumed, lays the saved draft out for Jev and asks once.
use crate::advice::{earlier, words};
use crate::advice_ports::NO_MODE;
use crate::{AdviceAsk, AdviceOutputGuard, AdviceRequest, TransactionMode, WorkspaceService};
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
        if let Some(early) = decided.answer() {
            tx.commit().await?;
            return Ok(early);
        }
        let Some(mode) = tx.program_mode(workspace.id, set.program_id).await? else {
            tx.commit().await?;
            return self
                .sizing_without_mode(context, SCOPE_CANDIDATES_SAVE, &work, reply)
                .await;
        };
        // The Program's words as the save's snapshot had read them, then the words
        // recorded for this candidate set up to the cursor the save consumed.
        let program = (set.program_id, stored.context.snapshot.program_latest_input);
        let planning = (set.id, request.input_cursor);
        let words = words::scope_level(&mut *tx, workspace.id, program, planning).await?;
        tx.commit().await?;

        self.ask_sizing(
            context,
            SCOPE_CANDIDATES_SAVE,
            &work,
            draft.advice(&words, mode),
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
        let Some(draft) = SizingDraft::slice_candidates(&request.draft, scope.into())? else {
            return Ok(None);
        };

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (SIZING_POINT, SLICE_CANDIDATES_SAVE);
        let request_id = Some(request.request_id);
        let decided = earlier(&mut *tx, workspace.id, false, names, request_id).await?;
        if let Some(early) = decided.answer() {
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
        let work = AdviceWorkRefs {
            request_id,
            program_id: Some(basis.program_id),
            candidate_set_id: Some(saved.candidate_set.id),
            scope_id: Some(scope.id),
            ..AdviceWorkRefs::default()
        };
        let Some(mode) = tx.program_mode(workspace.id, basis.program_id).await? else {
            tx.commit().await?;
            return self
                .sizing_without_mode(context, SLICE_CANDIDATES_SAVE, &work, reply)
                .await;
        };
        let behind = (scope.source_candidate_set_id, &basis);
        let planning = (saved.candidate_set.id, request.input_cursor);
        let words = words::slice_level(&mut *tx, workspace.id, behind, planning).await?;
        tx.commit().await?;

        // Inside an opened Scope the level is the Scope's: its override, else the
        // Program's mode.
        self.ask_sizing(
            context,
            SLICE_CANDIDATES_SAVE,
            &work,
            draft.advice(&words, scope.effective_mode(mode)),
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
            ask: AdviceAsk::Questions {
                questions: advice.questions(),
                shaping: &advice,
            },
            reply,
        };
        self.advise(context, &request).await
    }

    /// The point fired for a Program that has no mode: the journal and the reply
    /// say so, and nothing is gathered for Jev.
    async fn sizing_without_mode(
        &self,
        context: &RequestContext,
        route: &str,
        work: &AdviceWorkRefs,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let request = AdviceRequest {
            point: SIZING_POINT,
            route,
            work,
            ask: AdviceAsk::Unmet {
                reason: AdviceUnavailableReason::ModeNotSet,
                detail: NO_MODE,
            },
            reply,
        };
        self.advise(context, &request).await
    }
}
