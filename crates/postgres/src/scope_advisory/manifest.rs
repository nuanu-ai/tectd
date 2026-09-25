#[derive(Debug, serde::Serialize, serde::Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct SourceObligationsAggregate {
    source: FrozenScopeSource,
    obligations: Vec<SourceObligation>,
}

#[derive(sqlx::FromRow)]
struct ManifestRow {
    candidate_set_id: Uuid,
    candidate_set_revision: i64,
    snapshot_id: Uuid,
    config_revision: i64,
    opportunity_material_digest: String,
    authored_request_digest: Option<String>,
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
    candidate_latest_input: i64,
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

#[derive(sqlx::FromRow)]
struct PersistedSourceFragment {
    id: Uuid,
    body_digest: String,
    body: String,
}

async fn load_persisted_fragments(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    candidate_set_id: Uuid,
    snapshot_id: Uuid,
) -> Result<Vec<PersistedSourceFragment>> {
    let fragments: Vec<PersistedSourceFragment> = sqlx::query_as(
        "SELECT r.id,r.body_digest,c.body FROM scope_candidate_source_refs r \
         JOIN scope_candidate_contents c ON (c.tenant_id,c.workspace_id,c.digest)=\
           (r.tenant_id,r.workspace_id,r.body_digest) \
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.candidate_set_id=$3 \
           AND r.snapshot_id=$4 ORDER BY r.id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(candidate_set_id)
    .bind(snapshot_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(fragments)
}

fn source_inputs_and_obligations(
    fragments: Vec<PersistedSourceFragment>,
    snapshot_id: Uuid,
) -> Result<(Vec<FrozenSourceInput>, Vec<SourceObligation>)> {
    let mut expected_inputs = Vec::new();
    let mut expected_obligations = Vec::new();
    for fragment in fragments {
        if fragment.body.trim().is_empty() {
            continue;
        }
        let id = fragment.id.to_string();
        expected_inputs.push(FrozenSourceInput {
            id: id.clone(),
            version: snapshot_id.to_string(),
            digest: fragment.body_digest.clone(),
            provenance: format!("scope_candidate_source_ref:{id}"),
            applicability: SourceApplicability::Applicable,
        });
        expected_obligations.push(SourceObligation {
            id: id.clone(),
            source_input_id: id,
            statement_digest: fragment.body_digest,
            conditions: vec![],
            exceptions: vec![],
        });
    }
    if expected_inputs.is_empty() {
        return Err(Error::InvalidSource);
    }
    expected_inputs.sort_by(|a, b| a.id.cmp(&b.id));
    expected_obligations.sort_by(|a, b| a.id.cmp(&b.id));
    Ok((expected_inputs, expected_obligations))
}

pub(crate) async fn require_persisted_fragments(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    source: &FrozenScopeSource,
    obligations: &[SourceObligation],
) -> Result<()> {
    // The candidate-set revision and current snapshot were checked and locked
    // in this transaction by require_frozen_authority.
    let fragments = load_persisted_fragments(
        tx,
        tenant,
        workspace,
        source.candidate_set_id,
        source.snapshot_id,
    )
    .await?;
    let (expected_inputs, expected_obligations) =
        source_inputs_and_obligations(fragments, source.snapshot_id)?;
    if source.inputs != expected_inputs || obligations != expected_obligations {
        return Err(Error::InvalidSource);
    }
    Ok(())
}

async fn require_frozen_authority(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    source: &FrozenScopeSource,
) -> Result<()> {
    let authority: Option<FrozenAuthorityRow> = sqlx::query_as(
        "SELECT c.revision AS candidate_set_revision,c.current_snapshot_id,c.input_cursor,\
                c.latest_input AS candidate_latest_input,c.program_id,\
                p.revision AS program_revision,p.latest_input AS program_current_latest,\
                s.program_latest_input,s.planning_latest_input,\
                s.selected_sources_digest,s.method_revision,s.method_digest,s.registry_revision,s.registry_digest \
         FROM scope_candidate_sets c JOIN programs p \
           ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
         JOIN scope_candidate_snapshots s \
           ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)=\
              (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 FOR UPDATE OF c,p",
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
        || authority.candidate_latest_input != source.planning_latest_input
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

fn authored_graph_binding(
    manifest: &ScopeConstructorManifest,
) -> Result<(Vec<AntiBloatObligationLink>, String, String)> {
    authored_graph_binding_for(
        manifest,
        &manifest.baseline_id,
        manifest.source.candidate_set_revision,
    )
}

pub(crate) fn authored_graph_binding_for(
    manifest: &ScopeConstructorManifest,
    selected_id: &ScopeAlternativeId,
    selected_revision: i64,
) -> Result<(Vec<AntiBloatObligationLink>, String, String)> {
    if !is_source_authored_identity(&manifest.constructor)
        || (manifest.constructor == legacy_source_authored_identity()
            && (manifest.emitted.iter().any(|alternative| {
                alternative
                    .material
                    .candidates
                    .iter()
                    .any(|candidate| !candidate.grounding.is_source_grounded())
            }) || manifest.rejected.iter().any(|rejected| {
                rejected
                    .alternative
                    .material
                    .candidates
                    .iter()
                    .any(|candidate| !candidate.grounding.is_source_grounded())
            })))
    {
        return Err(Error::InvalidSource);
    }
    let selected = manifest.eligible(selected_id).ok_or(Error::InvalidSource)?;
    if selected_revision < manifest.source.candidate_set_revision {
        return Err(Error::StaleRevision);
    }
    let mut obligations = std::collections::BTreeSet::new();
    for obligation in &manifest.obligations {
        if obligation.id != obligation.source_input_id
            || Uuid::parse_str(&obligation.id).is_err()
            || !obligations.insert(obligation.id.clone())
        {
            return Err(Error::InvalidSource);
        }
    }
    let mut links = Vec::new();
    let mut covered = std::collections::BTreeSet::new();
    for goal in &selected.material.goals {
        let source_id = goal.source_ref_id.to_string();
        if !obligations.contains(&source_id) || !covered.insert((source_id.clone(), goal.id)) {
            return Err(Error::InvalidSource);
        }
        links.push(AntiBloatObligationLink {
            obligation_id: source_id,
            goal_id: goal.id,
        });
    }
    if covered
        .iter()
        .map(|(id, _)| id)
        .collect::<std::collections::BTreeSet<_>>()
        .len()
        != obligations.len()
    {
        return Err(Error::InvalidSource);
    }
    links.sort_by(|a, b| (&a.obligation_id, a.goal_id).cmp(&(&b.obligation_id, b.goal_id)));

    let mut dependencies = selected
        .material
        .candidates
        .iter()
        .map(|candidate| {
            let mut ids = candidate.dependencies.clone();
            ids.sort();
            (candidate.id, ids)
        })
        .collect::<Vec<_>>();
    dependencies.sort_by_key(|(id, _)| *id);
    let digest_payload = serde_json::to_vec(&(
        "tect.anti-bloat-dependency-graph/1",
        &manifest.source.digest,
        &manifest.whole_set_digest,
        &selected.material_digest,
        selected_revision,
        &dependencies,
    ))
    .map_err(storage_error)?;
    let dependency_digest = format!("{:x}", sha2::Sha256::digest(digest_payload));
    let provenance = format!(
        "tect.source-authored-graph-binding/1:source={}:whole={}:material={}:revision={}:dependencies={}",
        manifest.source.digest,
        manifest.whole_set_digest,
        selected.material_digest,
        selected_revision,
        dependency_digest,
    );
    Ok((links, dependency_digest, provenance))
}

async fn insert_selected_graph_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: &ScopeConstructorManifest,
    opportunity_id: Uuid,
    selected_id: &ScopeAlternativeId,
    selected_revision: i64,
    caller_link_id: Uuid,
    caller_request_id: Uuid,
) -> Result<()> {
    let selected = manifest.eligible(selected_id).ok_or(Error::InvalidSource)?;
    let (links, dependency_digest, provenance) =
        authored_graph_binding_for(manifest, selected_id, selected_revision)?;
    let provenance = format!(
        "{provenance}:selected={}:caller={caller_link_id}:receipt={caller_request_id}",
        selected_id.0
    );
    let saved_payload: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND set_revision=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest.source.candidate_set_id)
    .bind(selected_revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .flatten();
    let saved: ResolvedCandidateDraft =
        serde_json::from_value(saved_payload.ok_or(Error::InputConflict)?)
            .map_err(storage_error)?;
    if saved != selected.material
        || scope_candidate_material_digest(&Sha256ScopeDigest, &saved)? != selected.material_digest
    {
        return Err(Error::InputConflict);
    }
    sqlx::query(
        "INSERT INTO scope_anti_bloat_bindings \
         (tenant_id,workspace_id,candidate_set_id,opportunity_id,candidate_set_revision, \
          source_digest,dependency_digest,obligation_links,mandatory_policy_obligation_ids,provenance, \
          selected_draft_revision,selected_material_digest,selected_alternative_id, \
          selected_caller_link_id,selected_caller_request_id) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest.source.candidate_set_id)
    .bind(opportunity_id)
    .bind(selected_revision)
    .bind(&manifest.source.digest)
    .bind(dependency_digest)
    .bind(serde_json::to_value(links).map_err(storage_error)?)
    .bind(serde_json::json!([]))
    .bind(provenance)
    .bind(selected_revision)
    .bind(&selected.material_digest)
    .bind(&selected_id.0)
    .bind(caller_link_id)
    .bind(caller_request_id)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn insert_authored_graph_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopeManifestRecord,
) -> Result<()> {
    let (links, dependency_digest, provenance) = authored_graph_binding(&record.manifest)?;
    sqlx::query(
        "INSERT INTO scope_anti_bloat_bindings \
         (tenant_id,workspace_id,candidate_set_id,opportunity_id,candidate_set_revision, \
          source_digest,dependency_digest,obligation_links,mandatory_policy_obligation_ids,provenance) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.candidate_set_id)
    .bind(record.opportunity_id)
    .bind(record.manifest.source.candidate_set_revision)
    .bind(&record.manifest.source.digest)
    .bind(dependency_digest)
    .bind(serde_json::to_value(links).map_err(storage_error)?)
    .bind(serde_json::json!([]))
    .bind(provenance)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn require_authored_graph_binding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    record: &ScopeManifestRecord,
) -> Result<()> {
    let (links, dependency_digest, provenance) = authored_graph_binding(&record.manifest)?;
    let actual: Option<(serde_json::Value, serde_json::Value, String, String)> = sqlx::query_as(
        "SELECT obligation_links,mandatory_policy_obligation_ids,dependency_digest,provenance \
         FROM scope_anti_bloat_bindings WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND candidate_set_revision=$4 AND opportunity_id=$5 \
         AND source_digest=$6",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(record.candidate_set_id)
    .bind(record.manifest.source.candidate_set_revision)
    .bind(record.opportunity_id)
    .bind(&record.manifest.source.digest)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if actual
        != Some((
            serde_json::to_value(links).map_err(storage_error)?,
            serde_json::json!([]),
            dependency_digest,
            provenance,
        ))
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(())
}

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

#[cfg(test)]
mod authored_graph_binding_tests {
    use super::*;
    use tect_domain::{CandidateGrounding, ExploratoryProvenance};

    fn manifest(sources: &[Uuid]) -> ScopeConstructorManifest {
        let fragments = sources
            .iter()
            .map(|id| (*id, crate::scope_advisory::live_support::D))
            .collect::<Vec<_>>();
        let mut value = crate::scope_advisory::live_support::manifest(
            Uuid::new_v4(),
            Uuid::new_v4(),
            Uuid::new_v4(),
            &fragments,
        );
        value.constructor = source_authored_identity();
        crate::scope_advisory::live_support::reseal_manifest(&mut value);
        value
    }

    #[test]
    fn binding_derives_exact_source_goal_and_stable_dependency_graph() {
        let source = Uuid::new_v4();
        let value = manifest(&[source]);
        let (links, digest, provenance) = authored_graph_binding(&value).unwrap();
        assert_eq!(
            links,
            vec![AntiBloatObligationLink {
                obligation_id: source.to_string(),
                goal_id: value.emitted[0].material.goals[0].id,
            }]
        );
        assert_eq!(digest.len(), 64);
        assert!(provenance.contains(&value.emitted[0].material_digest));
        assert_eq!(
            authored_graph_binding(&value).unwrap(),
            (links, digest.clone(), provenance)
        );

        let mut changed = value.clone();
        changed.emitted[0].material.candidates[0]
            .dependencies
            .push(Uuid::new_v4());
        assert_ne!(authored_graph_binding(&changed).unwrap().1, digest);
    }

    #[test]
    fn binding_refuses_missing_unmatched_and_duplicate_obligations() {
        let source = Uuid::new_v4();
        let mut value = manifest(&[source, Uuid::new_v4()]);
        assert_eq!(
            authored_graph_binding(&value).unwrap_err(),
            Error::InvalidSource
        );

        value = manifest(&[source]);
        value.emitted[0].material.goals[0].source_ref_id = Uuid::new_v4();
        assert_eq!(
            authored_graph_binding(&value).unwrap_err(),
            Error::InvalidSource
        );

        value = manifest(&[source]);
        value.obligations.push(value.obligations[0].clone());
        assert_eq!(
            authored_graph_binding(&value).unwrap_err(),
            Error::InvalidSource
        );
    }

    #[test]
    fn v2_exploratory_mechanism_preserves_source_obligations_and_v1_refuses_it() {
        let source = Uuid::new_v4();
        let mut value = manifest(&[source]);
        let original = authored_graph_binding(&value).unwrap().0;
        let mut exploratory = value.emitted[0].material.candidates[0].clone();
        exploratory.id = Uuid::new_v4();
        exploratory.grounding = CandidateGrounding::ExploratoryUnrequested {
            provenance: ExploratoryProvenance::SourceAuthoredV2,
        };
        exploratory.coverage_goal_ids.clear();
        value.emitted[0]
            .material
            .delta
            .added
            .push(tect_domain::CandidateAdded {
                candidate_id: exploratory.id,
                revision: exploratory.revision,
            });
        value.emitted[0].material.candidates.push(exploratory);
        assert!(
            value.emitted[0].material.validate().is_ok(),
            "{:?}",
            value.emitted[0].material.validate()
        );
        assert_eq!(authored_graph_binding(&value).unwrap().0, original);
        value.constructor = legacy_source_authored_identity();
        assert_eq!(
            authored_graph_binding(&value).unwrap_err(),
            Error::InvalidSource
        );
    }
}
