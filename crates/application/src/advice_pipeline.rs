//! The `pipeline` advice point on the Slice draft save: a second point on that
//! command, asked after `sizing` with a request, a journal row and a reply field of
//! its own. It runs after the save has committed, for the work nodes the save
//! created or changed: it reads what `sizing` reads at this level and asks Jev once.
use crate::advice::{earlier, words};
use crate::advice_ports::NO_MODE;
use crate::{AdviceAsk, AdviceOutputGuard, AdviceRequest, TransactionMode, WorkspaceService};
use tect_domain::*;

const SLICE_CANDIDATES_SAVE: &str = "slice.candidates.save";

impl WorkspaceService {
    /// Jev's choice of pipeline for the work nodes that `slice.candidates.save` has
    /// created or changed in the draft it stored, laid out in `draft`. `None` when
    /// Jev is disabled; nothing is read beyond the setting then.
    pub async fn advise_slice_candidate_pipeline(
        &self,
        context: &RequestContext,
        request: &SaveSliceCandidateDraft,
        saved: &SliceCandidateContext,
        draft: PipelineDraft,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let scope = &saved.scope;

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (PIPELINE_POINT, SLICE_CANDIDATES_SAVE);
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
        let asking = |ask| AdviceRequest {
            point: PIPELINE_POINT,
            route: SLICE_CANDIDATES_SAVE,
            work: &work,
            ask,
            reply,
        };
        let Some(mode) = tx.program_mode(workspace.id, basis.program_id).await? else {
            tx.commit().await?;
            // The point fired for a Program that has no mode: the journal and the
            // reply say so, and nothing is gathered for Jev.
            let without_mode = AdviceAsk::Unmet {
                reason: AdviceUnavailableReason::ModeNotSet,
                detail: NO_MODE,
            };
            return self.advise(context, &asking(without_mode)).await;
        };
        let behind = (scope.source_candidate_set_id, &basis);
        let planning = (saved.candidate_set.id, request.input_cursor);
        let words = words::slice_level(&mut *tx, workspace.id, behind, planning).await?;
        tx.commit().await?;

        // Inside an opened Scope the level is the Scope's: its override, else the
        // Program's mode.
        let advice = draft.advice(&words, scope.effective_mode(mode));
        let questions = AdviceAsk::Questions {
            questions: advice.questions(),
            shaping: &advice,
        };
        self.advise(context, &asking(questions)).await
    }
}
