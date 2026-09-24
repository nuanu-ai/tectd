#[derive(sqlx::FromRow)]
struct AuthoredScopeSourceRow {
    candidate_set_id: Uuid,
    source_digest: String,
    aggregate_payload: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct CurrentScopeAuthorityRow {
    candidate_set_revision: i64,
    current_snapshot_id: Uuid,
    input_cursor: i64,
    candidate_latest_input: i64,
    program_id: Uuid,
    program_revision: i64,
    program_current_latest: i64,
    program_payload_erased: bool,
    snapshot_program_revision: i64,
    snapshot_program_latest: i64,
    planning_latest_input: i64,
    selected_sources_digest: String,
    method_revision: String,
    method_digest: String,
    registry_revision: String,
    registry_digest: String,
}

async fn authored_scope_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunity_id: Uuid,
    expected_candidate_set_id: Option<Uuid>,
) -> Result<Option<tect_domain::FrozenScopeSource>> {
    let row: Option<AuthoredScopeSourceRow> = sqlx::query_as(
        "SELECT m.candidate_set_id,s.source_digest,s.aggregate_payload \
         FROM advisory_scope_manifest m \
         JOIN advisory_scope_source_snapshot s \
           USING(tenant_id,workspace_id,opportunity_id,candidate_set_id,source_digest) \
         JOIN advisory_opportunity o \
           ON (o.tenant_id,o.workspace_id,o.id)=(m.tenant_id,m.workspace_id,m.opportunity_id) \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.opportunity_id=$3 \
           AND m.authored_request_digest IS NOT NULL \
           AND o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' \
           AND o.work_item_id=m.candidate_set_id \
           AND o.capability='scope_decomposition' \
           AND o.decision_point='scope.decomposition.before_selection'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if expected_candidate_set_id.is_some_and(|id| id != row.candidate_set_id) {
        return Err(Error::StorageUnavailable);
    }
    let source_value = row
        .aggregate_payload
        .get("source")
        .cloned()
        .ok_or(Error::StorageUnavailable)?;
    let source: tect_domain::FrozenScopeSource =
        serde_json::from_value(source_value).map_err(storage_error)?;
    source
        .validate(&tect_application::Sha256ScopeDigest)
        .map_err(|_| Error::StorageUnavailable)?;
    if source.candidate_set_id != row.candidate_set_id || source.digest != row.source_digest {
        return Err(Error::StorageUnavailable);
    }
    Ok(Some(source))
}

async fn require_current_authored_scope_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    source: &tect_domain::FrozenScopeSource,
) -> Result<()> {
    let authority: Option<CurrentScopeAuthorityRow> = sqlx::query_as(
        "SELECT c.revision AS candidate_set_revision,c.current_snapshot_id,c.input_cursor,\
                c.latest_input AS candidate_latest_input,c.program_id,\
                p.revision AS program_revision,p.latest_input AS program_current_latest,\
                p.payload_erased AS program_payload_erased,\
                s.program_revision AS snapshot_program_revision,\
                s.program_latest_input AS snapshot_program_latest,s.planning_latest_input,\
                s.selected_sources_digest,s.method_revision,s.method_digest,\
                s.registry_revision,s.registry_digest \
         FROM scope_candidate_sets c JOIN programs p \
           ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
         JOIN scope_candidate_snapshots s \
           ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)=\
              (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 \
         FOR UPDATE OF c,p",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(authority) = authority else {
        return Err(Error::StaleContext);
    };
    if authority.candidate_set_revision != source.candidate_set_revision
        || authority.current_snapshot_id != source.snapshot_id
        || authority.input_cursor != source.input_cursor
        || authority.candidate_latest_input != source.planning_latest_input
        || source.input_cursor != source.planning_latest_input
        || authority.program_id != source.program_id
        || authority.program_revision != source.program_revision
        || authority.program_current_latest != source.program_latest_input
        || authority.program_payload_erased
        || authority.snapshot_program_revision != source.program_revision
        || authority.snapshot_program_latest != source.program_latest_input
        || authority.planning_latest_input != source.planning_latest_input
        || authority.selected_sources_digest != source.selected_sources_digest
        || authority.method_revision != source.method_revision
        || authority.method_digest != source.method_digest
        || authority.registry_revision != source.registry_revision
        || authority.registry_digest != source.registry_digest
    {
        return Err(Error::StaleContext);
    }
    Ok(())
}
