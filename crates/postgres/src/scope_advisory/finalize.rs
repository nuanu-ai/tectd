async fn finalize_advice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &GuardedScopeAdviceRecord,
) -> Result<GuardedScopeAdvice> {
    let updated = sqlx::query(
        "UPDATE advisory_opportunity o SET state='advised',primary_reason='provider_response',updated_at=pg_catalog.clock_timestamp() \
         FROM advisory_workspace_config c,advisory_dispatch d \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$4 \
           AND o.state='awaiting_response' AND o.config_revision=$5 \
           AND c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id \
           AND c.revision=$5 AND c.mode='optional' \
           AND d.tenant_id=o.tenant_id AND d.workspace_id=o.workspace_id \
           AND d.opportunity_id=o.id AND d.id=$6 AND d.material_digest=$7 \
           AND d.state='sealed' AND d.send_certainty='sent' AND d.outcome='provider_response'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .bind(record.config_revision)
    .bind(record.dispatch_id)
    .bind(&record.dispatch_material_digest)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::StaleContext);
    }
    persist_advice(tx, tenant, workspace, record).await
}

async fn invalidate_advice_opportunity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
) -> Result<()> {
    let updated = sqlx::query(
        "UPDATE advisory_opportunity SET state='invalidated',primary_reason='configuration_changed',updated_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='awaiting_response'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if updated.rows_affected() != 1 {
        return Err(Error::StaleContext);
    }
    Ok(())
}
