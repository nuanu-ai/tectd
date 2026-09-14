use super::*;
use std::collections::BTreeMap;

type PromotionSliceRow = (i64, String, String, Option<Uuid>, Option<Uuid>, bool);

fn replay_outcome(value: BeginKnowledgeChangeOutcome) -> BeginKnowledgeChangeOutcome {
    match value {
        BeginKnowledgeChangeOutcome::Created(context)
        | BeginKnowledgeChangeOutcome::Replay(context) => {
            BeginKnowledgeChangeOutcome::Replay(context)
        }
    }
}

#[allow(clippy::too_many_arguments)]
async fn begin_erased_no_change(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &BeginKnowledgeChange,
    definition: &KnowledgeChangeDefinition,
    registry: &KnowledgeProfileRegistry,
    proof: &KnowledgeErasedNoChangeProof,
) -> Result<BeginKnowledgeChangeOutcome> {
    let result_phase = definition
        .phases
        .iter()
        .find(|phase| phase.id == KnowledgeChangePhaseId::KcResultHandoff)
        .ok_or(Error::InvalidConfiguration)?;
    let change_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO knowledge_lifecycle_changes \
         (id,tenant_id,workspace_id,request_id,initiator_principal_id,initiator_session_id, \
          owner,status,payload_erased) VALUES($1,$2,$3,$4,$5,$6,$7,'active',true)",
    )
    .bind(change_id)
    .bind(tenant)
    .bind(workspace)
    .bind(request.request_id)
    .bind(principal)
    .bind(session)
    .bind(json(&request.owner)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO knowledge_change_runs \
         (id,tenant_id,workspace_id,change_id,definition,definition_version,definition_digest, \
          registry,registry_version,registry_digest,delivery_mode,status,current_phase_id, \
          current_phase_ordinal,terminal_review_outcome,payload_erased,erased_no_change_proof) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'phasewise','active',$11,$12,'no_change',true,$13)",
    )
    .bind(run_id)
    .bind(tenant)
    .bind(workspace)
    .bind(change_id)
    .bind(json(definition)?)
    .bind(&definition.version)
    .bind(&definition.digest)
    .bind(json(registry)?)
    .bind(&registry.version)
    .bind(&registry.digest)
    .bind(enum_text(&result_phase.id)?)
    .bind(result_phase.ordinal as i32)
    .bind(json(proof)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    for operation in &proof.operations {
        sqlx::query(
            "INSERT INTO knowledge_change_operations \
             (id,tenant_id,workspace_id,change_id,operation,unit_id,expected_revision, \
              expected_lifecycle,payload_erased) VALUES($1,$2,$3,$4,'erase',$5,$6,'erased',true)",
        )
        .bind(operation.operation_id)
        .bind(tenant)
        .bind(workspace)
        .bind(change_id)
        .bind(operation.unit_id)
        .bind(operation.expected_revision)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    if let KnowledgeChangeOwner::PromotionSlice { slice_id, .. } = request.owner {
        sqlx::query(
            "UPDATE native_slices SET knowledge_change_id=$4,knowledge_run_id=$5 \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(slice_id)
        .bind(change_id)
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    let context = load_context(tx, tenant, workspace, change_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = BeginKnowledgeChangeOutcome::Created(Box::new(context));
    sqlx::query(
        "INSERT INTO knowledge_lifecycle_command_receipts \
         (tenant_id,workspace_id,operation,request_id,actor_principal_id,actor_session_id, \
          payload_erased,erased_change_id) VALUES($1,$2,'begin',$3,$4,$5,true,$6)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.request_id)
    .bind(principal)
    .bind(session)
    .bind(change_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn begin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &BeginKnowledgeChange,
    definition: &KnowledgeChangeDefinition,
    registry: &KnowledgeProfileRegistry,
) -> Result<BeginKnowledgeChangeOutcome> {
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(prior) = replay::<BeginKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "begin",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    let _generation = lock_workspace(tx, tenant, workspace).await?;
    if let Some(prior) = replay::<BeginKnowledgeChangeOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "begin",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    if let KnowledgeChangeOwner::PromotionSlice {
        scope_id,
        slice_id,
        slice_revision,
    } = request.owner
    {
        let slice: Option<PromotionSliceRow> = sqlx::query_as(
            "SELECT s.revision,s.state,s.pipeline,s.knowledge_change_id,s.knowledge_run_id, \
                 EXISTS(SELECT 1 FROM slice_pipeline_runs r WHERE r.tenant_id=s.tenant_id \
                    AND r.workspace_id=s.workspace_id AND r.slice_id=s.id) \
                 FROM native_slices s WHERE s.tenant_id=$1 AND s.workspace_id=$2 \
                 AND s.scope_id=$3 AND s.id=$4 FOR UPDATE",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(scope_id)
        .bind(slice_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
        let Some((revision, state, pipeline, linked_change, linked_run, pipeline_run)) = slice
        else {
            return Err(Error::NotFound);
        };
        if revision != slice_revision {
            return Err(Error::StaleRevision);
        }
        if state != "open"
            || pipeline != PipelineKind::PromoteToDurableKnowledge.as_str()
            || linked_change.is_some()
            || linked_run.is_some()
            || pipeline_run
        {
            return Err(Error::Forbidden);
        }
    }
    if let Some(proof) = super::qualify_erased_no_change(tx, tenant, workspace, request).await? {
        return begin_erased_no_change(
            tx, tenant, workspace, principal, session, request, definition, registry, &proof,
        )
        .await;
    }
    let delivery_mode = request.delivery_mode.unwrap_or(definition.default_mode);
    if !definition.allowed_modes.contains(&delivery_mode) {
        return Err(Error::InvalidArguments);
    }
    let first = definition.phases.first().ok_or(Error::InternalInvariant)?;
    if first.id != KnowledgeChangePhaseId::KcIntake {
        return Err(Error::InvalidConfiguration);
    }
    let change_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let operation_ids = request
        .operation_hints
        .iter()
        .map(|hint| (hint.client_label.clone(), Uuid::new_v4()))
        .collect::<BTreeMap<_, _>>();
    let source_pins =
        super::phase_data::resolve_sources(tx, tenant, workspace, change_id, &request.sources)
            .await?
            .into_iter()
            .map(|value| value.pin)
            .collect::<Vec<_>>();
    sqlx::query(
        "INSERT INTO knowledge_lifecycle_changes \
         (id,tenant_id,workspace_id,request_id,initiator_principal_id,initiator_session_id,owner,intent,desired_outcome,sources,source_pins,operation_hints,completion,status) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,'active')",
    )
    .bind(change_id)
    .bind(tenant)
    .bind(workspace)
    .bind(request.request_id)
    .bind(principal)
    .bind(session)
    .bind(json(&request.owner)?)
    .bind(&request.intent)
    .bind(&request.desired_outcome)
    .bind(json(&request.sources)?)
    .bind(json(&source_pins)?)
    .bind(json(&request.operation_hints)?)
    .bind(json(&request.completion)?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO knowledge_change_runs \
         (id,tenant_id,workspace_id,change_id,definition,definition_version,definition_digest,registry,registry_version,registry_digest,delivery_mode,status,current_phase_id,current_phase_ordinal) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'active',$12,$13)",
    )
    .bind(run_id)
    .bind(tenant)
    .bind(workspace)
    .bind(change_id)
    .bind(json(definition)?)
    .bind(&definition.version)
    .bind(&definition.digest)
    .bind(json(registry)?)
    .bind(&registry.version)
    .bind(&registry.digest)
    .bind(enum_text(&delivery_mode)?)
    .bind(enum_text(&first.id)?)
    .bind(first.ordinal as i32)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    for hint in &request.operation_hints {
        let operation_id = operation_ids[&hint.client_label];
        let unit_id = hint.unit_id.unwrap_or_else(Uuid::new_v4);
        let dependencies = hint
            .depends_on_labels
            .iter()
            .map(|label| operation_ids[label])
            .collect::<Vec<_>>();
        sqlx::query(
            "INSERT INTO knowledge_change_operations \
             (id,tenant_id,workspace_id,change_id,client_label,operation,unit_id,expected_revision,expected_lifecycle,reason,authority_basis,dependency_operation_ids) \
             VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
        )
        .bind(operation_id)
        .bind(tenant)
        .bind(workspace)
        .bind(change_id)
        .bind(&hint.client_label)
        .bind(enum_text(&hint.operation)?)
        .bind(unit_id)
        .bind(hint.expected_revision)
        .bind(hint.expected_lifecycle.as_ref().map(enum_text).transpose()?)
        .bind(&hint.reason)
        .bind(&hint.authority_basis)
        .bind(&dependencies)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    if let KnowledgeChangeOwner::PromotionSlice { slice_id, .. } = request.owner {
        sqlx::query(
            "UPDATE native_slices SET knowledge_change_id=$4,knowledge_run_id=$5 \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(slice_id)
        .bind(change_id)
        .bind(run_id)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    let context = load_context(tx, tenant, workspace, change_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let outcome = BeginKnowledgeChangeOutcome::Created(Box::new(context));
    save_receipt(
        tx,
        tenant,
        workspace,
        principal,
        session,
        "begin",
        request.request_id,
        &payload,
        &outcome,
    )
    .await?;
    Ok(outcome)
}
