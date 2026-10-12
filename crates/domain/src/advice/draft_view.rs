//! One draft's candidates as Jev reads them, each under a short key that the questions
//! name. Every advice point lays a draft out through this one projection, whether the
//! draft is the one a save carries or the one the store holds.
use crate::{
    ResolvedCandidateDraft, ResolvedSliceCandidateDraft, ScopeCandidateDraft, SliceCandidateDraft,
    SliceCandidateDraftNode, SliceCandidateNode, SliceCandidateRef,
};
use serde_json::{Value, json};
use std::collections::BTreeMap;
use uuid::Uuid;

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

/// The two ways a draft names a node: a local label or a backend id.
type Name<'a> = (Option<&'a String>, Option<Uuid>);

/// One node of a draft, in whichever form the draft is.
struct Node<'a> {
    name: Name<'a>,
    dependencies: Vec<Name<'a>>,
    body: Body<'a>,
}

enum Body<'a> {
    /// Work Jev is asked about. `marked` says so in the object, where a draft also
    /// holds decisions.
    Work {
        title: &'a str,
        outcome: &'a str,
        includes: &'a [String],
        excludes: &'a [String],
        marked: bool,
    },
    /// Shown so that dependencies read whole; Jev is never asked about it.
    Decision { title: &'a str, question: &'a str },
}

/// The candidates of one draft in draft order, and the ones Jev may be asked about.
/// Positions count every node of the draft.
#[derive(Debug, Clone, PartialEq)]
pub(super) struct DraftView {
    pub candidates: Vec<Value>,
    pub asked: Vec<Asked>,
}

impl DraftView {
    /// A Scope candidate draft as a save carries it. `None` when a dependency names
    /// no candidate of the same draft.
    pub fn scope_draft(draft: &ScopeCandidateDraft) -> Option<Self> {
        let nodes = draft.candidates.iter().map(|candidate| Node {
            name: (candidate.identity.local.as_ref(), candidate.identity.id),
            dependencies: candidate
                .dependencies
                .iter()
                .map(|reference| (reference.local.as_ref(), reference.id))
                .collect(),
            body: Body::Work {
                title: &candidate.title,
                outcome: &candidate.outcome,
                includes: &candidate.includes,
                excludes: &candidate.excludes,
                marked: false,
            },
        });
        Self::of(nodes.collect())
    }

    /// A Scope candidate draft as the store holds it.
    pub fn saved_scope_draft(draft: &ResolvedCandidateDraft) -> Option<Self> {
        let nodes = draft.candidates.iter().map(|candidate| Node {
            name: (None, Some(candidate.id)),
            dependencies: backend(&candidate.dependencies),
            body: Body::Work {
                title: &candidate.title,
                outcome: &candidate.outcome,
                includes: &candidate.includes,
                excludes: &candidate.excludes,
                marked: false,
            },
        });
        Self::of(nodes.collect())
    }

    /// A Slice candidate draft as a save carries it: work nodes and decision nodes.
    pub fn slice_draft(draft: &SliceCandidateDraft) -> Option<Self> {
        let nodes = draft.nodes.iter().map(|node| match node {
            SliceCandidateDraftNode::Work {
                identity,
                title,
                outcome,
                includes,
                excludes,
                dependencies,
                ..
            } => Node {
                name: (identity.local.as_ref(), identity.candidate_id),
                dependencies: named(dependencies),
                body: Body::Work {
                    title,
                    outcome,
                    includes,
                    excludes,
                    marked: true,
                },
            },
            SliceCandidateDraftNode::Decision {
                identity,
                title,
                question,
                dependencies,
                ..
            } => Node {
                name: (identity.local.as_ref(), identity.candidate_id),
                dependencies: named(dependencies),
                body: Body::Decision { title, question },
            },
        });
        Self::of(nodes.collect())
    }

    /// A Slice candidate draft as the store holds it.
    pub fn saved_slice_draft(draft: &ResolvedSliceCandidateDraft) -> Option<Self> {
        let nodes = draft.nodes.iter().map(|node| Node {
            name: (None, Some(node.id())),
            dependencies: backend(node.dependencies()),
            body: match node {
                SliceCandidateNode::Work {
                    title,
                    outcome,
                    includes,
                    excludes,
                    ..
                } => Body::Work {
                    title,
                    outcome,
                    includes,
                    excludes,
                    marked: true,
                },
                SliceCandidateNode::Decision {
                    title, question, ..
                } => Body::Decision { title, question },
            },
        });
        Self::of(nodes.collect())
    }

    fn of(nodes: Vec<Node<'_>>) -> Option<Self> {
        let mut keys = Keys::default();
        for (index, node) in nodes.iter().enumerate() {
            keys.name(node.name, index + 1);
        }
        let mut view = Self {
            candidates: Vec::new(),
            asked: Vec::new(),
        };
        for (index, node) in nodes.iter().enumerate() {
            let position = index + 1;
            let dependencies = node
                .dependencies
                .iter()
                .map(|name| keys.of(*name))
                .collect::<Option<Vec<_>>>()?;
            view.candidates.push(match node.body {
                Body::Work {
                    title,
                    outcome,
                    includes,
                    excludes,
                    marked,
                } => {
                    view.asked.push(Asked {
                        position,
                        items: includes.len(),
                    });
                    let includes = includes
                        .iter()
                        .enumerate()
                        .map(|(index, text)| {
                            json!({"key":item_key(position, index + 1),"text":text})
                        })
                        .collect::<Vec<_>>();
                    let mut candidate = json!({"key":candidate_key(position),"title":title,
                        "outcome":outcome,"includes":includes,"excludes":excludes,
                        "dependencies":dependencies});
                    if marked {
                        candidate["kind"] = json!("work");
                    }
                    candidate
                }
                Body::Decision { title, question } => {
                    json!({"key":candidate_key(position),"kind":"decision","title":title,
                        "question":question,"dependencies":dependencies})
                }
            });
        }
        Some(view)
    }
}

/// Dependencies of a Slice candidate node as a save names them.
fn named(references: &[SliceCandidateRef]) -> Vec<Name<'_>> {
    references
        .iter()
        .map(|reference| match reference {
            SliceCandidateRef::Local { local } => (Some(local), None),
            SliceCandidateRef::Existing { candidate_id, .. } => (None, Some(*candidate_id)),
        })
        .collect()
}

/// Dependencies of a stored node: backend ids of nodes of the same draft.
fn backend(dependencies: &[Uuid]) -> Vec<Name<'_>> {
    dependencies.iter().map(|id| (None, Some(*id))).collect()
}

/// Candidate keys by the two ways a draft names a candidate.
#[derive(Default)]
struct Keys {
    local: BTreeMap<String, String>,
    id: BTreeMap<Uuid, String>,
}

impl Keys {
    fn name(&mut self, (local, id): Name<'_>, position: usize) {
        if let Some(local) = local {
            self.local.insert(local.clone(), candidate_key(position));
        }
        if let Some(id) = id {
            self.id.insert(id, candidate_key(position));
        }
    }

    /// A dependency names a candidate of the same draft; the save has held that.
    fn of(&self, (local, id): Name<'_>) -> Option<String> {
        local
            .and_then(|local| self.local.get(local))
            .or_else(|| id.and_then(|id| self.id.get(&id)))
            .cloned()
    }
}

#[cfg(test)]
mod tests;
