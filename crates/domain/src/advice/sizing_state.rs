//! What the `sizing` point lets Jev read: the product level the draft is planned
//! under, the user's recorded words and one draft, its candidates under short keys
//! that the questions name. A candidate with a level of its own carries it.
use super::SizingLevel;
use super::draft_view::DraftView;
use super::point;
use super::written::{self, Written};
use crate::{
    Error, NativeScope, ProgramMode, ResolvedCandidateDraft, ResolvedSliceCandidateDraft, Result,
    ScopeCandidateDraft, SliceCandidateDraft,
};
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

/// A candidate Jev is asked about: one the save created or changed, and how many
/// items it includes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub(super) struct Asked {
    pub written: Written,
    pub items: usize,
}

/// One saved draft laid out for Jev; the user's words and the product level are added
/// last, after they are read.
#[derive(Debug, Clone, PartialEq)]
pub struct SizingDraft {
    pub(super) level: SizingLevel,
    scope: Option<SizingScope>,
    goals: Vec<Value>,
    candidates: Vec<Value>,
    pub(super) asked: Vec<Asked>,
}

impl SizingDraft {
    /// A Scope candidate draft of a Program as its save carried it, with the draft
    /// the save `stored`. The whole draft is laid out; Jev is asked about the
    /// candidates the save created or changed. `None` when it created and changed none.
    pub fn scope_candidates(
        draft: &ScopeCandidateDraft,
        stored: &ResolvedCandidateDraft,
    ) -> Result<Option<Self>> {
        let view = DraftView::scope_draft(draft).ok_or(Error::InvalidArguments)?;
        let asked = asked(&view, written::scope_candidates(draft, stored)?)?;
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
            candidates: view.candidates,
            asked,
        }))
    }

    /// A Slice candidate draft of an opened Scope as its save carried it, with the
    /// draft the save `stored`. Jev is asked about the work nodes the save created or
    /// changed; the other nodes, decision nodes among them, are shown so that the
    /// draft reads whole. `None` when the save created and changed no work node.
    pub fn slice_candidates(
        draft: &SliceCandidateDraft,
        stored: &ResolvedSliceCandidateDraft,
        scope: SizingScope,
    ) -> Result<Option<Self>> {
        let view = DraftView::slice_draft(draft).ok_or(Error::InvalidArguments)?;
        let asked = asked(&view, written::slice_work_nodes(draft, stored)?)?;
        Ok((!asked.is_empty()).then_some(Self {
            level: SizingLevel::ScopeToSlices,
            scope: Some(scope),
            goals: Vec::new(),
            candidates: view.candidates,
            asked,
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

/// The candidates a save wrote, each with the number of items the layout shows it to
/// include.
fn asked(view: &DraftView, written: Vec<Written>) -> Result<Vec<Asked>> {
    let items = |written: &Written| {
        let shown = view
            .asked
            .iter()
            .find(|shown| shown.position == written.position);
        // A candidate a save wrote is work, and the layout shows every piece of work.
        shown
            .map(|shown| shown.items)
            .ok_or(Error::InternalInvariant)
    };
    written
        .into_iter()
        .map(|written| {
            Ok(Asked {
                items: items(&written)?,
                written,
            })
        })
        .collect()
}
