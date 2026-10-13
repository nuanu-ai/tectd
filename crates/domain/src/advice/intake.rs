//! The `intake` advice point: how much new work an input the user has just given asks
//! for, where that work lands in what is planned, and how it sits with the earlier words.
use super::draft_view::{DraftView, candidate_key};
use super::intake_catalog as catalog;
use super::point::{self, WIDEST_PROBABILITY, unanswered};
use super::sizing_state::{SizingScope, SizingWord};
use super::{
    ADVICE_ABSTAIN, AdviceAnswer, AdviceField, AdviceQuestionSet, AdviceShaping, CandidateAdvice,
    SizingLevel,
};
use crate::{
    Error, Program, ProgramMode, ResolvedCandidateDraft, ResolvedSliceCandidateDraft, Result,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const INTAKE_POINT: &str = "intake";
pub use catalog::NOTE as INTAKE_NOTE;

const WORK: &str = "work";
const PLACEMENT: &str = "placement";
const NEAREST: &str = "nearest";
const FIT: &str = "fit";

/// What a new input lands on: the planning level, the opened Scope on the Slice
/// level, and the latest saved draft of the candidate set when there is one.
#[derive(Debug, Clone, PartialEq)]
pub struct IntakePlanning {
    level: SizingLevel,
    scope: Option<SizingScope>,
    draft: Option<DraftView>,
}

/// One recorded input prepared for Jev: the questions to ask and how the answers are shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct IntakeAdvice {
    level: SizingLevel,
    /// Draft positions of the candidates `nearest` may name; empty when it is not asked.
    askable: Vec<usize>,
    questions: AdviceQuestionSet,
}

impl IntakePlanning {
    /// The Scope candidate set of a Program, with its latest saved draft if it has one.
    pub fn scope_candidates(saved: Option<&ResolvedCandidateDraft>) -> Result<Self> {
        Ok(Self {
            level: SizingLevel::ProgramToScopes,
            scope: None,
            draft: stored(saved.map(DraftView::saved_scope_draft))?,
        })
    }

    /// The Slice candidate set of an opened Scope, with its latest saved draft if it
    /// has one. Decision nodes are shown and cannot be named.
    pub fn slice_candidates(
        saved: Option<&ResolvedSliceCandidateDraft>,
        scope: SizingScope,
    ) -> Result<Self> {
        Ok(Self {
            level: SizingLevel::ScopeToSlices,
            scope: Some(scope),
            draft: stored(saved.map(DraftView::saved_slice_draft))?,
        })
    }

    /// Add the Program's statement, the product level the input lands on (the
    /// Program's mode, and inside an opened Scope the Scope's effective level), the
    /// user's earlier words in recorded order and the input just recorded, and form
    /// the questions.
    pub fn advice(
        self,
        program: &Program,
        mode: ProgramMode,
        user_words: &[SizingWord],
        new_input: &str,
    ) -> IntakeAdvice {
        let askable = self
            .draft
            .iter()
            .flat_map(|view| &view.asked)
            .map(|asked| asked.position)
            .collect::<Vec<_>>();
        let mut questions = BTreeMap::new();
        let mut ask = |name: &str, instructions: &str, criteria: &[(&str, &str)], abstain| {
            let question = point::question(instructions.to_owned(), criteria, abstain);
            questions.insert(name.to_owned(), question);
        };

        let mut work = vec![catalog::WORK_NONE];
        work.extend(catalog::WORK_SIZES);
        ask(
            WORK,
            catalog::WORK_INSTRUCTIONS,
            &work,
            catalog::WORK_ABSTAIN,
        );

        // Without a candidate to name, the work cannot lie in one.
        let mut placement = Vec::new();
        if !askable.is_empty() {
            placement.extend(catalog::PLACEMENT_IN_DRAFT);
        }
        placement.push(catalog::PLACEMENT_NEW_CANDIDATE);
        if self.level == SizingLevel::ScopeToSlices {
            placement.push(catalog::PLACEMENT_SEPARATE_SCOPE);
        }
        placement.push(catalog::PLACEMENT_OUTSIDE_PROGRAM);
        ask(
            PLACEMENT,
            catalog::PLACEMENT_INSTRUCTIONS,
            &placement,
            catalog::PLACEMENT_ABSTAIN,
        );

        if !askable.is_empty() {
            let named = askable
                .iter()
                .map(|position| {
                    let key = candidate_key(*position);
                    let description = catalog::NEAREST_CANDIDATE.replace("{key}", &key);
                    (key, description)
                })
                .collect::<Vec<_>>();
            let mut nearest = named
                .iter()
                .map(|(key, description)| (key.as_str(), description.as_str()))
                .collect::<Vec<_>>();
            nearest.push(catalog::NEAREST_NONE);
            ask(
                NEAREST,
                catalog::NEAREST_INSTRUCTIONS,
                &nearest,
                catalog::NEAREST_ABSTAIN,
            );
        }

        ask(
            FIT,
            catalog::FIT_INSTRUCTIONS,
            &catalog::FIT_CRITERIA,
            catalog::FIT_ABSTAIN,
        );
        IntakeAdvice {
            level: self.level,
            questions: AdviceQuestionSet {
                state: self.state(program, mode, user_words, new_input),
                questions,
            },
            askable,
        }
    }

