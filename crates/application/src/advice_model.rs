//! The `model` advice point on the commands that record new user input. On the two
//! that record it for a candidate set it is a second point, asked after `intake` with
//! a request, a journal row and a reply field of its own, and it starts from what
//! `intake` starts from for the same input. On `program.record_input` it is the only
//! point: `intake` asks where an input lands in a candidate set, and a Program has
//! none. It runs after the command has committed and asks Jev once which kinds of
//! executor the work of the input needs.
use crate::advice::{earlier, words};
use crate::advice_intake::{
    RecordedInput, SCOPE_CANDIDATES_RECORD_INPUT, SLICE_CANDIDATES_INPUT, WITHOUT_MODE,
    WITHOUT_PROGRAM, readable_program,
};
use crate::{AdviceAsk, AdviceOutputGuard, TransactionMode, WorkspaceService};
use tect_domain::*;
use uuid::Uuid;

const PROGRAM_RECORD_INPUT: &str = "program.record_input";

impl WorkspaceService {
    /// Jev's advice on the executors of the input that `scope.candidates.record_input`
    /// has recorded, given as `stored`, the context that command returned. `None`
    /// when Jev is disabled; nothing is read beyond the setting then.
    pub async fn advise_scope_candidate_model(
        &self,
        context: &RequestContext,
        request: &RecordCandidateInput,
        stored: &StoredCandidateContext,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let read = self
            .recorded_scope_candidate_input(context, MODEL_POINT, request, stored)
            .await?;
        let asked = (SCOPE_CANDIDATES_RECORD_INPUT, request.input.as_str());
        self.advise_model(context, asked, read, reply).await
    }

    /// Jev's advice on the executors of the input that `slice.candidates.input` has
    /// recorded, given as `recorded`, the context that command returned. `None` when
    /// Jev is disabled.
    pub async fn advise_slice_candidate_model(
        &self,
        context: &RequestContext,
        request: &RecordSliceCandidateInput,
        recorded: &SliceCandidateContext,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let read = self
            .recorded_slice_candidate_input(context, MODEL_POINT, request, recorded)
            .await?;
        let asked = (SLICE_CANDIDATES_INPUT, request.input.as_str());
        self.advise_model(context, asked, read, reply).await
    }

    /// Jev's advice on the executors of the input that `program.record_input` has
    /// recorded for this Program under this request id. `None` when Jev is disabled;
    /// nothing is read beyond the setting then. Jev reads the Program's statement and
    /// product level, the words recorded for the Program before this input, and the
    /// input.
    pub async fn advise_program_input_model(
        &self,
        context: &RequestContext,
        (program_id, request_id): (Uuid, Uuid),
        input: &str,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let names = (MODEL_POINT, PROGRAM_RECORD_INPUT);
        let work = AdviceWorkRefs {
            request_id: Some(request_id),
            program_id: Some(program_id),
            ..AdviceWorkRefs::default()
        };

        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let (workspace, _) = Self::bound_session(&mut *tx, context, &identity).await?;
        let decided = earlier(&mut *tx, workspace.id, false, names, work.request_id).await?;
        if let Some(early) = decided.answer() {
            tx.commit().await?;
            return Ok(early);
        }
        let Some(program) = readable_program(&mut *tx, workspace.id, program_id).await? else {
            tx.commit().await?;
            return self
                .advise_recorded_input(context, names, &work, WITHOUT_PROGRAM, reply)
                .await;
        };
        let Some(mode) = program.mode else {
            tx.commit().await?;
            return self
                .advise_recorded_input(context, names, &work, WITHOUT_MODE, reply)
                .await;
        };
        // The command stored the input under this request id, now or in an earlier
        // attempt. The earlier words stop before it, wherever the inputs of the
        // Program have got to since.
        let recorded = tx
            .program_input(workspace.id, program_id, request_id)
            .await?
            .ok_or(Error::InternalInvariant)?;
        let earlier_words = (program_id, recorded.sequence - 1);
        let words = words::program_level(&mut *tx, workspace.id, earlier_words).await?;
        tx.commit().await?;

        let advice = ModelAdvice::of_program_input(&program, mode, &words, input);
        let questions = AdviceAsk::Questions {
            questions: advice.questions(),
            shaping: &advice,
        };
        self.advise_recorded_input(context, names, &work, questions, reply)
            .await
    }

    /// Ask `model` about the input a command of this route has recorded.
    async fn advise_model(
        &self,
        context: &RequestContext,
        (route, new_input): (&str, &str),
        read: RecordedInput,
        reply: &dyn AdviceOutputGuard,
    ) -> Result<Option<AdviceField>> {
        let names = (MODEL_POINT, route);
        let (work, reading) = match read {
            RecordedInput::Decided(early) => return Ok(early),
            RecordedInput::Unmet(work, unmet) => {
                return self
                    .advise_recorded_input(context, names, &work, unmet, reply)
                    .await;
            }
            RecordedInput::Read(work, reading) => (work, *reading),
        };
        let advice = ModelAdvice::of(
            &reading.planning,
            &reading.program,
            reading.mode,
            &reading.words,
            new_input,
        );
        let questions = AdviceAsk::Questions {
            questions: advice.questions(),
            shaping: &advice,
        };
        self.advise_recorded_input(context, names, &work, questions, reply)
            .await
    }
}
