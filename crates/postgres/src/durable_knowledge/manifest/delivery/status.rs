use super::*;
use std::collections::BTreeSet;

type ResourceIdentity = (Uuid, i64, String, String);

struct ResourceStatusBasis<'a> {
    workspace_generation: i64,
    run_revision: i64,
    semantic_digest: &'a str,
    definition_version: &'a str,
    definition_digest: &'a str,
    method_requirements: &'a [KnowledgeContractRef],
    inquiry: Option<&'a PipelineInquiryContract>,
    projection_policy: Option<PipelineKnowledgeProjectionPolicy>,
    items: BTreeSet<ResourceIdentity>,
}

pub(super) fn project_resource_status(
    generation: i64,
    run_revision: i64,
    manifest: Option<&PipelineKnowledgeResourceManifest>,
    current: &super::super::generic::Snapshot,
) -> PipelineKnowledgeResourceStatus {
    let basis = manifest.map(|old| ResourceStatusBasis {
        workspace_generation: old.workspace_generation,
        run_revision: old.run_revision,
        semantic_digest: &old.semantic_digest,
        definition_version: &old.definition_version,
        definition_digest: &old.definition_digest,
        method_requirements: &old.method_requirements,
        inquiry: old.inquiry.as_ref(),
        projection_policy: old.projection_policy,
        items: old
            .selected
            .iter()
            .map(|v| {
                (
                    v.unit_id,
                    v.revision,
                    v.rdf_digest.clone(),
                    v.binding.binding_iri.clone(),
                )
            })
            .collect(),
    });
    project_resource_status_from_basis(generation, run_revision, basis, current)
}

pub(crate) fn project_paged_resource_status(
    generation: i64,
    run_revision: i64,
    manifest: &PagedPipelineKnowledgeManifest,
    pins: &[PagedPipelineKnowledgeResourcePin],
    current: &super::super::generic::Snapshot,
) -> PipelineKnowledgeResourceStatus {
    let basis = ResourceStatusBasis {
        workspace_generation: manifest.workspace_generation,
        run_revision: manifest.run_revision,
        semantic_digest: &manifest.semantic_digest,
        definition_version: &manifest.definition_version,
        definition_digest: &manifest.definition_digest,
        method_requirements: &manifest.method_requirements,
        inquiry: manifest.inquiry.as_ref(),
        projection_policy: manifest.projection_policy,
        items: pins
            .iter()
            .map(|pin| {
                (
                    pin.unit_id,
                    pin.revision,
                    pin.rdf_digest.clone(),
                    pin.binding_pin.binding_iri.clone(),
                )
            })
            .collect(),
    };
    project_resource_status_from_basis(generation, run_revision, Some(basis), current)
}

fn project_resource_status_from_basis(
    generation: i64,
    run_revision: i64,
    basis: Option<ResourceStatusBasis<'_>>,
    current: &super::super::generic::Snapshot,
) -> PipelineKnowledgeResourceStatus {
    let Some(old) = basis else {
        return PipelineKnowledgeResourceStatus {
            state: PipelineKnowledgeResourceState::NeedsContext,
            current_generation: generation,
            changed_unit_ids: current
                .manifest
                .selected
                .iter()
                .map(|v| v.unit_id)
                .collect(),
            freshness_warnings: current.manifest.freshness_warnings.clone(),
            access_changed: current
                .blocking_gaps
                .iter()
                .any(|v| v == "resource_inaccessible"),
        };
    };
    let new_items: BTreeSet<ResourceIdentity> = current
        .manifest
        .selected
        .iter()
        .map(|v| {
            (
                v.unit_id,
                v.revision,
                v.rdf_digest.clone(),
                v.binding.binding_iri.clone(),
            )
        })
        .collect();
    let changed_unit_ids = old
        .items
        .symmetric_difference(&new_items)
        .map(|v| v.0)
        .collect::<BTreeSet<_>>()
        .into_iter()
        .collect();
    let same_basis = old.workspace_generation == generation
        && old.run_revision == run_revision
        && old.semantic_digest == current.manifest.semantic_digest
        && old.definition_version == current.manifest.definition_version
        && old.definition_digest == current.manifest.definition_digest
        && old.method_requirements == current.manifest.method_requirements
        && old.inquiry == current.manifest.inquiry.as_ref()
        && old.projection_policy == current.manifest.projection_policy;
    let state = if !current.blocking_gaps.is_empty() {
        PipelineKnowledgeResourceState::NeedsContext
    } else if same_basis {
        PipelineKnowledgeResourceState::Current
    } else {
        PipelineKnowledgeResourceState::Stale
    };
    PipelineKnowledgeResourceStatus {
        state,
        current_generation: generation,
        changed_unit_ids,
        freshness_warnings: current.manifest.freshness_warnings.clone(),
        access_changed: current
            .blocking_gaps
            .iter()
            .any(|v| v == "resource_inaccessible"),
    }
}
