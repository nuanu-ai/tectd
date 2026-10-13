//! The `intake` advice point on the two commands that record new user input, and the
//! reading every point of those commands starts from. A point runs after its command
//! has committed: it reads the Program's statement and product level and the user's
//! words recorded before the new input, lays the latest saved draft out for Jev and
//! asks once.
use crate::advice::{earlier, words};
use crate::advice_ports::{NO_MODE, NO_PROGRAM};
use crate::{
    AdviceAsk, AdviceOutputGuard, AdviceRequest, TransactionMode, UnitOfWork, WorkspaceService,
};
use tect_domain::*;
use uuid::Uuid;

pub(crate) const SCOPE_CANDIDATES_RECORD_INPUT: &str = "scope.candidates.record_input";
pub(crate) const SLICE_CANDIDATES_INPUT: &str = "slice.candidates.input";
/// A point that fired for a Program without a mode gathers nothing for Jev.
pub(crate) const WITHOUT_MODE: AdviceAsk<'static> = AdviceAsk::Unmet {
    reason: AdviceUnavailableReason::ModeNotSet,
    detail: NO_MODE,
};
/// Nor does one that fired for a Program whose statement cannot be read.
pub(crate) const WITHOUT_PROGRAM: AdviceAsk<'static> = AdviceAsk::Unmet {
    reason: AdviceUnavailableReason::ProgramUnavailable,
    detail: NO_PROGRAM,
};

/// The Program a recorded input belongs to, or `None` when its text is erased or not
/// readable. That is not a failure of the command, which has recorded its input: the
/// point then says it has no advice.
pub(crate) async fn readable_program(
    tx: &mut dyn UnitOfWork,
    workspace_id: Uuid,
    program_id: Uuid,
) -> Result<Option<Program>> {
    match tx.program(workspace_id, program_id, false).await {
        Ok(program) => program.map(Some).ok_or(Error::InternalInvariant),
        Err(Error::KnowledgePayloadErased | Error::KnowledgeUnavailable) => Ok(None),
        Err(error) => Err(error),
    }
}

/// What a point of a recorded input starts from once the command has committed.
pub(crate) enum RecordedInput {
    /// Decided before anything is read: no field for a disabled Jev, the stored
    /// field for a replay.
    Decided(Option<AdviceField>),
    /// The point fired and has nothing to send: the Program has no mode, or its
    /// statement cannot be read.
    Unmet(AdviceWorkRefs, AdviceAsk<'static>),
    /// What Jev reads about the input, apart from the input itself.
    Read(AdviceWorkRefs, Box<InputReading>),
}

/// What a recorded input lands on, the Program's statement, the product level the
/// input is judged at and the user's earlier words in recorded order.
pub(crate) struct InputReading {
    pub planning: IntakePlanning,
    pub program: Program,
    pub mode: ProgramMode,
    pub words: Vec<SizingWord>,
}

impl WorkspaceService {
    /// What `point` starts from for the input that `scope.candidates.record_input`
    /// has recorded, given as `stored`, the context that command returned. Nothing is
    /// read beyond the setting when Jev is disabled.
    pub(crate) async fn recorded_scope_candidate_input(
        &self,
        context: &RequestContext,
        point: &str,
        request: &RecordCandidateInput,
        stored: &StoredCandidateContext,
    ) -> Result<RecordedInput> {
        let set = &stored.context.candidate_set;
        let work = AdviceWorkRefs {
            request_id: Some(request.request_id),
            program_id: Some(set.program_id),
            candidate_set_id: Some(set.id),
            ..AdviceWorkRefs::default()
        };

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (point, SCOPE_CANDIDATES_RECORD_INPUT);
        let decided = earlier(&mut *tx, workspace.id, false, names, work.request_id).await?;
        if let Some(early) = decided.answer() {
            tx.commit().await?;
            return Ok(RecordedInput::Decided(early));
        }
        let Some(program) = readable_program(&mut *tx, workspace.id, set.program_id).await? else {
            tx.commit().await?;
            return Ok(RecordedInput::Unmet(work, WITHOUT_PROGRAM));
        };
        let Some(mode) = program.mode else {
            tx.commit().await?;
            return Ok(RecordedInput::Unmet(work, WITHOUT_MODE));
        };
        // The Program's words as the set's snapshot had read them, then the words
        // recorded for this candidate set before the new input, which is its latest.
        let behind = (set.program_id, stored.context.snapshot.program_latest_input);
        let earlier_words = (set.id, set.latest_input - 1);
        let words = words::scope_level(&mut *tx, workspace.id, behind, earlier_words).await?;
        tx.commit().await?;

        let reading = InputReading {
            planning: IntakePlanning::scope_candidates(stored.draft.as_ref())?,
            program,
            mode,
            words,
        };
        Ok(RecordedInput::Read(work, Box::new(reading)))
    }

