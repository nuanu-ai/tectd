#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourceObligationsAggregate {
    source: FrozenScopeSource,
    obligations: Vec<SourceObligation>,
}

#[derive(sqlx::FromRow)]
struct ManifestRow {
    case_id: Uuid,
    candidate_set_id: Uuid,
    candidate_set_revision: i64,
    snapshot_id: Uuid,
    source_digest: String,
    constructor_id: String,
    constructor_version: String,
    constructor_digest: String,
    baseline_alternative_id: String,
    eligible_set_digest: String,
    whole_set_digest: String,
    source_payload: serde_json::Value,
    manifest_payload: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct FrozenAuthorityRow {
    candidate_set_revision: i64,
    current_snapshot_id: Option<Uuid>,
    input_cursor: i64,
    program_id: Uuid,
    program_revision: i64,
    program_current_latest: i64,
    program_latest_input: i64,
    planning_latest_input: i64,
    selected_sources_digest: String,
    method_revision: String,
    method_digest: String,
    registry_revision: String,
    registry_digest: String,
}

async fn require_frozen_authority(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    source: &FrozenScopeSource,
) -> Result<()> {
    let authority: Option<FrozenAuthorityRow> = sqlx::query_as(
        "SELECT c.revision AS candidate_set_revision,c.current_snapshot_id,c.input_cursor,c.program_id,\
                p.revision AS program_revision,p.latest_input AS program_current_latest,\
                s.program_latest_input,s.planning_latest_input,\
                s.selected_sources_digest,s.method_revision,s.method_digest,s.registry_revision,s.registry_digest \
         FROM scope_candidate_sets c JOIN programs p \
           ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
         JOIN scope_candidate_snapshots s \
           ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)=\
              (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 FOR UPDATE OF c",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(authority) = authority else {
        return Err(Error::NotFound);
    };
    if authority.candidate_set_revision != source.candidate_set_revision
        || authority.current_snapshot_id != Some(source.snapshot_id)
        || authority.input_cursor != source.input_cursor
        || authority.program_id != source.program_id
        || authority.program_revision != source.program_revision
        || authority.program_current_latest != source.program_latest_input
        || authority.program_latest_input != source.program_latest_input
        || authority.planning_latest_input != source.planning_latest_input
        || authority.selected_sources_digest != source.selected_sources_digest
        || authority.method_revision != source.method_revision
        || authority.method_digest != source.method_digest
        || authority.registry_revision != source.registry_revision
        || authority.registry_digest != source.registry_digest
    {
        return Err(Error::StaleRevision);
    }
    Ok(())
}

async fn prepare_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopeManifestRecord,
) -> Result<ScopeConstructorManifest> {
    record.manifest.validate(&Sha256ScopeDigest)?;
    let opportunity: Option<(Uuid, i64, String)> = sqlx::query_as(
        "SELECT scope_id,config_revision,material_digest FROM advisory_opportunity \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
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
            record.case_id,
            record.config_revision,
            record.opportunity_material_digest.clone(),
        ))
    {
        return Err(Error::InputConflict);
    }
    let source = &record.manifest.source;
    require_frozen_authority(tx, tenant, workspace, source).await?;
    if let Some(existing) = load_manifest(
        tx,
        tenant,
        workspace,
        record.opportunity_id,
        Some(record.case_id),
    )
    .await?
    {
        return if existing == record.manifest {
            Ok(existing)
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
         (tenant_id,workspace_id,opportunity_id,case_id,config_revision,opportunity_material_digest,\
          candidate_set_id,candidate_set_revision,snapshot_id,source_digest,aggregate_schema,aggregate_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'tect.scope-source-obligations/1',$11)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.case_id)
    .bind(record.config_revision)
    .bind(&record.opportunity_material_digest)
    .bind(source.candidate_set_id)
    .bind(source.candidate_set_revision)
    .bind(source.snapshot_id)
    .bind(&source.digest)
    .bind(source_payload)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO advisory_scope_manifest \
         (tenant_id,workspace_id,opportunity_id,case_id,source_digest,constructor_id,constructor_version,\
          constructor_digest,baseline_alternative_id,eligible_set_digest,whole_set_digest,aggregate_schema,aggregate_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'tect.scope-constructor-manifest/2',$12)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.opportunity_id)
    .bind(record.case_id)
    .bind(&source.digest)
    .bind(&record.manifest.constructor.id)
    .bind(&record.manifest.constructor.version)
    .bind(&record.manifest.constructor.digest)
    .bind(&record.manifest.baseline_id.0)
    .bind(&record.manifest.eligible_set_digest)
    .bind(&record.manifest.whole_set_digest)
    .bind(manifest_payload)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(record.manifest.clone())
}

async fn load_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity: Uuid,
    expected_case: Option<Uuid>,
) -> Result<Option<ScopeConstructorManifest>> {
    let row: Option<ManifestRow> = sqlx::query_as(
        "SELECT m.case_id,s.candidate_set_id,s.candidate_set_revision,s.snapshot_id,m.source_digest,\
                m.constructor_id,m.constructor_version,m.constructor_digest,\
                m.baseline_alternative_id,m.eligible_set_digest,\
                m.whole_set_digest,s.aggregate_payload AS source_payload,\
                m.aggregate_payload AS manifest_payload \
         FROM advisory_scope_manifest m JOIN advisory_scope_source_snapshot s \
           USING(tenant_id,workspace_id,opportunity_id,case_id,source_digest) \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    if expected_case.is_some_and(|value| value != row.case_id) {
        return Err(Error::InputConflict);
    }
    let source: SourceObligationsAggregate =
        serde_json::from_value(row.source_payload).map_err(storage_error)?;
    let manifest: ScopeConstructorManifest =
        serde_json::from_value(row.manifest_payload).map_err(storage_error)?;
    manifest.validate(&Sha256ScopeDigest)?;
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
    Ok(Some(manifest))
}