    /// The one JSON object Jev reads. `scope` is there on the Slice level and
    /// `candidates` when the set has a saved draft.
    fn state(
        &self,
        program: &Program,
        mode: ProgramMode,
        user_words: &[SizingWord],
        new_input: &str,
    ) -> Value {
        let mut state = json!({"level":self.level,"mode":point::mode(mode),
            "program":{"name":program.name,"intent":program.intent,"basis":program.basis,
                "boundaries":program.boundaries,"constraints":program.constraints,
                "success":program.success},
            "user_words":user_words,"new_input":new_input});
        if let Some(scope) = &self.scope {
            state["scope"] = json!(scope);
        }
        if let Some(view) = &self.draft {
            state["candidates"] = json!(view.candidates);
        }
        state
    }
}

/// A draft the store holds has passed its save: every dependency names a node of it.
fn stored(view: Option<Option<DraftView>>) -> Result<Option<DraftView>> {
    view.map(|view| view.ok_or(Error::InternalInvariant))
        .transpose()
}

impl IntakeAdvice {
    pub fn questions(&self) -> &AdviceQuestionSet {
        &self.questions
    }

    fn frame(&self) -> Map<String, Value> {
        Map::from_iter([
            ("note".to_owned(), json!(catalog::NOTE)),
            ("level".to_owned(), json!(self.level)),
        ])
    }

    /// A chosen criterion as the field prints it: a token, and for `nearest` the
    /// 1-based draft position of the candidate it names. `None` for a key that names
    /// no candidate that was offered.
    fn printed(&self, question: &str, choice: &str) -> Option<Value> {
        if question != NEAREST || choice == ADVICE_ABSTAIN || choice == catalog::NEAREST_NONE.0 {
            return Some(json!(choice));
        }
        self.askable
            .iter()
            .find(|position| candidate_key(**position) == choice)
            .map(|position| json!(position))
    }
}

impl AdviceShaping for IntakeAdvice {
    /// One value and its probability per question asked: tokens and a position only,
    /// since the agent holds its own input and draft. A question Jev abstained on
    /// keeps `ABSTAIN`.
    fn shape(&self, answers: &BTreeMap<String, AdviceAnswer>) -> AdviceField {
        let mut content = self.frame();
        for name in self.questions.questions.keys() {
            let printed = point::chosen(answers, name)
                .and_then(|(choice, probability)| Some((self.printed(name, choice)?, probability)));
            let Some((value, probability)) = printed else {
                return unanswered(name);
            };
            content.insert(name.clone(), value);
            content.insert(format!("{name}_p"), json!(probability));
        }
        AdviceField::Advised { content }
    }

    /// Every question asked, with the value that prints longest and the widest
    /// probability. A draft position is a number and stays shorter than `ABSTAIN`
    /// in any draft a request to Jev can carry.
    fn largest(&self) -> AdviceField {
        let mut content = self.frame();
        for (name, question) in &self.questions.questions {
            let widest = question
                .criteria
                .keys()
                .filter_map(|choice| self.printed(name, choice))
                .fold(json!(ADVICE_ABSTAIN), |widest, value| {
                    if value.to_string().len() > widest.to_string().len() {
                        value
                    } else {
                        widest
                    }
                });
            content.insert(name.clone(), widest);
            content.insert(format!("{name}_p"), json!(WIDEST_PROBABILITY));
        }
        AdviceField::Advised { content }
    }

    fn subject(&self) -> String {
        "this input".to_owned()
    }

    /// No review is held to `intake`: nothing is kept of it beyond its journal row.
    fn kept(&self, _: &BTreeMap<String, AdviceAnswer>) -> Vec<CandidateAdvice> {
        Vec::new()
    }
}

#[cfg(test)]
mod tests;
