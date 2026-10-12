//! What the `sizing` point lets Jev read: the product level the draft is planned
//! under, the user's recorded words and one draft, its candidates under short keys
//! that the questions name. A candidate with a level of its own carries it.
use super::SizingLevel;
use super::draft_view::{Asked, DraftView};
use super::point;
use crate::{Error, NativeScope, ProgramMode, Result, ScopeCandidateDraft, SliceCandidateDraft};
use serde::Serialize;
use serde_json::{Value, json};
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

impl From<&NativeScope> for SizingScope {
    fn from(scope: &NativeScope) -> Self {
        Self {
            title: scope.title.clone(),
            outcome: scope.outcome.clone(),
            includes: scope.includes.clone(),
            excludes: scope.excludes.clone(),
        }
    }
}

/// The Program of a Scope candidate set and how far one of its snapshots had read
/// the inputs of the Program and of the set.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SizingWordsBasis {
    pub program_id: Uuid,
    pub program_latest_input: i64,
    pub planning_latest_input: i64,
}

/// One draft laid out for Jev; the user's words and the product level are added last,
/// after they are read.
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
        let view = DraftView::scope_draft(draft).ok_or(Error::InvalidArguments)?;
        let goals = draft
            .goals
            .iter()
            .map(|goal| match &goal.exact_quote {
                Some(quote) => json!({"text":goal.text,"quote":quote}),
                None => json!({"text":goal.text}),
            })
            .collect();
        Ok((!view.asked.is_empty()).then_some(Self {
            level: SizingLevel::ProgramToScopes,
            scope: None,
            goals,
            candidates: view.candidates,
            asked: view.asked,
        }))
    }

    /// A Slice candidate draft of an opened Scope. Jev is asked about its work nodes;
    /// decision nodes are shown so that dependencies read whole. `None` when the
    /// draft has no work node.
    pub fn slice_candidates(
        draft: &SliceCandidateDraft,
        scope: SizingScope,
    ) -> Result<Option<Self>> {
        let view = DraftView::slice_draft(draft).ok_or(Error::InvalidArguments)?;
        Ok((!view.asked.is_empty()).then_some(Self {
            level: SizingLevel::ScopeToSlices,
            scope: Some(scope),
            goals: Vec::new(),
            candidates: view.candidates,
            asked: view.asked,
        }))
    }

    /// The one JSON object Jev reads, with the user's words in recorded order and the
    /// product level with its meaning.
    pub(super) fn state(&self, user_words: &[SizingWord], mode: ProgramMode) -> Value {
        let mut state = json!({"level":self.level,"mode":point::mode(mode),
            "user_words":user_words,"goals":self.goals,"candidates":self.candidates});
        if let Some(scope) = &self.scope {
            state["scope"] = json!(scope);
        }
        state
    }
}