    /// What `point` starts from for the input that `slice.candidates.input` has
    /// recorded, given as `recorded`, the context that command returned.
    pub(crate) async fn recorded_slice_candidate_input(
        &self,
        context: &RequestContext,
        point: &str,
        request: &RecordSliceCandidateInput,
        recorded: &SliceCandidateContext,
    ) -> Result<RecordedInput> {
        let (scope, set) = (&recorded.scope, &recorded.candidate_set);

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let names = (point, SLICE_CANDIDATES_INPUT);
        let request_id = Some(request.request_id);
        let decided = earlier(&mut *tx, workspace.id, false, names, request_id).await?;
        if let Some(early) = decided.answer() {
            tx.commit().await?;
            return Ok(RecordedInput::Decided(early));
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
        let Some(program) = readable_program(&mut *tx, workspace.id, basis.program_id).await?
        else {
            tx.commit().await?;
            return Ok(RecordedInput::Unmet(work, WITHOUT_PROGRAM));
        };
        let Some(mode) = program.mode else {
            tx.commit().await?;
            return Ok(RecordedInput::Unmet(work, WITHOUT_MODE));
        };
        // The Slice candidate set's own words stop before the new input, its latest.
        let behind = (scope.source_candidate_set_id, &basis);
        let earlier_words = (set.id, set.latest_input - 1);
        let words = words::slice_level(&mut *tx, workspace.id, behind, earlier_words).await?;
        tx.commit().await?;

        let reading = InputReading {
            planning: IntakePlanning::slice_candidates(recorded.draft.as_ref(), scope.into())?,
            program,
            // Inside an opened Scope the level is the Scope's: its override, else the
            // Program's mode.
            mode: scope.effective_mode(mode),
            words,
        };
        Ok(RecordedInput::Read(work, Box::new(reading)))
    }

    /// One point of a recorded input asking Jev, or journalling why it does not.
    pub(crate) async fn advise_recorded_input(
        &self,
        context: &RequestContext,
        (point, route): (&str, &str),
        work: &AdviceWorkRefs,
        ask: AdviceAsk<'_>,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let request = AdviceRequest {
            point,
            route,
            work,
            ask,
            reply,
        };
        self.advise(context, &request).await
    }

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
        let read = self
            .recorded_scope_candidate_input(context, INTAKE_POINT, request, stored)
            .await?;
        let asked = (SCOPE_CANDIDATES_RECORD_INPUT, request.input.as_str());
        self.advise_intake(context, asked, read, reply).await
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
        let read = self
            .recorded_slice_candidate_input(context, INTAKE_POINT, request, recorded)
            .await?;
        let asked = (SLICE_CANDIDATES_INPUT, request.input.as_str());
        self.advise_intake(context, asked, read, reply).await
    }

    /// Ask `intake` about the input a command of this route has recorded.
    async fn advise_intake(
        &self,
        context: &RequestContext,
        (route, new_input): (&str, &str),
        read: RecordedInput,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let names = (INTAKE_POINT, route);
        let (work, reading) = match read {
            RecordedInput::Decided(early) => return Ok(early),
            RecordedInput::Unmet(work, unmet) => {
                return self
                    .advise_recorded_input(context, names, &work, unmet, reply)
                    .await;
            }
            RecordedInput::Read(work, reading) => (work, *reading),
        };
        let InputReading {
            planning,
            program,
            mode,
            words,
        } = reading;
        let advice = planning.advice(&program, mode, &words, new_input);
        let questions = AdviceAsk::Questions {
            questions: advice.questions(),
            shaping: &advice,
        };
        self.advise_recorded_input(context, names, &work, questions, reply)
            .await
    }
}
