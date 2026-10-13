//! The `sizing` advice point: how large each candidate of a draft is, whether to keep,
//! split or cut it, and which of its included items go beyond what was asked.
use super::draft_view::{candidate_key, item_key};
use super::point::{self, WIDEST_PROBABILITY, unanswered};
use super::sizing_catalog as catalog;
use super::sizing_state::{Asked, SizingDraft, SizingWord};
use super::{
    ADVICE_ABSTAIN, AdviceAnswer, AdviceField, AdviceQuestion, AdviceQuestionSet, AdviceShaping,
};
use crate::ProgramMode;
use serde::Serialize;
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const SIZING_POINT: &str = "sizing";
pub use catalog::NOTE as SIZING_NOTE;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SizingLevel {
    ProgramToScopes,
    ScopeToSlices,
}

/// One saved draft prepared for Jev: the questions to ask about the candidates the
/// save created or changed, and how the answers are shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct SizingAdvice {
    level: SizingLevel,
    asked: Vec<Asked>,
    questions: AdviceQuestionSet,
}

impl SizingDraft {
    /// Add the user's recorded words, in recorded order, and the product level the
    /// draft is planned under, and form the questions. That level is the Program's
    /// mode, and inside an opened Scope the Scope's effective level.
    pub fn advice(self, user_words: &[SizingWord], mode: ProgramMode) -> SizingAdvice {
        let mut questions = BTreeMap::new();
        for candidate in &self.asked {
            let key = candidate_key(candidate.written.position);
            questions.insert(
                format!("{key}.size"),
                question(
                    catalog::SIZE_INSTRUCTIONS,
                    (&key, ""),
                    &catalog::SIZE_CRITERIA,
                    catalog::SIZE_ABSTAIN,
                ),
            );
            questions.insert(
                format!("{key}.advice"),
                question(
                    catalog::ADVICE_INSTRUCTIONS,
                    (&key, ""),
                    &advice_criteria(self.level),
                    catalog::ADVICE_ABSTAIN,
                ),
            );
            for item in 1..=candidate.items {
                let item = item_key(candidate.written.position, item);
                questions.insert(
                    item.clone(),
                    question(
                        catalog::ITEM_INSTRUCTIONS,
                        (&key, &item),
                        &[
                            catalog::ITEM_REQUESTED,
                            catalog::ITEM_ENABLER,
                            catalog::ITEM_EXTRA,
                        ],
                        catalog::ITEM_ABSTAIN,
                    ),
                );
            }
        }
        SizingAdvice {
            level: self.level,
            questions: AdviceQuestionSet {
                state: self.state(user_words, mode),
                questions,
            },
            asked: self.asked,
        }
    }
}

impl SizingAdvice {
    pub fn questions(&self) -> &AdviceQuestionSet {
        &self.questions
    }

    fn advised(&self, candidates: Vec<Value>) -> AdviceField {
        AdviceField::Advised {
            content: Map::from_iter([
                ("note".to_owned(), json!(catalog::NOTE)),
                ("level".to_owned(), json!(self.level)),
                ("candidates".to_owned(), Value::Array(candidates)),
            ]),
        }
    }
}

impl AdviceShaping for SizingAdvice {
    /// One entry per asked candidate, in draft order: positions, tokens and
    /// probabilities only, since the agent holds its own draft. `extra_items` lists the
    /// included items Jev classed as extra and `extra_p` the probability of each, in
    /// the same order. A candidate Jev abstained on keeps `ABSTAIN`.
    fn shape(&self, answers: &BTreeMap<String, AdviceAnswer>) -> AdviceField {
        let chosen = |name: String| point::chosen(answers, &name);
        let mut candidates = Vec::new();
        for candidate in &self.asked {
            let key = candidate_key(candidate.written.position);
            let (mut extra_items, mut extra_p) = (Vec::new(), Vec::new());
            for item in 1..=candidate.items {
                match chosen(item_key(candidate.written.position, item)) {
                    Some((choice, probability)) if choice == catalog::ITEM_EXTRA.0 => {
                        extra_items.push(item);
                        extra_p.push(probability);
                    }
                    Some(_) => {}
                    None => return unanswered(&item_key(candidate.written.position, item)),
                }
            }
            let Some((size, size_p)) = chosen(format!("{key}.size")) else {
                return unanswered(&format!("{key}.size"));
            };
            let Some((advice, advice_p)) = chosen(format!("{key}.advice")) else {
                return unanswered(&format!("{key}.advice"));
            };
            candidates.push(
                json!({"candidate":candidate.written.position,"size":size,"size_p":size_p,
                "advice":advice,"advice_p":advice_p,"extra_items":extra_items,
                "extra_p":extra_p}),
            );
        }
        self.advised(candidates)
    }

    /// Every candidate with the longest tokens and every included item listed as extra.
    fn largest(&self) -> AdviceField {
        let size = longest(&catalog::SIZE_CRITERIA);
        let advice = longest(&advice_criteria(self.level));
        self.advised(
            self.asked
                .iter()
                .map(|candidate| {
                    json!({"candidate":candidate.written.position,"size":size,
                        "size_p":WIDEST_PROBABILITY,"advice":advice,
                        "advice_p":WIDEST_PROBABILITY,
                        "extra_items":(1..=candidate.items).collect::<Vec<_>>(),
                        "extra_p":vec![WIDEST_PROBABILITY; candidate.items]})
                })
                .collect(),
        )
    }

    fn subject(&self) -> String {
        let items = self.asked.iter().map(|candidate| candidate.items).sum();
        format!(
            "this draft ({}, {})",
            counted(self.asked.len(), "candidate"),
            counted(items, "included item")
        )
    }
}

fn advice_criteria(level: SizingLevel) -> Vec<(&'static str, &'static str)> {
    let mut criteria = catalog::ADVICE_CRITERIA.to_vec();
    if level == SizingLevel::ScopeToSlices {
        criteria.push(catalog::ADVICE_SEPARATE_SCOPE);
    }
    criteria
}

fn question(
    instructions: &str,
    (key, item): (&str, &str),
    criteria: &[(&str, &str)],
    abstain: &str,
) -> AdviceQuestion {
    point::question(
        instructions.replace("{key}", key).replace("{item}", item),
        criteria,
        abstain,
    )
}

/// The longest token Jev may choose for a question with these criteria.
fn longest(criteria: &[(&'static str, &'static str)]) -> &'static str {
    criteria
        .iter()
        .map(|(name, _)| *name)
        .chain([ADVICE_ABSTAIN])
        .max_by_key(|name| name.len())
        .unwrap_or(ADVICE_ABSTAIN)
}

fn counted(count: usize, noun: &str) -> String {
    format!("{count} {noun}{}", if count == 1 { "" } else { "s" })
}

#[cfg(test)]
mod tests;
