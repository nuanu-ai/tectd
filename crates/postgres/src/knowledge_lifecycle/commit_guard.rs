use super::*;
use std::collections::BTreeSet;

pub(super) struct SealedCommit {
    pub changeset: KnowledgeProposedChangeset,
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn verify(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
    run_revision: i64,
    generation: i64,
    ready: &KnowledgeReadyToCommit,
) -> Result<SealedCommit> {
    let plan: KnowledgeBranchPlan = decode(
        sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT branch_plan FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(run)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NeedsContext)?,
    )?;
    let changeset: KnowledgeProposedChangeset = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcPrepareChange,
    )
    .await?;
    let evidence: KnowledgeEvidenceManifest = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcQualifyEvidence,
    )
    .await?;
    let checks: KnowledgeObligationReceipts = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcDomainChecks,
    )
    .await?;
    let impact: KnowledgeImpactPlan = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcImpactPlan,
    )
    .await?;
    let review: KnowledgeReviewReceipt = phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcReviewReconcile,
    )
    .await?;
    let baseline: KnowledgeBaselineManifest = decode(
        sqlx::query_scalar::<_, Option<serde_json::Value>>(
            "SELECT baseline FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(run)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NeedsContext)?,
    )?;
    let operation_ids = changeset
        .operations
        .iter()
        .map(|operation| operation.operation_id)
        .collect::<BTreeSet<_>>();
    let plan_ids = plan.operation_ids.iter().copied().collect::<BTreeSet<_>>();
    if ready.run_revision != run_revision
        || ready.workspace_generation != generation
        || ready.plan_revision != plan.revision
        || ready.plan_digest != plan.digest
        || ready.changeset_digest != changeset.digest
        || ready.operation_ids != plan.operation_ids
        || operation_ids != plan_ids
        || baseline.workspace_generation != generation
        || !evidence.unresolved_gaps.is_empty()
        || !checks.unresolved_obligation_ids.is_empty()
        || !impact.blocking_conflicts.is_empty()
        || review.outcome != KnowledgeReviewOutcome::Ready
    {
        return Err(Error::StaleContext);
    }
    let source_digest =
        phase_data::current_source_pin_digest(tx, tenant, workspace, change).await?;
    if source_digest != evidence.source_pin_digest {
        return Err(Error::InvalidSource);
    }
    for guard in baseline.targets.iter().chain(&baseline.dependencies) {
        event::verify_revision_guard(tx, tenant, workspace, guard).await?;
    }
    let current_impact = phase_data::current_impact(tx, tenant, workspace, change).await?;
    if !phase_data::machine_impact_matches(&current_impact, &impact) {
        return Err(Error::NeedsContext);
    }
    let command_digest = sealed_command_digest(
        &plan,
        &changeset,
        &evidence,
        &checks,
        &impact,
        &review,
        &baseline,
        generation,
        run_revision
            .checked_sub(1)
            .ok_or(Error::InternalInvariant)?,
    )?;
    if command_digest != ready.command_digest {
        return Err(Error::StaleContext);
    }
    Ok(SealedCommit { changeset })
}
