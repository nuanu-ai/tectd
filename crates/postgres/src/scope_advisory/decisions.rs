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
