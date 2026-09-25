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

#[derive(Clone, sqlx::FromRow)]
struct PersistedSourceFragment {
    id: Uuid,
    kind: String,
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
        "SELECT r.id,r.kind,r.body_digest,c.body FROM scope_candidate_source_refs r \
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

/// Re-read the exact frozen source and its native citation kinds. The source
/// digest covers IDs and bodies; the immutable typed partition also prevents
/// a changed kind from silently converting required context into a goal.
pub(crate) async fn trusted_non_goal_source_obligation_ids(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: &ScopeConstructorManifest,
    boundary: tect_domain::CandidateBoundary,
) -> Result<Vec<String>> {
    let fragments = load_persisted_fragments(
        tx,
        tenant,
        workspace,
        manifest.source.candidate_set_id,
        manifest.source.snapshot_id,
    )
    .await?;
    let (inputs, obligations) =
        source_inputs_and_obligations(fragments.clone(), manifest.source.snapshot_id)?;
    if inputs != manifest.source.inputs || obligations != manifest.obligations {
        return Err(Error::InvalidSource);
    }
    let mut non_goal = Vec::new();
    for fragment in fragments {
        if fragment.body.trim().is_empty() {
            continue;
        }
        if !source_ref_can_anchor_goal(boundary, &fragment.kind)? {
            non_goal.push(fragment.id.to_string());
        }
    }
    non_goal.sort();
    Ok(non_goal)
}

fn source_ref_can_anchor_goal(
    boundary: tect_domain::CandidateBoundary,
    kind: &str,
) -> Result<bool> {
    if !matches!(kind, "planning_input" | "program_success" | "program_field") {
        return Err(Error::InvalidSource);
    }
    Ok(kind
        == match boundary {
            tect_domain::CandidateBoundary::Ongoing => "planning_input",
            tect_domain::CandidateBoundary::Finite => "program_success",
        })
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
