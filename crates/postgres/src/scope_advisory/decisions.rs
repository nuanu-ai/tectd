async fn persist_advice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &GuardedScopeAdviceRecord,
) -> Result<GuardedScopeAdvice> {
    lock_scope_key(tx, tenant, workspace, "advice", record.opportunity_id).await?;
    let manifest = load_manifest(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        Some(record.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    validate_guarded_advice_binding(&Sha256ScopeDigest, &manifest, &record.advice)?;
    if record.advice.opportunity_id != Some(record.opportunity_id) {
        return Err(Error::InputConflict);
    }
    require_current_opportunity_config(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        record.candidate_set_id,
        record.config_revision,
    )
    .await?;
    if let Some((header, advice)) = load_advice(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        Some(record.candidate_set_id),
    )
    .await?
    {
        return if header.dispatch_id == record.dispatch_id
            && header.dispatch_material_digest == record.dispatch_material_digest
            && header.config_revision == record.config_revision
            && advice == record.advice
        {
            Ok(advice)
        } else {
            Err(Error::InputConflict)
        };
    }
    let advice = &record.advice;
    let payload = serde_json::to_value(advice).map_err(storage_error)?;
    let inserted = sqlx::query(
        "INSERT INTO advisory_scope_advice \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,advice_id,dispatch_id,dispatch_material_digest,\
          config_revision,source_digest,manifest_digest,eligible_set_digest,request_digest,\
          normalized_answers_digest,aggregate_schema,aggregate_payload) \
         SELECT $1,$2,$3,$4,$5,d.id,d.material_digest,$6,$7,$8,$9,$10,$11,\
                'tect.guarded-scope-advice/1',$12 \
         FROM advisory_dispatch d JOIN advisory_opportunity o \
           ON (o.tenant_id,o.workspace_id,o.id)=(d.tenant_id,d.workspace_id,d.opportunity_id) \
         WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=$3 AND d.id=$13 \
           AND d.material_digest=$14 AND d.state='sealed' AND d.send_certainty='sent' \
           AND d.outcome='provider_response' AND d.response_payload IS NOT NULL AND d.sealed_at IS NOT NULL \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' \
           AND o.work_item_id=$4 AND EXISTS (SELECT 1 FROM advisory_scope_source_snapshot s \
               WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.opportunity_id=$3 \
               AND s.candidate_set_id=$4 AND o.source_revision=s.candidate_set_revision::text) \
           AND o.config_revision=$6 AND o.material_digest=$14 \
           AND o.state='advised' AND o.primary_reason='provider_response' FOR UPDATE OF d,o",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .bind(&advice.id.0)
    .bind(record.config_revision)
    .bind(&advice.source_digest)
    .bind(&advice.manifest_digest)
    .bind(&advice.eligible_set_digest)
    .bind(&advice.request_digest)
    .bind(&advice.normalized_answers_digest)
    .bind(payload)
    .bind(record.dispatch_id)
    .bind(&record.dispatch_material_digest)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if inserted.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(advice.clone())
}

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

async fn persist_caller_link(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    input: &ScopeCallerLinkInput,
) -> Result<Uuid> {
    lock_scope_key(
        tx,
        tenant,
        workspace,
        "caller",
        input.preservation_receipt_id,
    )
    .await?;
    let (advice_header, _) = load_advice(
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
    let lineage: Option<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT p.disposition_id,p.receipt_id FROM advisory_scope_preservation_receipt p \
         WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND p.opportunity_id=$3 AND p.candidate_set_id=$4 \
           AND p.receipt_id=$5 AND p.status='passed'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(input.opportunity_id)
    .bind(input.candidate_set_id)
    .bind(input.preservation_receipt_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if lineage != Some((input.disposition_id, input.preservation_receipt_id)) {
        return Err(Error::InputConflict);
    }
    let result_revision: Option<i64> = sqlx::query_scalar(
        "SELECT result_revision FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND operation=$4 AND request_id=$5",
    ).bind(tenant).bind(workspace).bind(input.candidate_set_id).bind(&input.caller_operation)
      .bind(input.caller_request_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if result_revision != Some(input.caller_result_revision)
        || !actor_session_exists(tx, tenant, workspace, input.actor_id, input.session_id).await?
    {
        return Err(Error::InputConflict);
    }
    let existing: Option<(Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,String,Uuid,i64,Uuid,Uuid)> = sqlx::query_as(
        "SELECT link_id,request_id,opportunity_id,candidate_set_id,disposition_id,preservation_receipt_id,caller_operation,\
                caller_request_id,caller_result_revision,actor_id,session_id FROM advisory_scope_caller_link \
         WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    ).bind(tenant).bind(workspace).bind(input.request_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let expected = (
        input.link_id,
        input.request_id,
        input.opportunity_id,
        input.candidate_set_id,
        input.disposition_id,
        input.preservation_receipt_id,
        input.caller_operation.clone(),
        input.caller_request_id,
        input.caller_result_revision,
        input.actor_id,
        input.session_id,
    );
    if let Some(row) = existing {
        return if row == expected {
            Ok(row.0)
        } else {
            Err(Error::InputConflict)
        };
    }
    sqlx::query(
        "INSERT INTO advisory_scope_caller_link \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,link_id,request_id,disposition_id,\
          preservation_receipt_id,caller_operation,caller_request_id,caller_result_revision,actor_id,session_id) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id).bind(input.link_id)
      .bind(input.request_id).bind(input.disposition_id).bind(input.preservation_receipt_id)
      .bind(&input.caller_operation).bind(input.caller_request_id)
      .bind(input.caller_result_revision).bind(input.actor_id).bind(input.session_id)
      .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(input.link_id)
}

async fn persist_verifier(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    input: &ScopeVerifierReceiptInput,
) -> Result<Uuid> {
    lock_scope_key(tx, tenant, workspace, "verifier", input.caller_link_id).await?;
    let identities: Option<(Uuid,Uuid,Uuid,Uuid,Uuid,i64)> = sqlx::query_as(
        "SELECT c.actor_id,c.session_id,d.actor_id,d.session_id,c.candidate_set_id,c.caller_result_revision \
         FROM advisory_scope_caller_link c JOIN advisory_scope_disposition d \
           ON (d.tenant_id,d.workspace_id,d.disposition_id)=(c.tenant_id,c.workspace_id,c.disposition_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.opportunity_id=$3 AND c.candidate_set_id=$4 \
           AND c.link_id=$5",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id)
      .bind(input.caller_link_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((caller_actor, caller_session, decision_actor, decision_session, candidate, revision)) =
        identities
    else {
        return Err(Error::NotFound);
    };
    if candidate != input.candidate_set_id
        || revision != input.verified_revision
        || !actor_session_exists(tx, tenant, workspace, input.actor_id, input.session_id).await?
        || (input.actor_id == caller_actor && input.session_id == caller_session)
        || (input.actor_id == decision_actor && input.session_id == decision_session)
    {
        return Err(Error::InputConflict);
    }
    let existing: Option<(Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,i64,String)> = sqlx::query_as(
        "SELECT receipt_id,request_id,opportunity_id,candidate_set_id,caller_link_id,actor_id,session_id,verified_revision,verifier_digest \
         FROM advisory_scope_verifier_receipt WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
    ).bind(tenant).bind(workspace).bind(input.request_id)
      .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let expected = (
        input.receipt_id,
        input.request_id,
        input.opportunity_id,
        input.candidate_set_id,
        input.caller_link_id,
        input.actor_id,
        input.session_id,
        input.verified_revision,
        input.verifier_digest.clone(),
    );
    if let Some(row) = existing {
        return if row == expected {
            Ok(row.0)
        } else {
            Err(Error::InputConflict)
        };
    }
    sqlx::query(
        "INSERT INTO advisory_scope_verifier_receipt \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,receipt_id,request_id,caller_link_id,\
          actor_id,session_id,verified_revision,verifier_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)",
    ).bind(tenant).bind(workspace).bind(input.opportunity_id).bind(input.candidate_set_id)
      .bind(input.receipt_id).bind(input.request_id).bind(input.caller_link_id)
      .bind(input.actor_id).bind(input.session_id)
      .bind(input.verified_revision)
      .bind(&input.verifier_digest).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(input.receipt_id)
}
