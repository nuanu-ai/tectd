//! What every advice point builds with: a choice question that always offers `ABSTAIN`,
//! the product level as a request states it, and an answer as a reply prints it.
use super::{ADVICE_ABSTAIN, AdviceAnswer, AdviceField, AdviceQuestion, AdviceUnavailableReason};
use crate::ProgramMode;
use serde_json::{Value, json};
use std::collections::BTreeMap;

/// The widest probability the reply can print: two decimals.
pub(super) const WIDEST_PROBABILITY: f64 = 0.99;

/// One choice question: its criteria, each a name with its description, and `ABSTAIN`.
pub(super) fn question(
    instructions: String,
    criteria: &[(&str, &str)],
    abstain: &str,
) -> AdviceQuestion {
    AdviceQuestion {
        instructions,
        criteria: criteria
            .iter()
            .copied()
            .chain([(ADVICE_ABSTAIN, abstain)])
            .map(|(name, description)| (name.to_owned(), description.to_owned()))
            .collect(),
    }
}

/// `state.mode` of a request: the product level with its meaning.
pub(super) fn mode(mode: ProgramMode) -> Value {
    json!({"level":mode,"description":mode.description()})
}

/// What Jev chose for one question, and its probability to two decimals.
pub(super) fn chosen<'a>(
    answers: &'a BTreeMap<String, AdviceAnswer>,
    name: &str,
) -> Option<(&'a str, f64)> {
    let answer = answers.get(name)?;
    Some((
        answer.choice.as_str(),
        given(answers, name, &answer.choice)?,
    ))
}

/// The probability Jev gave one criterion of a question, to two decimals.
pub(super) fn given(
    answers: &BTreeMap<String, AdviceAnswer>,
    name: &str,
    criterion: &str,
) -> Option<f64> {
    let probability = answers.get(name)?.probabilities.get(criterion)?;
    Some((probability * 100.0).round() / 100.0)
}

/// Unreachable after the answers were accepted; never a panic and never a guess.
pub(super) fn unanswered(name: &str) -> AdviceField {
    AdviceField::unavailable(
        AdviceUnavailableReason::InvalidResponse,
        &format!("question `{name}` has no usable answer"),
    )
}
