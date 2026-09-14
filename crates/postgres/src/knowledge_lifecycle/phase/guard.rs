use super::*;

use std::collections::BTreeSet;

pub(super) fn replay_outcome(
    value: KnowledgeChangeMutationOutcome,
) -> KnowledgeChangeMutationOutcome {
    match value {
        KnowledgeChangeMutationOutcome::Advanced(context)
        | KnowledgeChangeMutationOutcome::Replay(context) => {
            KnowledgeChangeMutationOutcome::Replay(context)
        }
    }
}

pub(super) async fn verify_envelope(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    definition: &KnowledgeChangeDefinition,
    phase_id: KnowledgeChangePhaseId,
    output: &KnowledgeAgentPhaseOutputDraft,
) -> Result<()> {
    let (plan, payload_erased): (Option<serde_json::Value>, bool) = sqlx::query_as(
        "SELECT branch_plan,payload_erased FROM knowledge_change_runs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let plan: Option<KnowledgeBranchPlan> = plan.map(decode).transpose()?;
    if payload_erased {
        if phase_id != KnowledgeChangePhaseId::KcResultHandoff
            || output.plan_revision != 0
            || !output.plan_digest.is_empty()
        {
            return Err(Error::StaleContext);
        }
    } else if phase_id.ordinal() <= KnowledgeChangePhaseId::KcQualifyPlan.ordinal() {
        if output.plan_revision != 0 || !output.plan_digest.is_empty() {
            return Err(Error::InputConflict);
        }
    } else {
        let plan = plan.as_ref().ok_or(Error::NeedsContext)?;
        if output.plan_revision != plan.revision || output.plan_digest != plan.digest {
            return Err(Error::StaleContext);
        }
    }
    let required_outputs:Vec<(String,i64,String)>=sqlx::query_as(
        "SELECT b.phase_id,b.output_revision,o.digest FROM knowledge_change_output_bindings b JOIN knowledge_change_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal<$4 AND b.stale=false AND o.payload_erased=false ORDER BY b.phase_ordinal"
    ).bind(tenant).bind(workspace).bind(run).bind(phase_id.ordinal() as i32).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected_outputs = required_outputs
        .into_iter()
        .map(|row| PipelineConsumedOutput {
            phase_id: row.0,
            output_revision: row.1,
            digest: row.2,
        })
        .collect::<BTreeSet<_>>();
    if output
        .consumed_outputs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        != expected_outputs
    {
        return Err(Error::StaleContext);
    }
    let required_inputs:Vec<(Uuid,i64,String)>=sqlx::query_as("SELECT id,sequence,digest FROM knowledge_change_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND payload_erased=false ORDER BY sequence").bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected_inputs = required_inputs
        .into_iter()
        .map(|row| PipelineConsumedInput {
            input_id: row.0,
            sequence: row.1,
            digest: row.2,
        })
        .collect::<BTreeSet<_>>();
    if output
        .consumed_inputs
        .iter()
        .cloned()
        .collect::<BTreeSet<_>>()
        != expected_inputs
    {
        return Err(Error::StaleContext);
    }
    let baseline:Option<KnowledgeBaselineManifest>=sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT baseline FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.map(decode).transpose()?;
    let expected_guards = baseline
        .map(|value| {
            value
                .targets
                .into_iter()
                .chain(value.dependencies)
                .collect::<Vec<_>>()
        })
        .unwrap_or_default();
    if phase_id.ordinal() > KnowledgeChangePhaseId::KcResolveBaseline.ordinal()
        && output.baseline_guards != expected_guards
    {
        return Err(Error::StaleContext);
    }
    let current = definition
        .phases
        .iter()
        .find(|value| value.id == phase_id)
        .ok_or(Error::InternalInvariant)?;
    let mut methods = BTreeSet::new();
    if let Some(id) = phase_id.method_id() {
        let own = current
            .methods
            .iter()
            .find(|value| value.id == id)
            .ok_or(Error::InvalidConfiguration)?;
        methods.insert((own.id.clone(), own.version.clone(), own.digest.clone()));
    }
    if let Some(plan) = plan.as_ref() {
        for obligation in &plan.obligations {
            if obligation.phase_id == phase_id {
                for value in &obligation.method_refs {
                    methods.insert((
                        value.id.clone(),
                        value.version.clone(),
                        value.digest.clone(),
                    ));
                }
            }
        }
        if (KnowledgeChangePhaseId::KcQualifyEvidence.ordinal()
            ..=KnowledgeChangePhaseId::KcReviewReconcile.ordinal())
            .contains(&phase_id.ordinal())
        {
            let registry: KnowledgeProfileRegistry = decode(
                sqlx::query_scalar("SELECT registry FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                    .bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?,
            )?;
            for profile_id in &plan.profiles {
                let profile = registry
                    .profiles
                    .iter()
                    .find(|value| value.profile_id == *profile_id)
                    .ok_or(Error::InvalidConfiguration)?;
                for value in &profile.methods {
                    methods.insert((
                        value.id.clone(),
                        value.version.clone(),
                        value.digest.clone(),
                    ));
                }
            }
        }
    }
    let reads = output
        .method_reads
        .iter()
        .map(|value| {
            (
                value.instruction_id.clone(),
                value.version.clone(),
                value.digest.clone(),
            )
        })
        .collect::<BTreeSet<_>>();
    if methods != reads {
        return Err(Error::StaleContext);
    }
    if phase_id.ordinal() >= KnowledgeChangePhaseId::KcPrepareChange.ordinal() {
        let pins:Option<Vec<KnowledgeResolvedSourcePin>>=sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT o.output->'data'->'data'->'source_pins' FROM knowledge_change_output_bindings b JOIN knowledge_change_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id='kc-qualify-evidence' AND b.stale=false").bind(tenant).bind(workspace).bind(run).fetch_optional(&mut **tx).await.map_err(storage_error)?.flatten().map(decode).transpose()?;
        let expected = pins
            .unwrap_or_default()
            .into_iter()
            .map(|value| value.digest)
            .collect::<Vec<_>>();
        if output.source_digests != expected {
            return Err(Error::StaleContext);
        }
    }
    Ok(())
}

pub(super) async fn seal_gate(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
    revision: i64,
    generation: i64,
) -> Result<KnowledgeReadyToCommit> {
    let completion:KnowledgeCompletionRequirement=decode(sqlx::query_scalar("SELECT completion FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?)?;
    if completion.search == KnowledgeSearchRequirement::Required {
        return Err(Error::KnowledgeUnavailable);
    }
    if completion.erasure == KnowledgeErasureRequirement::AllRetainedCopies {
        return Err(Error::UnsupportedCompletionRequirement);
    }
    let plan:KnowledgeBranchPlan=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT branch_plan FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
    let changeset: KnowledgeProposedChangeset = super::phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcPrepareChange,
    )
    .await?;
    let evidence: KnowledgeEvidenceManifest = super::phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcQualifyEvidence,
    )
    .await?;
    let checks: KnowledgeObligationReceipts = super::phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcDomainChecks,
    )
    .await?;
    let impact: KnowledgeImpactPlan = super::phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcImpactPlan,
    )
    .await?;
    let review: KnowledgeReviewReceipt = super::phase_data::load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcReviewReconcile,
    )
    .await?;
    if !evidence.unresolved_gaps.is_empty()
        || !checks.unresolved_obligation_ids.is_empty()
        || !impact.blocking_conflicts.is_empty()
        || review.outcome != KnowledgeReviewOutcome::Ready
    {
        return Err(Error::NeedsContext);
    }
    let baseline:KnowledgeBaselineManifest=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT baseline FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
    if baseline.workspace_generation != generation {
        return Err(Error::ContextChanged);
    }
    let seal_id = Uuid::new_v4();
    let command_digest = sealed_command_digest(
        &plan, &changeset, &evidence, &checks, &impact, &review, &baseline, generation, revision,
    )?;
    Ok(KnowledgeReadyToCommit {
        seal_id,
        plan_revision: plan.revision,
        plan_digest: plan.digest,
        changeset_digest: changeset.digest,
        run_revision: revision + 1,
        workspace_generation: generation,
        operation_ids: plan.operation_ids,
        command_digest,
    })
}
