//! What the `sizing` point lets Jev read: the user's recorded words and one draft, its
//! candidates under short keys that the questions name.
use super::SizingLevel;
use crate::{
    Error, Result, ScopeCandidateDraft, SliceCandidateDraft, SliceCandidateDraftNode,
    SliceCandidateRef,
};
use serde::Serialize;
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

/// Where a recorded text came from, so the original task and later additions stay apart.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "snake_case")]
pub enum SizingWordSource {
    ProgramInput,
    ScopeCandidateInput,
    SliceCandidateInput,
}

/// One recorded text, whole and as recorded.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SizingWord {
    pub source: SizingWordSource,
    pub text: String,
}

/// The opened Scope's own statement, as the store holds it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
pub struct SizingScope {
    pub title: String,
    pub outcome: String,
    pub includes: Vec<String>,
    pub excludes: Vec<String>,
}

/// The Program of a Scope candidate set and how far one of its snapshots had read
/// the inputs of the Program and of the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizingWordsBasis {
    pub program_id: Uuid,
    pub program_latest_input: i64,
    pub planning_latest_input: i64,
}

/// A candidate Jev is asked about: its 1-based draft position and how many items it includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Asked {
    pub position: usize,
    pub items: usize,
}

pub(super) fn candidate_key(position: usize) -> String {
    format!("c{position}")
}

pub(super) fn item_key(position: usize, item: usize) -> String {
    format!("c{position}.i{item}")
}

/// One draft laid out for Jev; the user's words are added last, after they are read.
#[derive(Debug, Clone, PartialEq)]
pub struct SizingDraft {
    pub(super) level: SizingLevel,
    scope: Option<SizingScope>,
    goals: Vec<Value>,
    candidates: Vec<Value>,
    pub(super) asked: Vec<Asked>,
}

impl SizingDraft {
    /// A Scope candidate draft of a Program. `None` when it has no candidate.
    pub fn scope_candidates(draft: &ScopeCandidateDraft) -> Result<Option<Self>> {
        let mut keys = Keys::default();
        for (index, candidate) in draft.candidates.iter().enumerate() {
            keys.name(
                candidate.identity.local.as_ref(),
                candidate.identity.id,
                index + 1,
            );
        }
        let mut candidates = Vec::new();
        let mut asked = Vec::new();
        for (index, candidate) in draft.candidates.iter().enumerate() {
            let dependencies = candidate
                .dependencies
                .iter()
                .map(|reference| keys.of(reference.local.as_ref(), reference.id))
                .collect::<Result<Vec<_>>>()?;
            asked.push(Asked {
                position: index + 1,
                items: candidate.includes.len(),
            });
            candidates.push(work(
                index + 1,
                (&candidate.title, &candidate.outcome),
                (&candidate.includes, &candidate.excludes),
                dependencies,
            ));
        }
        let goals = draft
            .goals
            .iter()
            .map(|goal| match &goal.exact_quote {
                Some(quote) => json!({"text":goal.text,"quote":quote}),
                None => json!({"text":goal.text}),
            })
            .collect();
        Ok((!asked.is_empty()).then_some(Self {
            level: SizingLevel::ProgramToScopes,
            scope: None,
            goals,
            candidates,
            asked,
        }))
    }

    /// A Slice candidate draft of an opened Scope. Jev is asked about its work nodes;
    /// decision nodes are shown so that dependencies read whole. `None` when the
    /// draft has no work node.
    pub fn slice_candidates(
        draft: &SliceCandidateDraft,
        scope: SizingScope,
    ) -> Result<Option<Self>> {
        let mut keys = Keys::default();
        for (index, node) in draft.nodes.iter().enumerate() {
            let (SliceCandidateDraftNode::Work { identity, .. }
            | SliceCandidateDraftNode::Decision { identity, .. }) = node;
            keys.name(identity.local.as_ref(), identity.candidate_id, index + 1);
        }
        let named = |dependencies: &[SliceCandidateRef]| {
            dependencies
                .iter()
                .map(|reference| match reference {
                    SliceCandidateRef::Local { local } => keys.of(Some(local), None),
                    SliceCandidateRef::Existing { candidate_id, .. } => {
                        keys.of(None, Some(*candidate_id))
                    }
                })
                .collect::<Result<Vec<_>>>()
        };
        let mut candidates = Vec::new();
        let mut asked = Vec::new();
        for (index, node) in draft.nodes.iter().enumerate() {
            let position = index + 1;
            candidates.push(match node {
                SliceCandidateDraftNode::Work {
                    title,
                    outcome,
                    includes,
                    excludes,
                    dependencies,
                    ..
                } => {
                    asked.push(Asked {
                        position,
                        items: includes.len(),
                    });
                    let mut candidate = work(
                        position,
                        (title, outcome),
                        (includes, excludes),
                        named(dependencies)?,
                    );
                    candidate["kind"] = json!("work");
                    candidate
                }
                SliceCandidateDraftNode::Decision {
                    title,
                    question,
                    dependencies,
                    ..
                } => json!({"key":candidate_key(position),"kind":"decision","title":title,
                    "question":question,"dependencies":named(dependencies)?}),
            });
        }
        Ok((!asked.is_empty()).then_some(Self {
            level: SizingLevel::ScopeToSlices,
            scope: Some(scope),
            goals: Vec::new(),
            candidates,
            asked,
        }))
    }

    /// The one JSON object Jev reads, with the user's words in recorded order.
    pub(super) fn state(&self, user_words: &[SizingWord]) -> Value {
        let mut state = json!({"level":self.level,"user_words":user_words,"goals":self.goals,
            "candidates":self.candidates});
        if let Some(scope) = &self.scope {
            state["scope"] = json!(scope);
        }
        state
    }
}

fn work(
    position: usize,
    (title, outcome): (&str, &str),
    (includes, excludes): (&[String], &[String]),
    dependencies: Vec<String>,
) -> Value {
    let includes = includes
        .iter()
        .enumerate()
        .map(|(index, text)| json!({"key":item_key(position, index + 1),"text":text}))
        .collect::<Vec<_>>();
    json!({"key":candidate_key(position),"title":title,"outcome":outcome,"includes":includes,
        "excludes":excludes,"dependencies":dependencies})
}

/// Candidate keys by the two ways a draft names a candidate: a local label or a backend id.
#[derive(Default)]
struct Keys {
    local: BTreeMap<String, String>,
    id: BTreeMap<Uuid, String>,
}

impl Keys {
    fn name(&mut self, local: Option<&String>, id: Option<Uuid>, position: usize) {
        if let Some(local) = local {
            self.local.insert(local.clone(), candidate_key(position));
        }
        if let Some(id) = id {
            self.id.insert(id, candidate_key(position));
        }
    }

    /// A dependency names a candidate of the same draft; the save has already held that.
    fn of(&self, local: Option<&String>, id: Option<Uuid>) -> Result<String> {
        local
            .and_then(|local| self.local.get(local))
            .or_else(|| id.and_then(|id| self.id.get(&id)))
            .cloned()
            .ok_or(Error::InvalidArguments)
    }
}
