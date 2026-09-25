async fn prepare_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopeManifestRecord,
    authored_request_digest: Option<&str>,
) -> Result<ScopeConstructorManifest> {
    record.manifest.validate(&Sha256ScopeDigest)?;
    if authored_request_digest.is_some_and(|value| !valid_authored_request_digest(value)) {
        return Err(Error::InvalidArguments);
    }
    let opportunity: Option<(Option<Uuid>, Option<String>, i64, String)> = sqlx::query_as(
        "SELECT work_item_id,source_revision,config_revision,material_digest FROM advisory_opportunity \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
         AND scope_id IS NULL AND work_item_kind='scope_candidate_set' \
         AND capability='scope_decomposition' \
         AND decision_point='scope.decomposition.before_selection' \
         AND state='prepared' AND primary_reason='dispatch_authorized' FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if opportunity
        != Some((
            Some(record.candidate_set_id),
            Some(record.manifest.source.candidate_set_revision.to_string()),
            record.config_revision,
            record.opportunity_material_digest.clone(),
        ))
    {
        return Err(Error::InputConflict);
    }
    let source = &record.manifest.source;
    if source.candidate_set_id != record.candidate_set_id {
        return Err(Error::InputConflict);
    }
    require_frozen_authority(tx, tenant, workspace, source).await?;
    require_persisted_fragments(tx, tenant, workspace, source, &record.manifest.obligations)
        .await?;
    if let Some(existing) = load_manifest_record(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        Some(record.candidate_set_id),
    )
    .await?
    {
        return if existing.record.manifest == record.manifest
            && existing.authored_request_digest.as_deref() == authored_request_digest
        {
            if authored_request_digest.is_some() {
                require_authored_graph_binding(tx, tenant, workspace, &existing.record).await?;
            }
            Ok(existing.record.manifest)
        } else {
            Err(Error::InputConflict)
        };
    }

    let aggregate = SourceObligationsAggregate {
        source: record.manifest.source.clone(),
        obligations: record.manifest.obligations.clone(),
    };
    let source_payload = serde_json::to_value(&aggregate).map_err(storage_error)?;
    let manifest_payload = serde_json::to_value(&record.manifest).map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO advisory_scope_source_snapshot \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,config_revision,opportunity_material_digest,\
          candidate_set_revision,snapshot_id,source_digest,aggregate_schema,aggregate_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,'tect.scope-source-obligations/1',$10)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .bind(record.config_revision)
    .bind(&record.opportunity_material_digest)
    .bind(source.candidate_set_revision)
    .bind(source.snapshot_id)
    .bind(&source.digest)
    .bind(source_payload)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO advisory_scope_manifest \
         (tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest,constructor_id,constructor_version,\
          constructor_digest,baseline_alternative_id,eligible_set_digest,whole_set_digest,authored_request_digest,\
          aggregate_schema,aggregate_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'tect.scope-constructor-manifest/2',$13)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.candidate_set_id)
    .bind(&source.digest)
    .bind(&record.manifest.constructor.id)
    .bind(&record.manifest.constructor.version)
    .bind(&record.manifest.constructor.digest)
    .bind(&record.manifest.baseline_id.0)
    .bind(&record.manifest.eligible_set_digest)
    .bind(&record.manifest.whole_set_digest)
    .bind(authored_request_digest)
    .bind(manifest_payload)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if authored_request_digest.is_some() {
        let saved = load_manifest_record(
            tx,
            tenant,
            workspace,
            record.opportunity_id,
            Some(record.candidate_set_id),
        )
        .await?
        .ok_or(Error::StorageUnavailable)?;
        if saved.record != *record {
            return Err(Error::StorageUnavailable);
        }
        insert_authored_graph_binding(tx, tenant, workspace, &saved.record).await?;
    }
    Ok(record.manifest.clone())
}
