//! A generic set of choice questions for Jev and the rules its answers must meet.
use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use std::collections::BTreeMap;

/// Criterion every question carries; choosing it means Jev gives no advice.
pub const ADVICE_ABSTAIN: &str = "ABSTAIN";
/// Jev reports each probability rounded to two decimals, so the sum of a question's
/// probabilities may miss 1 by up to half a hundredth per criterion.
const PROBABILITY_ROUNDING: f64 = 0.005;
/// Room for the error of adding the rounded values as binary floats.
const FLOAT_SLACK: f64 = 1e-9;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct AdviceQuestion {
    pub instructions: String,
    /// Criterion name to its description, `ABSTAIN` included.
    pub criteria: BTreeMap<String, String>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceQuestionSet {
    /// The material Jev reads: a JSON object the advice point fills.
    pub state: Value,
    pub questions: BTreeMap<String, AdviceQuestion>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct AdviceAnswer {
    pub choice: String,
    pub probabilities: BTreeMap<String, f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub confidence: Option<f64>,
}

impl AdviceQuestionSet {
    pub fn validate(&self) -> Result<()> {
        let valid = self.state.is_object()
            && !self.questions.is_empty()
            && self.questions.iter().all(|(name, question)| {
                !name.trim().is_empty()
                    && !question.instructions.trim().is_empty()
                    && question.criteria.len() >= 2
                    && question.criteria.contains_key(ADVICE_ABSTAIN)
                    && question.criteria.iter().all(|(criterion, description)| {
                        !criterion.trim().is_empty() && !description.trim().is_empty()
                    })
            });
        if valid {
            Ok(())
        } else {
            Err(Error::InvalidArguments)
        }
    }

    /// Accept answers only when every question is answered, the probabilities cover
    /// exactly its criteria and sum to 1 within their rounding, and the choice is the
    /// most probable one within that rounding.
    /// The error names the first rule that failed.
    pub fn accept(
        &self,
        answers: &BTreeMap<String, AdviceAnswer>,
    ) -> std::result::Result<(), String> {
        for (name, question) in &self.questions {
            let answer = answers
                .get(name)
                .ok_or_else(|| format!("question `{name}` has no answer"))?;
            question
                .accept(answer)
                .map_err(|rule| format!("the answer to `{name}` {rule}"))?;
        }
        Ok(())
    }
}

impl AdviceQuestion {
    fn accept(&self, answer: &AdviceAnswer) -> std::result::Result<(), &'static str> {
        let unit = |value: f64| value.is_finite() && (0.0..=1.0).contains(&value);
        if !answer.probabilities.keys().eq(self.criteria.keys()) {
            return Err("does not give a probability for exactly the criteria asked");
        }
        if !answer.probabilities.values().copied().all(unit) {
            return Err("has a probability outside 0 to 1");
        }
        let rounding = PROBABILITY_ROUNDING * answer.probabilities.len() as f64 + FLOAT_SLACK;
        if (answer.probabilities.values().sum::<f64>() - 1.0).abs() > rounding {
            return Err("has probabilities that do not sum to 1");
        }
        let Some(chosen) = answer.probabilities.get(&answer.choice) else {
            return Err("chooses a criterion that was not asked");
        };
        // Two values rounded to two decimals: the chosen one may read up to a
        // hundredth below another and still be the most probable.
        let tie = 2.0 * PROBABILITY_ROUNDING + FLOAT_SLACK;
        if answer
            .probabilities
            .values()
            .any(|other| other - chosen > tie)
        {
            return Err("chooses a criterion that is not the most probable");
        }
        if !answer.confidence.is_none_or(unit) {
            return Err("has a confidence outside 0 to 1");
        }
        Ok(())
    }
}

/// Jev abstains when it chose `ABSTAIN` for every question it was asked.
pub(super) fn abstained(answers: &BTreeMap<String, AdviceAnswer>) -> bool {
    answers
        .values()
        .all(|answer| answer.choice == ADVICE_ABSTAIN)
}
