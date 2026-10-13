//! The `pipeline` advice point: which pipeline of the catalogue fits each work node
//! that a Slice candidate draft save created or changed. Jev chooses from the catalogue
//! the agent chose from and is not shown the agent's choice.
use super::draft_view::candidate_key;
use super::pipeline_catalog as catalog;
use super::point::{self, WIDEST_PROBABILITY, unanswered};
use super::sizing_state::{SizingDraft, SizingScope, SizingWord};
use super::written::Written;
use super::{
    ADVICE_ABSTAIN, AdviceAnswer, AdviceField, AdviceQuestionSet, AdviceShaping, CandidateAdvice,
    SizingLevel,
};
use crate::{
    Error, PipelineCatalogueEntry, PipelineCatalogueSnapshot, PipelineKind, ProgramMode,
    ResolvedSliceCandidateDraft, Result, SliceCandidateDraft, SliceCandidateDraftNode,
};
use serde_json::{Map, Value, json};
use std::collections::BTreeMap;

pub const PIPELINE_POINT: &str = "pipeline";
pub use catalog::NOTE as PIPELINE_NOTE;

/// A work node Jev is asked about: one the save created or changed, and the pipeline
/// the draft chose for it.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct Asked {
    written: Written,
    chosen: PipelineKind,
}

/// One saved draft laid out for Jev; the user's words and the product level are added
/// last, after they are read.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineDraft {
    /// The draft as `sizing` lays it out at level `scope_to_slices`.
    layout: SizingDraft,
    asked: Vec<Asked>,
    /// Each pipeline of the catalogue: its name and its description.
    pipelines: Vec<(String, String)>,
}

/// One saved draft prepared for Jev: the questions to ask and how the answers are shaped.
#[derive(Debug, Clone, PartialEq)]
pub struct PipelineAdvice {
    asked: Vec<Asked>,
    /// The longest name Jev may choose.
    longest: String,
    questions: AdviceQuestionSet,
}

impl PipelineDraft {
    /// A Slice candidate draft of an opened Scope as its save carried it, with the
    /// draft the save `stored` and the `catalogue` of the snapshot it was saved
    /// against. Jev is asked about the work nodes the save created or changed. `None`
    /// when it created and changed none.
    pub fn slice_candidates(
        draft: &SliceCandidateDraft,
        stored: &ResolvedSliceCandidateDraft,
        catalogue: &PipelineCatalogueSnapshot,
        scope: SizingScope,
    ) -> Result<Option<Self>> {
        // The layout holds the rule: it asks about exactly the work nodes the save wrote.
        let Some(layout) = SizingDraft::slice_candidates(draft, stored, scope)? else {
            return Ok(None);
        };
        let mut asked = Vec::new();
        for candidate in &layout.asked {
            let node = draft.nodes.get(candidate.written.position - 1);
            let Some(SliceCandidateDraftNode::Work { pipeline, .. }) = node else {
                return Err(Error::InternalInvariant);
            };
            asked.push(Asked {
                written: candidate.written,
                chosen: *pipeline,
            });
        }
        // The save has held that a work node it writes names a pipeline of this catalogue.
        let offered = |node: &Asked| catalogue.entries.iter().any(|e| e.kind == node.chosen);
        if !asked.iter().all(offered) {
            return Err(Error::InternalInvariant);
        }
        Ok(Some(Self {
            layout,
            asked,
            pipelines: catalogue.entries.iter().map(described).collect(),
        }))
    }

    /// What the point answers when the reply has no room for its advice. A Slice
    /// draft save keeps room for this field before `sizing`, the first point of the
    /// command, is measured, so the second point can always say why it gave nothing.
    pub fn without_room(&self) -> AdviceField {
        AdviceField::too_large(&subject(self.asked.len()))
    }

    /// Add the user's recorded words, in recorded order, and the opened Scope's
    /// effective product level, and form the questions: one per asked node, whose
    /// criteria are the pipelines of the catalogue.
    pub fn advice(self, user_words: &[SizingWord], mode: ProgramMode) -> PipelineAdvice {
        let criteria = self
            .pipelines
            .iter()
            .map(|(name, description)| (name.as_str(), description.as_str()))
            .collect::<Vec<_>>();
        let questions = self
            .asked
            .iter()
            .map(|node| {
                let key = candidate_key(node.written.position);
                let instructions = catalog::INSTRUCTIONS.replace("{key}", &key);
                (
                    question_name(node.written.position),
                    point::question(instructions, &criteria, catalog::ABSTAIN),
                )
            })
            .collect();
        let longest = criteria
            .iter()
            .map(|(name, _)| *name)
            .chain([ADVICE_ABSTAIN])
            .max_by_key(|name| name.len())
            .unwrap_or(ADVICE_ABSTAIN)
            .to_owned();
        PipelineAdvice {
            longest,
            questions: AdviceQuestionSet {
                state: self.layout.state(user_words, mode),
                questions,
            },
            asked: self.asked,
        }
    }
}

