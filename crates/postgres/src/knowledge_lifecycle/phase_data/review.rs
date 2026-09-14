use super::*;

pub(in crate::knowledge_lifecycle) async fn validate_no_change(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let changeset: KnowledgeProposedChangeset = load_phase_data(
        tx,
        tenant,
        workspace,
        run,
        KnowledgeChangePhaseId::KcPrepareChange,
    )
    .await?;
    let baseline: KnowledgeBaselineManifest = decode(sqlx::query_scalar::<_,Option<serde_json::Value>>(
        "SELECT baseline FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    ).bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
    let completion:KnowledgeCompletionRequirement=decode(sqlx::query_scalar("SELECT c.completion FROM knowledge_change_runs r JOIN knowledge_lifecycle_changes c ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.id=r.change_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?)?;
    let current_generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current_generation != baseline.workspace_generation {
        return Err(Error::ContextChanged);
    }
    for operation in changeset.operations {
        match operation.operation {
            KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise => {
                let identity = baseline
                    .identity_matches
                    .iter()
                    .find(|value| value.client_label == operation.client_label && !value.ambiguous)
                    .ok_or(Error::NeedsContext)?;
                let response = super::context::eligible_unit(
                    tx,
                    tenant,
                    workspace,
                    principal,
                    identity.unit_id,
                    Some(identity.revision),
                )
                .await?;
                let KnowledgeUnitResponse::Document(document) = response else {
                    return Err(Error::NeedsContext);
                };
                if document.revision != identity.revision
                    || Some(&document.document) != operation.document.as_ref()
                {
                    return Err(Error::NeedsContext);
                }
            }
            KnowledgeLifecycleOperation::Retract => {
                let row:Option<(i64,Uuid)>=sqlx::query_as("SELECT h.accepted_revision,h.last_event_id FROM knowledge_unit_heads h WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3 AND h.lifecycle='retracted' AND NOT h.payload_erased AND NOT EXISTS(SELECT 1 FROM knowledge_bindings b WHERE b.tenant_id=h.tenant_id AND b.workspace_id=h.workspace_id AND b.unit_id=h.unit_id AND b.active)").bind(tenant).bind(workspace).bind(operation.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                let Some((revision, event_id)) = row else {
                    return Err(Error::NeedsContext);
                };
                let verified = super::event::verify_publication_event(
                    tx,
                    tenant,
                    workspace,
                    operation.unit_id,
                    revision,
                    event_id,
                    false,
                )
                .await?;
                if verified.input.planned.operation != KnowledgeLifecycleOperation::Retract {
                    return Err(Error::NeedsContext);
                }
            }
            KnowledgeLifecycleOperation::Erase => {
                let row:Option<(String,String,i64)>=sqlx::query_as("SELECT owned_live_copies_status,restore_safe_status,erasure_sequence FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND lifecycle='erased'").bind(tenant).bind(workspace).bind(operation.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                let (owned, restore, sequence) = row.ok_or(Error::NeedsContext)?;
                if owned != "ready"
                    || completion.erasure == KnowledgeErasureRequirement::RestoreSafe
                        && restore != "ready"
                {
                    return Err(Error::NeedsContext);
                }
                let residual =
                    super::erase::residual_owned_unit(tx, tenant, workspace, operation.unit_id)
                        .await?;
                if !residual.complete {
                    return Err(Error::NeedsContext);
                }
                if completion.erasure == KnowledgeErasureRequirement::RestoreSafe {
                    let exported: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM knowledge_suppression_exports e \
                         JOIN durable_knowledge_capability c ON c.singleton \
                         AND c.database_lineage_id=e.database_lineage_id \
                         WHERE e.erasure_sequence >= $1)",
                    )
                    .bind(sequence)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
                    if !exported {
                        return Err(Error::NeedsContext);
                    }
                }
            }
            KnowledgeLifecycleOperation::Revalidate | KnowledgeLifecycleOperation::Supersede => {
                return Err(Error::InvalidArguments);
            }
        }
    }
    Ok(())
}

pub(in crate::knowledge_lifecycle) async fn require_prior_findings(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    current: &KnowledgeReviewReceipt,
) -> Result<()> {
    struct FindingState {
        owner_ref: String,
        revisit_phase_id: KnowledgeChangePhaseId,
        unresolved_output_id: Option<Uuid>,
    }
    let prior: Vec<(Uuid, serde_json::Value)> = sqlx::query_as(
        "SELECT id,output->'data'->'data'->'findings' FROM knowledge_change_outputs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 \
         AND phase_id='kc-review-reconcile' AND NOT payload_erased ORDER BY revision",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut states = BTreeMap::<String, FindingState>::new();
    for (output_id, value) in prior {
        for finding in decode::<Vec<KnowledgeReviewFinding>>(value)? {
            if let Some(state) = states.get_mut(&finding.id) {
                if state.owner_ref != finding.owner_ref
                    || state.revisit_phase_id != finding.revisit_phase_id
                    || (!finding.closed && state.unresolved_output_id.is_none())
                {
                    return Err(Error::InternalInvariant);
                }
                state.unresolved_output_id = (!finding.closed).then_some(output_id);
            } else {
                states.insert(
                    finding.id,
                    FindingState {
                        owner_ref: finding.owner_ref,
                        revisit_phase_id: finding.revisit_phase_id,
                        unresolved_output_id: (!finding.closed).then_some(output_id),
                    },
                );
            }
        }
    }
    for (id, state) in states
        .iter()
        .filter(|(_, state)| state.unresolved_output_id.is_some())
    {
        let finding = current
            .findings
            .iter()
            .find(|finding| &finding.id == id)
            .ok_or(Error::NeedsContext)?;
        if finding.owner_ref != state.owner_ref
            || finding.revisit_phase_id != state.revisit_phase_id
        {
            return Err(Error::NeedsContext);
        }
    }
    if matches!(
        current.outcome,
        KnowledgeReviewOutcome::Ready | KnowledgeReviewOutcome::NoChange
    ) && current.findings.iter().any(|finding| !finding.closed)
    {
        return Err(Error::NeedsContext);
    }
    for finding in current.findings.iter().filter(|finding| finding.closed) {
        if let Some(state) = states.get(&finding.id)
            && (state.owner_ref != finding.owner_ref
                || state.revisit_phase_id != finding.revisit_phase_id)
        {
            return Err(Error::NeedsContext);
        }
        let closure = finding
            .closure_output_digest
            .as_deref()
            .ok_or(Error::NeedsContext)?;
        let after_output = states
            .get(&finding.id)
            .and_then(|state| state.unresolved_output_id);
        let exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM knowledge_change_output_bindings b \
             JOIN knowledge_change_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id \
             WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 \
               AND b.stale=false AND o.payload_erased=false AND o.digest=$5 \
               AND ($6::uuid IS NULL OR o.created_at>(SELECT prior.created_at FROM knowledge_change_outputs prior \
                    WHERE prior.tenant_id=$1 AND prior.workspace_id=$2 AND prior.id=$6)))",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(run)
        .bind(enum_text(&finding.revisit_phase_id)?)
        .bind(closure)
        .bind(after_output)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
        if !exists {
            return Err(Error::NeedsContext);
        }
    }
    Ok(())
}
