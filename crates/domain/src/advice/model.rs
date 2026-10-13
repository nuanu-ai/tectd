//! The `model` advice point: which kinds of executor the work of an input the user has
//! just given needs. On the two commands that record input for a candidate set it is
//! a second point, asked after `intake` over the state `intake` reads. On the command
//! that records input on the Program it is the only point, asked over the part of
//! that state a Program has. It names kinds of work only: which model runs a kind is
//! the policy of the agent that reads the advice.
use super::input_state;
use super::intake::IntakePlanning;
use super::model_catalog as catalog;
use super::point::{self, WIDEST_PROBABILITY, unanswered};
use super::sizing_state::SizingWord;
use super::{
    ADVICE_ABSTAIN, AdviceAnswer, AdviceField, AdviceQuestionSet, AdviceShaping, CandidateAdvice,
};
use crate::{Program, ProgramMode};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const MODEL_POINT: &str = "model";
pub use catalog::NOTE as MODEL_NOTE;

const PRACTICAL: &str = "practical";
const SPECIALIST: &str = "specialist";
/// What the advice is about, as it reads in a message.
const SUBJECT: &str = "the executors of this input";

/// One recorded input prepared for Jev: the two questions to ask and how the answers
/// are shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct ModelAdvice {
    questions: AdviceQuestionSet,
}

impl ModelAdvice {
    /// The two questions about the input just recorded for a candidate set, over the
    /// state `intake` sends for the same input: `planning` lays it out from the same
    /// Program, product level, earlier words and new input.
    pub fn of(
        planning: &IntakePlanning,
        program: &Program,
        mode: ProgramMode,
        user_words: &[SizingWord],
        new_input: &str,
    ) -> Self {
        Self::over(planning.state(program, mode, user_words, new_input))
    }

    /// The same two questions about an input recorded on the Program itself, where
    /// `intake` is not asked: a Program has no candidate set to place the input in.
    /// The state is the part of the state of `intake` that a Program has: its own
    /// level, the Program's statement, the product level, the words recorded for the
    /// Program before the input, and the input. It holds no Scope and no candidates.
    pub fn of_program_input(
        program: &Program,
        mode: ProgramMode,
        user_words: &[SizingWord],
        new_input: &str,
    ) -> Self {
        let level = input_state::PROGRAM_LEVEL;
        Self::over(input_state::of(level, program, mode, user_words, new_input))
    }

    /// The two questions over what Jev reads about the input.
    fn over(state: Value) -> Self {
        let practical = point::question(
            catalog::PRACTICAL_INSTRUCTIONS.to_owned(),
            &catalog::PRACTICAL_CRITERIA,
            catalog::PRACTICAL_ABSTAIN,
        );
        let specialist = point::question(
            catalog::SPECIALIST_INSTRUCTIONS.to_owned(),
            &catalog::SPECIALIST_CRITERIA,
            catalog::SPECIALIST_ABSTAIN,
        );
        Self {
            questions: AdviceQuestionSet {
                state,
                questions: BTreeMap::from([
                    (PRACTICAL.to_owned(), practical),
                    (SPECIALIST.to_owned(), specialist),
                ]),
            },
        }
    }

    pub fn questions(&self) -> &AdviceQuestionSet {
        &self.questions
    }

    /// What the point answers when the reply has no room for its advice. A command
    /// that records input for a candidate set keeps room for this field before
    /// `intake`, its first point, is measured, so the second point can always say why
    /// it gave nothing.
    pub fn without_room() -> AdviceField {
        AdviceField::too_large(SUBJECT)
    }

    fn frame() -> Map<String, Value> {
        Map::from_iter([("note".to_owned(), json!(catalog::NOTE))])
    }
}

impl AdviceShaping for ModelAdvice {
    /// The chosen criterion and its probability for each of the two questions: tokens
    /// only, since the agent holds its own input and its own policy for each kind. A
    /// question Jev abstained on keeps `ABSTAIN`.
    fn shape(&self, answers: &BTreeMap<String, AdviceAnswer>) -> AdviceField {
        let mut content = Self::frame();
        for name in self.questions.questions.keys() {
            let Some((choice, probability)) = point::chosen(answers, name) else {
                return unanswered(name);
            };
            content.insert(name.clone(), json!(choice));
            content.insert(format!("{name}_p"), json!(probability));
        }
        AdviceField::Advised { content }
    }

    /// Both questions with the criterion whose name is longest and the widest
    /// probability.
    fn largest(&self) -> AdviceField {
        let mut content = Self::frame();
        for (name, question) in &self.questions.questions {
            let longest = question
                .criteria
                .keys()
                .map(String::as_str)
                .max_by_key(|criterion| criterion.len())
                .unwrap_or(ADVICE_ABSTAIN);
            content.insert(name.clone(), json!(longest));
            content.insert(format!("{name}_p"), json!(WIDEST_PROBABILITY));
        }
        AdviceField::Advised { content }
    }

    fn subject(&self) -> String {
        SUBJECT.to_owned()
    }

    /// No review is held to `model`: nothing is kept of it beyond its journal row.
    fn kept(&self, _: &BTreeMap<String, AdviceAnswer>) -> Vec<CandidateAdvice> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