/// One pipeline as a criterion: its name and the three texts of its catalogue entry.
fn described(entry: &PipelineCatalogueEntry) -> (String, String) {
    let description = format!(
        "{} {} {} {} {} {}",
        catalog::CHOOSE_WHEN,
        entry.choose_when,
        catalog::DO_NOT_CHOOSE_WHEN,
        entry.do_not_choose_when,
        catalog::EXPECTED_RESULT,
        entry.expected_result
    );
    (entry.kind.as_str().to_owned(), description)
}

fn question_name(position: usize) -> String {
    format!("{}.pipeline", candidate_key(position))
}

impl PipelineAdvice {
    pub fn questions(&self) -> &AdviceQuestionSet {
        &self.questions
    }

    fn advised(&self, candidates: Vec<Value>) -> AdviceField {
        AdviceField::Advised {
            content: Map::from_iter([
                ("note".to_owned(), json!(catalog::NOTE)),
                ("candidates".to_owned(), Value::Array(candidates)),
            ]),
        }
    }
}

impl PipelineAdvice {
    /// The entry of each asked node, in draft order, and whether Jev chose another
    /// pipeline than the draft; or the name of a question without a usable answer.
    fn entries(
        &self,
        answers: &BTreeMap<String, AdviceAnswer>,
    ) -> std::result::Result<Vec<(Value, bool)>, String> {
        let mut entries = Vec::new();
        for node in &self.asked {
            let name = question_name(node.written.position);
            let draft = node.chosen.as_str();
            let (Some((pipeline, pipeline_p)), Some(draft_pipeline_p)) = (
                point::chosen(answers, &name),
                point::given(answers, &name, draft),
            ) else {
                return Err(name);
            };
            let differs = pipeline != ADVICE_ABSTAIN && pipeline != draft;
            entries.push((
                json!({"candidate":node.written.position,"pipeline":pipeline,
                    "pipeline_p":pipeline_p,"draft_pipeline_p":draft_pipeline_p,
                    "differs":differs}),
                differs,
            ));
        }
        Ok(entries)
    }
}

/// What Jev chose for a work node in a kept answer, as a refusal says it: the pipeline.
pub(super) fn chose(answer: &Value) -> Option<String> {
    Some(answer.get("pipeline")?.as_str()?.to_owned())
}

impl AdviceShaping for PipelineAdvice {
    /// One entry per asked node, in draft order: a position, names of the catalogue
    /// and probabilities only, since the agent holds its own draft and catalogue.
    /// `draft_pipeline_p` is Jev's probability for the pipeline the draft chose, and
    /// `differs` says Jev chose another one. A node Jev abstained on keeps `ABSTAIN`
    /// and does not differ.
    fn shape(&self, answers: &BTreeMap<String, AdviceAnswer>) -> AdviceField {
        match self.entries(answers) {
            Ok(entries) => self.advised(entries.into_iter().map(|(entry, _)| entry).collect()),
            Err(name) => unanswered(&name),
        }
    }

    /// The entry of each asked node under the id and revision the store holds the
    /// node under. It asks for a change when Jev chose another pipeline than the draft.
    fn kept(&self, answers: &BTreeMap<String, AdviceAnswer>) -> Vec<CandidateAdvice> {
        // Nothing is kept of answers that made no advised field.
        let Ok(entries) = self.entries(answers) else {
            return Vec::new();
        };
        let kept = |(node, (answer, asks_change)): (&Asked, (Value, bool))| CandidateAdvice {
            level: SizingLevel::ScopeToSlices,
            candidate_id: node.written.id,
            candidate_revision: node.written.revision,
            asks_change,
            answer,
        };
        self.asked.iter().zip(entries).map(kept).collect()
    }

    /// Every asked node with the longest name, the widest probabilities and the
    /// longer of the two values of `differs`.
    fn largest(&self) -> AdviceField {
        self.advised(
            self.asked
                .iter()
                .map(|node| {
                    json!({"candidate":node.written.position,"pipeline":self.longest,
                        "pipeline_p":WIDEST_PROBABILITY,
                        "draft_pipeline_p":WIDEST_PROBABILITY,"differs":false})
                })
                .collect(),
        )
    }

    fn subject(&self) -> String {
        subject(self.asked.len())
    }
}

/// What the advice is about, as it reads in a message.
fn subject(asked: usize) -> String {
    format!("the pipelines of this draft (work nodes asked about: {asked})")
}

#[cfg(test)]
mod tests;
