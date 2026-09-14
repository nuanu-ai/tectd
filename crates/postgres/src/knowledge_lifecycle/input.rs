use super::*;

fn replay_outcome(value: KnowledgeChangeMutationOutcome) -> KnowledgeChangeMutationOutcome {
    match value {
        KnowledgeChangeMutationOutcome::Advanced(context)
        | KnowledgeChangeMutationOutcome::Replay(context) => {
            KnowledgeChangeMutationOutcome::Replay(context)
        }
    }
}

pub(crate) async fn record_input(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    request: &RecordKnowledgeChangeInput,
) -> Result<KnowledgeChangeMutationOutcome> {
    require_owner(tx, principal).await?;
    let payload = json(request)?;
    if let Some(prior) = replay::<KnowledgeChangeMutationOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "record_input",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    let _ = lock_workspace(tx, tenant, workspace).await?;
    let (revision, status, current) =
        lock_run(tx, tenant, workspace, request.change_id, request.run_id).await?;
    if let Some(prior) = replay::<KnowledgeChangeMutationOutcome>(
        tx,
        tenant,
        workspace,
        principal,
        "record_input",
        request.request_id,
        &payload,
    )
    .await?
    {
        return Ok(replay_outcome(prior));
    }
    if revision != request.run_revision {
        return Err(Error::StaleRevision);
    }
    if !matches!(status.as_str(), "active" | "waiting_input" | "blocked") {
        return Err(Error::Forbidden);
    }
    let (published, payload_erased, terminal_review, has_result): (
        bool,
        bool,
        Option<String>,
        bool,
    ) = sqlx::query_as(
        "SELECT publisher_receipt IS NOT NULL OR erased_publisher_receipt IS NOT NULL, \
         payload_erased,terminal_review_outcome,result IS NOT NULL OR erased_result IS NOT NULL \
         FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if published || payload_erased || has_result {
        return Err(Error::Forbidden);
    }
    let current = phase(current.as_deref().ok_or(Error::InternalInvariant)?)?;
    let definition: KnowledgeChangeDefinition = decode(
        sqlx::query_scalar(
            "SELECT definition FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(request.run_id)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?,
    )?;
    let current_definition = definition
        .phases
        .iter()
        .find(|value| value.id == current)
        .ok_or(Error::InternalInvariant)?;
    let terminal_rework = current == KnowledgeChangePhaseId::KcResultHandoff
        && matches!(terminal_review.as_deref(), Some("no_change" | "rejected"))
        && (KnowledgeChangePhaseId::KcResolveBaseline.ordinal()
            ..=KnowledgeChangePhaseId::KcImpactPlan.ordinal())
            .contains(&request.revisit_phase_id.ordinal());
    if current == KnowledgeChangePhaseId::KcResultHandoff && !terminal_rework {
        return Err(Error::Forbidden);
    }
    if request.basis_amendment.is_some()
        && (request.revisit_phase_id != KnowledgeChangePhaseId::KcResolveBaseline
            || current.ordinal() < KnowledgeChangePhaseId::KcResolveBaseline.ordinal())
    {
        return Err(Error::InvalidArguments);
    }
    if request.revisit_phase_id != current
        && !current_definition
            .allowed_backward_to
            .contains(&request.revisit_phase_id)
    {
        return Err(Error::InvalidArguments);
    }
    let applied_basis_amendment = match &request.basis_amendment {
        Some(amendment) => {
            Some(super::basis::apply(tx, tenant, workspace, request.change_id, amendment).await?)
        }
        None => None,
    };
    let basis_rework = applied_basis_amendment.is_some();
    let input_digest = match &request.basis_amendment {
        Some(amendment) => digest(&(
            &request.reason,
            &request.input,
            request.revisit_phase_id,
            amendment,
        ))?,
        None => digest(&(&request.reason, &request.input, request.revisit_phase_id))?,
    };
    let duplicate: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM knowledge_change_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND digest=$4 AND payload_erased=false)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .bind(&input_digest)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if duplicate {
        return Err(Error::InputConflict);
    }
    let sequence: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(sequence),0)+1 FROM knowledge_change_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let input_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO knowledge_change_inputs \
         (id,tenant_id,workspace_id,run_id,request_id,sequence,revisit_phase_id,reason,input,digest,actor_session_id,applied_basis_amendment) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12)",
    )
    .bind(input_id)
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .bind(request.request_id)
    .bind(sequence)
    .bind(enum_text(&request.revisit_phase_id)?)
    .bind(&request.reason)
    .bind(&request.input)
    .bind(&input_digest)
    .bind(session)
    .bind(applied_basis_amendment.as_ref().map(json).transpose()?)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "UPDATE knowledge_change_output_bindings SET stale=true,stale_reason='new_input',updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_ordinal >= $4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .bind(request.revisit_phase_id.ordinal() as i32)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "UPDATE knowledge_change_runs SET revision=revision+1,status='active',current_phase_id=$4,current_phase_ordinal=$5,\
         delivery_mode='phasewise',\
         baseline=CASE WHEN $6 THEN NULL ELSE baseline END,\
         branch_plan=CASE WHEN $6 THEN NULL ELSE branch_plan END,\
         ready_to_commit=NULL,terminal_review_outcome=NULL,\
         updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .bind(enum_text(&request.revisit_phase_id)?)
    .bind(request.revisit_phase_id.ordinal() as i32)
    .bind(basis_rework)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    super::erase::register_knowledge_change_input_copies(
        tx,
        tenant,
        workspace,
        request.change_id,
        request.run_id,
        input_id,
        request.request_id,
    )
    .await?;
    super::erase::reconcile_change_owned_copies(tx, tenant, workspace, request.change_id).await?;
    let context = load_context(tx, tenant, workspace, request.change_id)
        .await?
        .ok_or(Error::InternalInvariant)?;
    let result = KnowledgeChangeMutationOutcome::Advanced(Box::new(context));
    save_receipt(
        tx,
        tenant,
        workspace,
        principal,
        session,
        "record_input",
        request.request_id,
        &payload,
        &result,
    )
    .await?;
    Ok(result)
}
