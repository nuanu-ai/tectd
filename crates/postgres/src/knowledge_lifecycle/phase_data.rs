use super::*;
use std::collections::{BTreeMap, BTreeSet};

pub(super) struct PhaseUpdates {
    pub baseline: Option<KnowledgeBaselineManifest>,
    pub plan: Option<KnowledgeBranchPlan>,
    pub ready: Option<KnowledgeReadyToCommit>,
    pub result: Option<KnowledgeChangeResult>,
}

impl PhaseUpdates {
    pub(super) fn empty() -> Self {
        Self {
            baseline: None,
            plan: None,
            ready: None,
            result: None,
        }
    }
}

pub(super) async fn load_phase_data<T: serde::de::DeserializeOwned>(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase_id: KnowledgeChangePhaseId,
) -> Result<T> {
    let value:serde_json::Value=sqlx::query_scalar(
        "SELECT o.output->'data'->'data' FROM knowledge_change_output_bindings b JOIN knowledge_change_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND b.stale=false AND o.payload_erased=false"
    ).bind(tenant).bind(workspace).bind(run).bind(enum_text(&phase_id)?).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?;
    decode(value)
}

mod baseline;
mod impact;
mod process;
mod review;
mod sources;

pub(super) use baseline::current_baseline;
use baseline::validate_receipts;
pub(super) use impact::{current_impact, machine_impact_matches, reconcile_impact};
pub(super) use process::process_agent_data;
use review::{require_prior_findings, validate_no_change};
use sources::phase_digest;
pub(super) use sources::{
    current_source_pin_digest, resolve_sources, resolve_sources_at_revision, source_iri,
};
