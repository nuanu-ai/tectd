//! The `model` advice point on the two commands that record new user input: a second
//! point on those commands, asked after `intake` with a request, a journal row and a
//! reply field of its own. It runs after the command has committed, starts from what
//! `intake` starts from for the same input, and asks Jev once which kinds of executor
//! the work of the input needs.
use crate::advice_intake::{RecordedInput, SCOPE_CANDIDATES_RECORD_INPUT, SLICE_CANDIDATES_INPUT};
use crate::{AdviceAsk, AdviceOutputGuard, WorkspaceService};
use tect_domain::*;

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
