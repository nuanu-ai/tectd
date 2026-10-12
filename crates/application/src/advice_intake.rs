//! The `intake` advice point on the two commands that record new user input. Each use
//! case runs after its command has committed: it reads the Program's statement and
//! product level and the user's words recorded before the new input, lays the latest
//! saved draft out for Jev and asks once.
use crate::advice::{earlier, words};
use crate::advice_ports::NO_MODE;
use crate::{AdviceAsk, AdviceOutputGuard, AdviceRequest, TransactionMode, WorkspaceService};
use tect_domain::*;

const SCOPE_CANDIDATES_RECORD_INPUT: &str = "scope.candidates.record_input";
const SLICE_CANDIDATES_INPUT: &str = "slice.candidates.input";
/// A point that fired for a Program without a mode gathers nothing for Jev.
const WITHOUT_MODE: AdviceAsk<'static> = AdviceAsk::Unmet {
    reason: AdviceUnavailableReason::ModeNotSet,
    detail: NO_MODE,
};

impl WorkspaceService {
    /// Advice on the input that `scope.candidates.record_input` has recorded, given as
    /// `stored`, the context that command returned. `None` when Jev is disabled;
    /// nothing is read beyond the setting then.
    pub async fn advise_scope_candidate_intake(
        &self,
        context: &RequestContext,
        request: &RecordCandidateInput,
        stored: &StoredCandidateContext,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let set = &stored.context.candidate_set;
        let work = AdviceWorkRefs {
            request_id: Some(request.request_id),
            program_id: Some(set.program_id),
            candidate_set_id: Some(set.id),
            ..AdviceWorkRefs::default()
        };

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (INTAKE_POINT, SCOPE_CANDIDATES_RECORD_INPUT);
        let decided = earlier(&mut *tx, workspace.id, false, names, work.request_id).await?;
        if let Some(early) = decided.answer() {
            tx.commit().await?;
            return Ok(early);
        }
        let program = tx
            .program(workspace.id, set.program_id, false)
            .await?
            .ok_or(Error::InternalInvariant)?;
        let Some(mode) = program.mode else {
            tx.commit().await?;
            return self
                .advise_intake(
                    context,
                    SCOPE_CANDIDATES_RECORD_INPUT,
                    &work,
                    WITHOUT_MODE,
                    reply,
                )
                .await;
        };
        // The Program's words as the set's snapshot had read them, then the words
        // recorded for this candidate set before the new input, which is its latest.
        let behind = (set.program_id, stored.context.snapshot.program_latest_input);
        let earlier_words = (set.id, set.latest_input - 1);
        let words = words::scope_level(&mut *tx, workspace.id, behind, earlier_words).await?;
        tx.commit().await?;

        let advice = IntakePlanning::scope_candidates(stored.draft.as_ref())?.advice(
            &program,
            mode,
            &words,
            &request.input,
        );
        let ask = AdviceAsk::Questions {
            questions: advice.questions(),
            shaping: &advice,
        };
        self.advise_intake(context, SCOPE_CANDIDATES_RECORD_INPUT, &work, ask, reply)
            .await
    }

    /// Advice on the input that `slice.candidates.input` has recorded, given as
    /// `recorded`, the context that command returned. `None` when Jev is disabled.
    pub async fn advise_slice_candidate_intake(
        &self,
        context: &RequestContext,
        request: &RecordSliceCandidateInput,
        recorded: &SliceCandidateContext,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let (scope, set) = (&recorded.scope, &recorded.candidate_set);

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (INTAKE_POINT, SLICE_CANDIDATES_INPUT);
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
                recorded.snapshot.source_snapshot_id,
            )
            .await?
            .ok_or(Error::InternalInvariant)?;
        let work = AdviceWorkRefs {
            request_id,
            program_id: Some(basis.program_id),
            candidate_set_id: Some(set.id),
            scope_id: Some(scope.id),
            ..AdviceWorkRefs::default()
        };
        let program = tx
            .program(workspace.id, basis.program_id, false)
            .await?
            .ok_or(Error::InternalInvariant)?;
        let Some(mode) = program.mode else {
            tx.commit().await?;
            return self
                .advise_intake(context, SLICE_CANDIDATES_INPUT, &work, WITHOUT_MODE, reply)
                .await;
        };
        // The Slice candidate set's own words stop before the new input, its latest.
        let behind = (scope.source_candidate_set_id, &basis);
        let earlier_words = (set.id, set.latest_input - 1);
        let words = words::slice_level(&mut *tx, workspace.id, behind, earlier_words).await?;
        tx.commit().await?;

        let advice = IntakePlanning::slice_candidates(recorded.draft.as_ref(), scope.into())?
            .advice(&program, mode, &words, &request.input);
        let ask = AdviceAsk::Questions {
            questions: advice.questions(),
            shaping: &advice,
        };
        self.advise_intake(context, SLICE_CANDIDATES_INPUT, &work, ask, reply)
            .await
    }

    async fn advise_intake(
        &self,
        context: &RequestContext,
        route: &str,
        work: &AdviceWorkRefs,
        ask: AdviceAsk<'_>,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let request = AdviceRequest {
            point: INTAKE_POINT,
            route,
            work,
            ask,
            reply,
        };
        self.advise(context, &request).await
    }
}
