use crate::{Error, KnowledgeBindingPurpose, Result};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const PLANNING_KNOWLEDGE_POLICY_ID: &str = "tect:planning-knowledge";
pub const PLANNING_KNOWLEDGE_POLICY_VERSION: &str = "dk-4.1";
pub const PLANNING_KNOWLEDGE_MAX_BRIEFS: usize = 64;

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningStage {
    Program,
    Scope,
    SliceCandidates,
}

impl PlanningStage {
    pub const fn as_str(self) -> &'static str {
        match self {
            Self::Program => "program",
            Self::Scope => "scope",
            Self::SliceCandidates => "slice_candidates",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PlanningBriefSelectors {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub target_iris: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub environment_iris: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub action_classes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningBrief {
    pub local_id: String,
    pub stage: PlanningStage,
    pub instruction: String,
    #[serde(default)]
    pub conditions: Vec<String>,
    #[serde(default)]
    pub exceptions: Vec<String>,
    pub purpose: String,
    #[serde(default)]
    pub selectors: PlanningBriefSelectors,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct PlanningTaskContext {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub target_iris: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub environment_iris: Option<Vec<String>>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub action_classes: Option<Vec<String>>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningKnowledgeNeed {
    pub stage: PlanningStage,
    pub roles: Vec<KnowledgeBindingPurpose>,
    pub expected_abstraction: String,
    pub policy_id: String,
    pub policy_version: String,
    pub method: PlanningMethodSnapshot,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningMethodSnapshot {
    pub id: String,
    pub version: String,
    pub digest: String,
    pub body: String,
    pub origin_refs: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningKnowledgeItem {
    pub unit_id: Uuid,
    pub unit_revision: i64,
    pub brief_local_id: String,
    pub rdf_digest: String,
    pub purposes: Vec<KnowledgeBindingPurpose>,
    pub why_included: Vec<String>,
    pub instruction: String,
    pub conditions: Vec<String>,
    pub exceptions: Vec<String>,
    pub declared_purpose: String,
    pub selectors: PlanningBriefSelectors,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PlanningKnowledgeGapKind {
    NeedsContext,
    RequiredUnavailable,
    NeedsReview,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningKnowledgeGap {
    pub kind: PlanningKnowledgeGapKind,
    pub unit_id: Uuid,
    pub unit_revision: i64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub brief_local_id: Option<String>,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PlanningKnowledgeManifest {
    pub id: Uuid,
    pub digest: String,
    pub stage: PlanningStage,
    pub owner_id: Uuid,
    pub owner_revision: i64,
    pub input_revision: i64,
    pub request_id: Uuid,
    pub policy_id: String,
    pub policy_version: String,
    pub task_context_digest: String,
    pub task_context: PlanningTaskContext,
    pub workspace_generation: i64,
    pub needs: PlanningKnowledgeNeed,
    pub selected: Vec<PlanningKnowledgeItem>,
    pub unresolved_needs: Vec<PlanningKnowledgeGap>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PlanningManifestGuard {
    pub manifest_id: Uuid,
    pub digest: String,
    pub workspace_generation: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize, Default)]
pub struct PlanningKnowledgeStatus {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub manifest: Option<PlanningKnowledgeManifest>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub stale_reasons: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub warnings: Vec<String>,
}

impl PlanningBrief {
    pub fn validate(&self) -> Result<()> {
        if !text(&self.local_id, 128)
            || !text(&self.instruction, 65_536)
            || !text(&self.purpose, 4096)
            || !list(&self.conditions, 128, 4096)
            || !list(&self.exceptions, 128, 4096)
            || !iris(&self.selectors.target_iris)
            || !iris(&self.selectors.environment_iris)
            || !list(&self.selectors.action_classes, 128, 1024)
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

impl PlanningTaskContext {
    pub fn validate(&self) -> Result<()> {
        if self.target_iris.as_deref().is_some_and(|v| !iris(v))
            || self.environment_iris.as_deref().is_some_and(|v| !iris(v))
            || self
                .action_classes
                .as_deref()
                .is_some_and(|v| !list(v, 128, 1024))
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl PlanningManifestGuard {
    pub fn validate(&self) -> Result<()> {
        if self.manifest_id.is_nil() || !text(&self.digest, 256) || self.workspace_generation < 0 {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

impl PlanningMethodSnapshot {
    pub fn from_candidate(value: &crate::CandidateMethodSnapshot) -> Self {
        Self {
            id: value.id.clone(),
            version: value.revision.clone(),
            digest: value.digest.clone(),
            body: value.body.clone(),
            origin_refs: value.origin_refs.clone(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        if !text(&self.id, 256)
            || !text(&self.version, 128)
            || !text(&self.digest, 256)
            || !text(&self.body, 262_144)
            || !list(&self.origin_refs, 32, 4096)
            || self.origin_refs.is_empty()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

pub fn planning_knowledge_need(
    stage: PlanningStage,
    method: PlanningMethodSnapshot,
) -> PlanningKnowledgeNeed {
    PlanningKnowledgeNeed {
        stage,
        roles: vec![
            KnowledgeBindingPurpose::Required,
            KnowledgeBindingPurpose::Reference,
            KnowledgeBindingPurpose::Procedure,
            KnowledgeBindingPurpose::ProofBasis,
        ],
        expected_abstraction: stage.as_str().into(),
        policy_id: PLANNING_KNOWLEDGE_POLICY_ID.into(),
        policy_version: PLANNING_KNOWLEDGE_POLICY_VERSION.into(),
        method,
    }
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}
fn iri(value: &str) -> bool {
    text(value, 4096)
        && (value.starts_with("http://")
            || value.starts_with("https://")
            || value.starts_with("urn:"))
}
fn list(values: &[String], max_items: usize, max_bytes: usize) -> bool {
    values.len() <= max_items
        && values.iter().all(|v| text(v, max_bytes))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}
fn iris(values: &[String]) -> bool {
    values.len() <= 128
        && values.iter().all(|v| iri(v))
        && values.iter().collect::<BTreeSet<_>>().len() == values.len()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn task_context_preserves_unknown_and_known_empty() {
        let unknown: PlanningTaskContext = serde_json::from_value(serde_json::json!({})).unwrap();
        let known_empty: PlanningTaskContext =
            serde_json::from_value(serde_json::json!({"target_iris": []})).unwrap();
        assert_eq!(unknown.target_iris, None);
        assert_eq!(known_empty.target_iris, Some(Vec::new()));
        assert_ne!(unknown, known_empty);
        assert_eq!(
            serde_json::to_value(&unknown).unwrap(),
            serde_json::json!({})
        );
        assert_eq!(
            serde_json::to_value(&known_empty).unwrap(),
            serde_json::json!({"target_iris": []})
        );
    }

    #[test]
    fn task_context_rejects_duplicate_and_non_iri_targets() {
        let duplicate = PlanningTaskContext {
            target_iris: Some(vec!["urn:fixture:r1".into(), "urn:fixture:r1".into()]),
            ..PlanningTaskContext::default()
        };
        let invalid = PlanningTaskContext {
            target_iris: Some(vec!["fixture-r1".into()]),
            ..PlanningTaskContext::default()
        };
        assert_eq!(duplicate.validate(), Err(Error::InvalidArguments));
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
    }
}
