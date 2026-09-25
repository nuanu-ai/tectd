async fn load_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
    expected_candidate: Option<Uuid>,
) -> Result<Option<ScopeConstructorManifest>> {
    Ok(
        load_manifest_record(tx, tenant, workspace, opportunity, expected_candidate)
            .await?
            .map(|value| value.record.manifest),
    )
}

fn valid_authored_request_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

async fn load_manifest_by_request_key(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request_key: &str,
) -> Result<Option<StoredScopeManifestRecord>> {
    let opportunity: Option<Uuid> = sqlx::query_scalar(
        "SELECT o.id FROM advisory_opportunity o JOIN advisory_scope_manifest m \
           ON (m.tenant_id,m.workspace_id,m.opportunity_id)=(o.tenant_id,o.workspace_id,o.id) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.request_key=$3 \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' \
           AND o.capability='scope_decomposition' \
           AND o.decision_point='scope.decomposition.before_selection'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request_key)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(opportunity) = opportunity else {
        return Ok(None);
    };
    load_manifest_record(tx, tenant, workspace, opportunity, None)
        .await?
        .map(Some)
        .ok_or(Error::StorageUnavailable)
}

async fn load_manifest_record(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
    expected_candidate: Option<Uuid>,
) -> Result<Option<StoredScopeManifestRecord>> {
    let row: Option<ManifestRow> = sqlx::query_as(
        "SELECT m.candidate_set_id,s.candidate_set_revision,s.snapshot_id,\
                s.config_revision,s.opportunity_material_digest,m.authored_request_digest,m.source_digest,\
                m.constructor_id,m.constructor_version,m.constructor_digest,\
                m.baseline_alternative_id,m.eligible_set_digest,\
                m.whole_set_digest,s.aggregate_payload AS source_payload,\
                m.aggregate_payload AS manifest_payload \
         FROM advisory_scope_manifest m JOIN advisory_scope_source_snapshot s \
           USING(tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest) \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    if expected_candidate.is_some_and(|value| value != row.candidate_set_id) {
        return Err(Error::InputConflict);
    }
    let source: SourceObligationsAggregate =
        serde_json::from_value(row.source_payload).map_err(storage_error)?;
    let manifest: ScopeConstructorManifest =
        serde_json::from_value(row.manifest_payload).map_err(storage_error)?;
    manifest.validate(&Sha256ScopeDigest)?;
    if row
        .authored_request_digest
        .as_deref()
        .is_some_and(|value| !valid_authored_request_digest(value))
    {
        return Err(Error::StorageUnavailable);
    }
    if manifest.source != source.source
        || manifest.obligations != source.obligations
        || manifest.source.candidate_set_id != row.candidate_set_id
        || manifest.source.candidate_set_revision != row.candidate_set_revision
        || manifest.source.snapshot_id != row.snapshot_id
        || manifest.source.digest != row.source_digest
        || manifest.constructor.id != row.constructor_id
        || manifest.constructor.version != row.constructor_version
        || manifest.constructor.digest != row.constructor_digest
        || manifest.baseline_id.0 != row.baseline_alternative_id
        || manifest.eligible_set_digest != row.eligible_set_digest
        || manifest.whole_set_digest != row.whole_set_digest
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(Some(StoredScopeManifestRecord {
        record: ScopeManifestRecord {
            opportunity_id: opportunity,
            candidate_set_id: row.candidate_set_id,
            config_revision: row.config_revision,
            opportunity_material_digest: row.opportunity_material_digest,
            manifest,
        },
        authored_request_digest: row.authored_request_digest,
    }))
}
