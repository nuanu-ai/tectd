async fn cas_disposition(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: ScopeDispositionRecord,
) -> Result<ScopeDispositionRevision> {
    // Request IDs are unique across the workspace, including across different advice IDs.
    // Serialize replay checks before the advice-specific revision lock.
    lock_scope_key(
        tx,
        tenant,
        workspace,
        "disposition-request",
        record.request.request_id,
    )
    .await?;
    lock_scope_key(
        tx,
        tenant,
        workspace,
        "disposition",
        &record.request.advice_id.0,
    )
    .await?;
    let replay: Option<DispositionRow> = sqlx::query_as(
        "SELECT opportunity_id,candidate_set_id,actor_id,session_id,disposition_id,request_id,advice_id,\
                revision,predecessor_id,action,selected_alternative_id,aggregate_payload \
         FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.request.request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if replay.as_ref().is_some_and(|row| {
        row.opportunity_id != record.opportunity_id
            || row.candidate_set_id != record.candidate_set_id
            || row.advice_id != record.request.advice_id.0
            || row.actor_id != record.actor_id
            || row.session_id != record.session_id
    }) {
        return Err(Error::InputConflict);
    }
    let identity: Option<(Uuid, Uuid, i64)> = sqlx::query_as(
        "SELECT o.authorized_actor_id,o.session_id,a.config_revision FROM advisory_scope_advice a \
         JOIN advisory_opportunity o USING(tenant_id,workspace_id) \
         WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.opportunity_id=$3 \
           AND a.candidate_set_id=$4 AND o.id=a.opportunity_id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((actor_id, session_id, config_revision)) = identity else {
        return Err(Error::NotFound);
    };
    if (actor_id, session_id) != (record.actor_id, record.session_id) {
        return Err(Error::InputConflict);
    }
    require_current_opportunity_config(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        record.candidate_set_id,
        config_revision,
    )
    .await?;
    if let Some(row) = replay {
        let revision = load_disposition(tx, tenant, workspace, row).await?;
        let request = &record.request;
        let same = revision.advice_id == request.advice_id
            && revision.revision.checked_sub(1) == Some(request.expected_revision)
            && revision.action == request.action
            && revision.selected_id == request.selected_id
            && revision.items == request.items
            && revision.rationale == request.rationale;
        return if same {
            Ok(revision)
        } else {
            Err(Error::InputConflict)
        };
    }
    let manifest = load_manifest(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        Some(record.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    let (_, advice) = load_advice(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        Some(record.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    let current_row: Option<DispositionRow> = sqlx::query_as(
        "SELECT opportunity_id,candidate_set_id,actor_id,session_id,disposition_id,request_id,advice_id,\
                revision,predecessor_id,action,selected_alternative_id,aggregate_payload \
         FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 AND advice_id=$3 \
         ORDER BY revision DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(&advice.id.0)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let current = match current_row {
        Some(row) => Some(load_disposition(tx, tenant, workspace, row).await?),
        None => None,
    };
    let revision = record.request.into_revision(
        Uuid::new_v4(),
        &Sha256ScopeDigest,
        &manifest,
        &advice,
        current.as_ref(),
    )?;
    let payload = serde_json::to_value(&revision).map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO advisory_scope_disposition \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,disposition_id,request_id,advice_id,revision,\
          predecessor_id,predecessor_revision,actor_id,session_id,action,selected_alternative_id,\
          aggregate_schema,aggregate_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,\
                'tect.scope-disposition-revision/1',$15)",
    )
    .bind(tenant).bind(workspace).bind(record.opportunity_id).bind(record.candidate_set_id)
    .bind(revision.id).bind(revision.request_id).bind(&revision.advice_id.0).bind(revision.revision)
    .bind(revision.supersedes_id).bind(current.as_ref().map(|value| value.revision))
    .bind(record.actor_id).bind(record.session_id).bind(disposition_action(revision.action))
    .bind(revision.selected_id.as_ref().map(|id| &id.0)).bind(payload)
    .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(revision)
}

async fn persist_preservation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    input: &ScopePreservationReceiptInput,
) -> Result<Uuid> {
    lock_scope_key(tx, tenant, workspace, "preservation", input.disposition_id).await?;
    let existing: Option<(Uuid,Uuid,Uuid,String,Uuid,serde_json::Value,serde_json::Value)> = sqlx::query_as(
        "SELECT receipt_id,opportunity_id,candidate_set_id,advice_id,disposition_id,observation_payload,result_payload FROM advisory_scope_preservation_receipt \
         WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    ).bind(tenant).bind(workspace).bind(input.request_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if existing.as_ref().is_some_and(|row| {
        row.0 != input.receipt_id
            || row.1 != input.opportunity_id
            || row.2 != input.candidate_set_id
            || row.3 != input.observation.advice_id.0
            || row.4 != input.disposition_id
    }) {
        return Err(Error::InputConflict);
    }
    let row: DispositionRow = sqlx::query_as(
        "SELECT opportunity_id,candidate_set_id,actor_id,session_id,disposition_id,request_id,advice_id,\
                revision,predecessor_id,action,selected_alternative_id,aggregate_payload \
         FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 \
           AND disposition_id=$3 AND opportunity_id=$4 AND candidate_set_id=$5",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(input.disposition_id)
    .bind(input.opportunity_id)
    .bind(input.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    let disposition = load_disposition(tx, tenant, workspace, row).await?;
    let manifest = load_manifest(
        tx,
        tenant,
        workspace,
        input.opportunity_id,
        Some(input.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    let (advice_header, advice) = load_advice(
        tx,
        tenant,
        workspace,
        input.opportunity_id,
        Some(input.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    require_current_opportunity_config(
        tx,
        tenant,
        workspace,
        input.opportunity_id,
        input.candidate_set_id,
        advice_header.config_revision,
    )
    .await?;
    require_frozen_authority(tx, tenant, workspace, &input.observation.source).await?;
    let derived = evaluate_scope_preservation(
        &Sha256ScopeDigest,
        &manifest,
        &advice,
        &disposition,
        &input.observation,
    )?;
    if derived != input.result {
        return Err(Error::InputConflict);
    }
    let observation_payload = serde_json::to_value(&input.observation).map_err(storage_error)?;
    let result_payload = serde_json::to_value(&derived).map_err(storage_error)?;
    if let Some(row) = existing {
        return if row
            == (
                input.receipt_id,
                input.opportunity_id,
                input.candidate_set_id,
                advice.id.0.clone(),
                input.disposition_id,
                observation_payload,
                result_payload,
            ) {
            Ok(row.0)
        } else {
            Err(Error::InputConflict)
        };
    }
    sqlx::query(
        "INSERT INTO advisory_scope_preservation_receipt \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,receipt_id,request_id,advice_id,disposition_id,\
          disposition_revision,source_digest,manifest_digest,eligible_set_digest,\
          observed_candidate_set_revision,status,aggregate_schema,observation_payload,result_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,'tect.scope-preservation/1',$15,$16)",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id)
      .bind(input.receipt_id).bind(input.request_id).bind(&advice.id.0).bind(input.disposition_id)
      .bind(disposition.revision).bind(&manifest.source.digest).bind(&manifest.whole_set_digest)
      .bind(&manifest.eligible_set_digest).bind(input.observation.candidate_set_revision)
      .bind(preservation_status(&derived.status)).bind(observation_payload).bind(result_payload)
      .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(input.receipt_id)
}
