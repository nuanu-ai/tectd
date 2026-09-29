use crate::{
    KnowledgeAccessScope, KnowledgeBindingPurpose, KnowledgeBindingTarget, KnowledgeBindingVersion,
    KnowledgeContractRef, KnowledgeEpistemicState, KnowledgeEvidenceKind, KnowledgeKind,
    KnowledgeLifecycleState, KnowledgeProfileId, KnowledgeProfileSections,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeSourcePin {
    pub source_iri: String,
    pub digest: String,
    pub evidence_kind: KnowledgeEvidenceKind,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub observed_at: Option<String>,
    pub evidence_scope: String,
    pub title: String,
    pub uri: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeValidationPin {
    pub event_id: Uuid,
    pub event_iri: String,
    pub event_digest: String,
    pub sequence: i64,
    pub source_pins: Vec<PipelineKnowledgeSourcePin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub valid_until: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub review_due_at: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeBindingPin {
    pub binding_iri: String,
    pub target: KnowledgeBindingTarget,
    pub purpose: KnowledgeBindingPurpose,
    pub version_resolution: KnowledgeBindingVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_kind: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub definition_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeResource {
    pub unit_id: Uuid,
    pub revision: i64,
    pub lifecycle: KnowledgeLifecycleState,
    pub access_scope: KnowledgeAccessScope,
    pub rdf_digest: String,
    pub unit_iri: String,
    pub revision_iri: String,
    pub title: String,
    pub canonical_text: String,
    pub knowledge_kind: KnowledgeKind,
    pub epistemic_state: KnowledgeEpistemicState,
    pub target_iris: Vec<String>,
    pub profiles: Vec<KnowledgeProfileId>,
    pub conditions: Vec<String>,
    pub exceptions: Vec<String>,
    pub sections: KnowledgeProfileSections,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry_briefs: Option<Vec<crate::PlanningBrief>>,
    pub source_pins: Vec<PipelineKnowledgeSourcePin>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub latest_validation: Option<PipelineKnowledgeValidationPin>,
    pub binding: PipelineKnowledgeBindingPin,
    pub why_included: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKnowledgeProjectionPolicy {
    FullResources,
    ProgramPlanningBriefs,
    ScopePlanningBriefs,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeResourceManifest {
    pub id: Uuid,
    pub digest: String,
    pub semantic_digest: String,
    pub workspace_generation: i64,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub definition_version: String,
    pub definition_digest: String,
    pub method_requirements: Vec<KnowledgeContractRef>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub inquiry: Option<crate::PipelineInquiryContract>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub projection_policy: Option<PipelineKnowledgeProjectionPolicy>,
    pub selected: Vec<PipelineKnowledgeResource>,
    pub unresolved_needs: Vec<String>,
    pub freshness_warnings: Vec<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineKnowledgeResourceState {
    Inactive,
    Current,
    Stale,
    NeedsContext,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineKnowledgeResourceStatus {
    pub state: PipelineKnowledgeResourceState,
    pub current_generation: i64,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub changed_unit_ids: Vec<Uuid>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub freshness_warnings: Vec<String>,
    pub access_changed: bool,
}

/// Storage and wire identity for the compact resource manifest. Existing inline
/// `PipelineKnowledgeResourceManifest` remains the DK2 compatibility shape.
pub const PAGED_KNOWLEDGE_CONTRACT_VERSION: &str = "dk-2-paged";
pub const PAGED_RESOURCE_DIGEST_ALGORITHM: &str = "resource-json-v1-sha256";

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagedPipelineKnowledgeManifest {
    pub contract_version: String,
    pub id: Uuid,
    pub digest: String,
    pub semantic_digest: String,
    pub workspace_generation: i64,
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub definition_version: String,
    pub definition_digest: String,
    pub method_requirements: Vec<KnowledgeContractRef>,
    pub inquiry: Option<crate::PipelineInquiryContract>,
    pub projection_policy: Option<PipelineKnowledgeProjectionPolicy>,
    pub unresolved_needs: Vec<String>,
    pub freshness_warnings: Vec<String>,
    pub resource_count: i64,
    pub total_resource_bytes: i64,
    pub resource_digest_algorithm: String,
    pub page_route: String,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PagedKnowledgeEntryKind {
    Dk2Event,
    Dk1Legacy,
}

/// One commitment per selected binding. Source text and typed sections stay in
/// immutable revisions and publication events, never in these rows.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagedPipelineKnowledgeResourcePin {
    pub ordinal: i64,
    pub entry_kind: PagedKnowledgeEntryKind,
    pub unit_id: Uuid,
    pub revision: i64,
    pub publication_event_id: Option<Uuid>,
    pub rdf_digest: String,
    pub binding_id: Uuid,
    pub binding_pin: PipelineKnowledgeBindingPin,
    pub lifecycle: KnowledgeLifecycleState,
    pub access_scope: KnowledgeAccessScope,
    pub validation_event_id: Option<Uuid>,
    pub validation_event_digest: Option<String>,
    pub projection: PagedKnowledgeProjectionPin,
    pub resource_digest: String,
    pub resource_bytes: i64,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagedKnowledgeProjectionPin {
    pub policy: PipelineKnowledgeProjectionPolicy,
    /// Exact brief identities and digests in projected order; empty for full resources.
    pub inquiry_briefs: Vec<PagedKnowledgeBriefPin>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PagedKnowledgeBriefPin {
    pub id: String,
    pub digest: String,
}

/// Canonical commitment shared by capture and page delivery. The domain value
/// fixes field order and JSON encoding; callers must not hash database JSONB
/// text, whose key order is an implementation detail.
pub fn paged_manifest_digest(
    tenant: Uuid,
    workspace: Uuid,
    manifest: &PagedPipelineKnowledgeManifest,
    rows: &[PagedPipelineKnowledgeResourcePin],
    legacy_selected: &[crate::PipelineKnowledgeItem],
    legacy_unresolved: &[String],
) -> crate::Result<String> {
    manifest.validate(rows)?;
    // DK1 is also returned inline. Every inline item must correspond to a
    // pinned DK1 revision, and every pinned DK1 revision must be represented.
    // Multiple bindings may select the same revision, but the legacy inline
    // projection has one item per revision.
    let selected: BTreeSet<_> = legacy_selected
        .iter()
        .map(|item| (item.unit_id, item.revision, item.rdf_digest.as_str()))
        .collect();
    let pinned: BTreeSet<_> = rows
        .iter()
        .filter(|row| row.entry_kind == PagedKnowledgeEntryKind::Dk1Legacy)
        .map(|row| (row.unit_id, row.revision, row.rdf_digest.as_str()))
        .collect();
    if selected.len() != legacy_selected.len() || selected != pinned {
        return Err(crate::Error::InternalInvariant);
    }
    // Keep the original DK2-only commitment stable. Mixed DK1 manifests and
    // any inline gaps use v2 because their legacy projection must be bound.
    let identity = (
        &manifest.contract_version,
        manifest.id,
        &manifest.semantic_digest,
        manifest.workspace_generation,
        manifest.run_id,
        manifest.run_revision,
        &manifest.phase_id,
        &manifest.definition_version,
        &manifest.definition_digest,
    );
    let metadata = (
        &manifest.method_requirements,
        &manifest.inquiry,
        &manifest.projection_policy,
        &manifest.unresolved_needs,
        &manifest.freshness_warnings,
        manifest.resource_count,
        manifest.total_resource_bytes,
        &manifest.resource_digest_algorithm,
        &manifest.page_route,
    );
    let bytes = if pinned.is_empty() && legacy_unresolved.is_empty() {
        serde_json::to_vec(&(
            "dk-2-paged-manifest-v1",
            tenant,
            workspace,
            &identity,
            &metadata,
            rows,
        ))
    } else {
        serde_json::to_vec(&(
            "dk-2-paged-manifest-v2",
            tenant,
            workspace,
            &identity,
            &metadata,
            rows,
            legacy_selected,
            legacy_unresolved,
        ))
    }
    .map_err(|_| crate::Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

impl PagedPipelineKnowledgeManifest {
    pub fn validate(&self, rows: &[PagedPipelineKnowledgeResourcePin]) -> crate::Result<()> {
        if self.contract_version != PAGED_KNOWLEDGE_CONTRACT_VERSION
            || self.resource_digest_algorithm != PAGED_RESOURCE_DIGEST_ALGORITHM
            || !hex_digest(&self.digest)
            || !hex_digest(&self.semantic_digest)
            || self.workspace_generation < 0
            || self.run_revision < 1
            || self.phase_id.is_empty()
            || self.definition_version.is_empty()
            || self.definition_digest.is_empty()
            || self.page_route != "slice.pipeline.knowledge_page"
            || self.inquiry.is_some() != self.projection_policy.is_some()
            || self.resource_count < 0
            || self.total_resource_bytes < 0
            || self.resource_count != rows.len() as i64
        {
            return Err(crate::Error::InternalInvariant);
        }
        let mut bytes = 0_i64;
        for (ordinal, row) in rows.iter().enumerate() {
            row.validate()?;
            if row.ordinal != ordinal as i64 {
                return Err(crate::Error::InternalInvariant);
            }
            bytes = bytes
                .checked_add(row.resource_bytes)
                .ok_or(crate::Error::InternalInvariant)?;
        }
        if bytes != self.total_resource_bytes {
            return Err(crate::Error::InternalInvariant);
        }
        Ok(())
    }
}

impl PagedPipelineKnowledgeResourcePin {
    pub fn validate(&self) -> crate::Result<()> {
        if self.ordinal < 0
            || self.revision < 1
            || self.resource_bytes < 1
            || !hex_digest(&self.rdf_digest)
            || !hex_digest(&self.resource_digest)
            || (self.validation_event_id.is_some() != self.validation_event_digest.is_some())
            || self
                .validation_event_digest
                .as_ref()
                .is_some_and(|digest| !hex_digest(digest))
            || (self.entry_kind == PagedKnowledgeEntryKind::Dk2Event)
                != self.publication_event_id.is_some()
            || self.binding_pin.binding_iri.is_empty()
            || (self.projection.policy == PipelineKnowledgeProjectionPolicy::FullResources
                && !self.projection.inquiry_briefs.is_empty())
            || self
                .projection
                .inquiry_briefs
                .iter()
                .any(|pin| pin.id.is_empty() || !hex_digest(&pin.digest))
        {
            return Err(crate::Error::InternalInvariant);
        }
        Ok(())
    }
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

/// Deserializes persisted or replayed wire values without rewriting old inline
/// DK2 bodies or reinterpreting their historical digests.
pub fn decode_pipeline_resource_manifest(
    value: serde_json::Value,
) -> crate::Result<PipelineResourceManifestVersion> {
    match value
        .get("contract_version")
        .and_then(serde_json::Value::as_str)
    {
        Some(PAGED_KNOWLEDGE_CONTRACT_VERSION) => {
            let paged: PagedPipelineKnowledgeManifest =
                serde_json::from_value(value).map_err(|_| crate::Error::InternalInvariant)?;
            Ok(PipelineResourceManifestVersion::Paged(paged))
        }
        None | Some("dk-2") => {
            let inline: PipelineKnowledgeResourceManifest =
                serde_json::from_value(value).map_err(|_| crate::Error::InternalInvariant)?;
            Ok(PipelineResourceManifestVersion::InlineDk2(inline))
        }
        _ => Err(crate::Error::InternalInvariant),
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum PipelineResourceManifestVersion {
    InlineDk2(PipelineKnowledgeResourceManifest),
    Paged(PagedPipelineKnowledgeManifest),
}

#[cfg(test)]
#[path = "knowledge_consumer/paged_tests.rs"]
mod paged_tests;
