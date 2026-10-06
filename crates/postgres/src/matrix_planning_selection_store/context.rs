use super::*;

/// Re-evaluate the V2 material while the task head and every declaration head
/// remain locked in this transaction. The caller's link is never authority.
pub(crate) async fn current_context_evaluation(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
) -> Result<(String, String)> {
    evaluate_current_context(tx, workspace_id, link, true).await
}

/// Verifier readback performs the same V2 task, declaration and effect checks
/// without writer locks. Its attestation path holds the parent
/// candidate-set lock and repeats this readback before insertion.
pub(crate) async fn current_context_evaluation_for_verifier(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
) -> Result<(String, String)> {
    evaluate_current_context(tx, workspace_id, link, false).await
}

async fn evaluate_current_context(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
    lock_authority: bool,
) -> Result<(String, String)> {
    validate_persisted_link(link)?;
    let provenance = link
        .context_provenance
        .as_ref()
        .ok_or(Error::StaleContext)?;
    let source = tx
        .matrix_task_source(workspace_id, link.selection.task_id)
        .await?
        .ok_or(Error::StaleRevision)?;
    let binding = source
        .requirements_binding
        .clone()
        .ok_or(Error::StaleContext)?;
    if binding.snapshot_id != provenance.frozen_snapshot_id
        || binding.authority_schema != provenance.authority_schema
        || binding.semantic_digest != provenance.requirements_semantic_digest
        || binding.authority_schema != MATRIX_REQUIREMENTS_SCHEMA
    {
        return Err(Error::StaleContext);
    }
    let current_principal = tx.principal_id()?;
    let lineage = tx
        .matrix_requirements_lineage(workspace_id, current_principal, &binding.locator, false)
        .await?;
    if lock_authority {
        for anchor in &lineage {
            tx.lock_matrix_requirements_head(workspace_id, *anchor)
                .await?;
        }
    }
    let current = if lock_authority {
        tx.lock_matrix_task(workspace_id, link.selection.task_id)
            .await?
    } else {
        tx.matrix_task(workspace_id, link.selection.task_id).await?
    }
    .ok_or(Error::StaleRevision)?;
    if current != source.revision
        || current.revision != link.selection.task_revision
        || current.input_digest != link.selection.expected_input_digest
    {
        return Err(Error::StaleRevision);
    }
    if current.choice_set.is_none() {
        return Err(Error::InternalInvariant);
    }
    if current.choice_set_digest.as_deref()
        != Some(link.selection.expected_choice_set_digest.as_str())
    {
        return Err(Error::StaleRevision);
    }
    // The declaration locks precede the task lock. Re-read both source and
    // lineage so a pre-lock observation cannot authorize a raced binding.
    let fresh = tx
        .matrix_task_source(workspace_id, link.selection.task_id)
        .await?
        .ok_or(Error::StaleRevision)?;
    let fresh_lineage = tx
        .matrix_requirements_lineage(workspace_id, current_principal, &binding.locator, false)
        .await?;
    if fresh != source || fresh_lineage != lineage {
        return Err(Error::StaleContext);
    }
    let frozen = tx
        .frozen_matrix_requirements_by_id(workspace_id, binding.snapshot_id)
        .await?
        .ok_or(Error::StaleContext)?;
    if lineage.last().copied() != Some(frozen.anchor)
        || frozen.effective.schema() != binding.authority_schema
        || frozen.effective.semantic_digest() != binding.semantic_digest
    {
        return Err(Error::StaleContext);
    }
    let revisions = tx
        .matrix_requirements_revisions(workspace_id, &lineage)
        .await?;
    let effective = resolve_matrix_requirements(&lineage, &revisions, MATRIX_REQUIREMENTS_SCHEMA)?;
    if effective.semantic_digest() != binding.semantic_digest {
        return Err(Error::StaleContext);
    }
    let set = current
        .choice_set
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    set.validate(&current.input)?;
    if !set
        .candidates
        .iter()
        .any(|candidate| candidate.candidate_id == link.selection.selected_choice_id)
    {
        return Err(Error::InternalInvariant);
    }
    let record = if lock_authority {
        locked_context_verification(tx, workspace_id, link, binding.snapshot_id).await?
    } else {
        tx.context_matrix_verification_for_revision(
            workspace_id,
            current.task_id,
            current.revision,
            &current.input_digest,
            binding.snapshot_id,
        )
        .await?
        .ok_or(Error::StaleContext)?
    };
    if record.digest != link.selection.expected_verification_digest
        || record.owner_principal != current.recorded_by_principal_id.to_string()
        || record.verifier_principal == record.owner_principal
        || record.input_digest != current.input_digest
        || record.frozen_snapshot_id != binding.snapshot_id.to_string()
        || record.authority_schema != binding.authority_schema
        || record.requirements_semantic_digest != binding.semantic_digest
    {
        return Err(Error::StaleContext);
    }
    let now: i64 = sqlx::query_scalar(
        "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
    )
    .fetch_one(&mut **tx.transaction()?)
    .await
    .map_err(storage_error)?;
    let validated = match classify_context_matrix_verification(
        &current.task_id.to_string(),
        &current.revision.to_string(),
        &binding.snapshot_id.to_string(),
        &current.input,
        &frozen.effective,
        &record,
        now,
    )? {
        ContextMatrixVerificationCurrentness::Current(validated) => validated,
        ContextMatrixVerificationCurrentness::Expired => return Err(Error::StaleContext),
    };
    let composition = compose_confirmed_requirements_matrix(
        &current.task_id.to_string(),
        &current.revision.to_string(),
        &binding.snapshot_id.to_string(),
        &current.input,
        &frozen.effective,
        &validated,
        now,
    )?;
    let digest =
        context_matrix_verified_disposition_digest(&current.input, &composition, set, &record)?;
    Ok((
        digest,
        composition.composition().catalogue_version.to_owned(),
    ))
}

/// Select the captured parent, lock it, then decode children in a fresh RC
/// statement. The lock-only definer grants no UPDATE or record-read capability.
async fn locked_context_verification(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
    link: &MatrixPlanningSelectionLink,
    snapshot_id: Uuid,
) -> Result<tect_domain::ContextMatrixVerificationRecord> {
    if !tx.is_read_write() {
        return Err(Error::Forbidden);
    }
    let tenant = tx.tenant_id()?;
    let row = sqlx::query(
        "SELECT v.id,v.input_digest,v.schema,v.owner_principal_id,v.verifier_principal_id, \
                v.policy_version,v.record_digest,v.verification_reason,v.frozen_snapshot_id, \
                v.requirements_semantic_digest,v.authority_schema \
         FROM matrix_tasks t JOIN matrix_verifications v \
           ON (v.tenant_id,v.workspace_id,v.task_id)=(t.tenant_id,t.workspace_id,t.id) \
         JOIN matrix_task_requirements_bindings b \
           ON (b.tenant_id,b.workspace_id,b.task_id,b.revision,b.snapshot_id,b.semantic_digest,b.authority_schema)= \
              (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,v.frozen_snapshot_id,v.requirements_semantic_digest,v.authority_schema) \
         WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3 \
           AND t.current_revision=$4 AND v.task_revision=$4 AND v.input_digest=$5 \
           AND v.frozen_snapshot_id=$6 AND v.schema='tect.context-matrix-verification/1' \
           AND v.record_digest=$7",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(link.selection.task_id)
    .bind(link.selection.task_revision)
    .bind(&link.selection.expected_input_digest)
    .bind(snapshot_id)
    .bind(&link.selection.expected_verification_digest)
    .fetch_optional(&mut **tx.transaction()?)
    .await
    .map_err(storage_error)?
    .ok_or(Error::StaleContext)?;
    let id: Uuid = row.try_get("id").map_err(storage_error)?;
    sqlx::query("SELECT public.matrix_planning_lock_verification($1,$2,$3)")
        .bind(tenant)
        .bind(workspace_id)
        .bind(id)
        .execute(&mut **tx.transaction()?)
        .await
        .map_err(link_write_error)?;
    tx.decode_context_verification(
        link.selection.task_id,
        link.selection.task_revision,
        workspace_id,
        row,
    )
    .await
}

#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct PersistedMappedNode {
    draft_index: usize,
    node_id: Uuid,
    node_revision: i64,
}

pub(super) fn mapped_nodes_json(nodes: &[MatrixPlanningMappedNode]) -> Result<Value> {
    let persisted: Vec<_> = nodes
        .iter()
        .map(|node| PersistedMappedNode {
            draft_index: node.draft_index,
            node_id: node.node_id,
            node_revision: node.node_revision,
        })
        .collect();
    serde_json::to_value(persisted).map_err(storage_error)
}

pub(crate) fn decode_mapped_nodes(value: Option<Value>) -> Result<Vec<MatrixPlanningMappedNode>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let persisted: Vec<PersistedMappedNode> =
        serde_json::from_value(value).map_err(|_| Error::InternalInvariant)?;
    if persisted.is_empty()
        || persisted.len() > 100
        || persisted
            .iter()
            .any(|node| node.node_id.is_nil() || node.node_revision < 1)
        || persisted
            .windows(2)
            .any(|pair| pair[0].draft_index >= pair[1].draft_index)
    {
        return Err(Error::InternalInvariant);
    }
    Ok(persisted
        .into_iter()
        .map(|node| MatrixPlanningMappedNode {
            draft_index: node.draft_index,
            node_id: node.node_id,
            node_revision: node.node_revision,
        })
        .collect())
}

pub(super) fn link_write_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|e| e.code())
        .is_some_and(|c| c == "42501")
    {
        Error::Forbidden
    } else {
        storage_error(error)
    }
}

pub(super) fn verify_mapped_nodes(
    request: &Value,
    result: &Value,
    link: &MatrixPlanningSelectionLink,
) -> Result<()> {
    let request_nodes = request
        .pointer("/draft/nodes")
        .and_then(Value::as_array)
        .ok_or(Error::InputConflict)?;
    let resolved_nodes = result
        .pointer("/draft/nodes")
        .and_then(Value::as_array)
        .ok_or(Error::InputConflict)?;
    if request_nodes.len() != resolved_nodes.len()
        || link.mapped_nodes.len() != link.selection.mapped_draft_node_indices.len()
        || link.mapped_nodes.is_empty()
    {
        return Err(Error::InputConflict);
    }
    for (mapped, index) in link
        .mapped_nodes
        .iter()
        .zip(&link.selection.mapped_draft_node_indices)
    {
        if mapped.draft_index != *index || mapped.node_id.is_nil() || mapped.node_revision < 1 {
            return Err(Error::InputConflict);
        }
        let submitted = request_nodes.get(*index).ok_or(Error::InputConflict)?;
        let resolved = resolved_nodes.get(*index).ok_or(Error::InputConflict)?;
        if submitted.get("kind") != resolved.get("kind")
            || submitted
                .get("kind")
                .and_then(Value::as_str)
                .is_none_or(|kind| kind != "work" && kind != "decision")
            || resolved.get("id") != Some(&serde_json::json!(mapped.node_id))
            || resolved.get("revision") != Some(&serde_json::json!(mapped.node_revision))
        {
            return Err(Error::InputConflict);
        }
        let identity = submitted.get("identity").ok_or(Error::InputConflict)?;
        if let Some(existing_id) = identity.get("candidate_id")
            && !existing_id.is_null()
            && existing_id != &serde_json::json!(mapped.node_id)
        {
            return Err(Error::InputConflict);
        }
    }
    Ok(())
}

pub(crate) async fn require_workspace_reader(
    tx: &mut PgUnitOfWork,
    workspace_id: Uuid,
) -> Result<()> {
    if workspace_id.is_nil()
        || !matches!(
            tx.principal_role()?,
            PrincipalRole::Owner | PrincipalRole::Verifier
        )
    {
        return Err(Error::Forbidden);
    }
    let principal = tx.principal_id()?;
    if !tx.is_member(workspace_id, principal).await? {
        return Err(Error::Forbidden);
    }
    Ok(())
}

pub(crate) fn decode_provenance(
    snapshot: Option<Uuid>,
    schema: Option<String>,
    digest: Option<String>,
) -> Result<Option<MatrixPlanningContextProvenance>> {
    match (snapshot, schema, digest) {
        (None, None, None) => Ok(None),
        (Some(frozen_snapshot_id), Some(authority_schema), Some(requirements_semantic_digest)) => {
            let provenance = MatrixPlanningContextProvenance {
                frozen_snapshot_id,
                authority_schema,
                requirements_semantic_digest,
            };
            provenance
                .validate()
                .map_err(|_| Error::InternalInvariant)?;
            Ok(Some(provenance))
        }
        _ => Err(Error::InternalInvariant),
    }
}

pub(crate) fn validate_persisted_link(link: &MatrixPlanningSelectionLink) -> Result<()> {
    // Legacy links can be read, but current_context_evaluation never authorizes them.
    let Some(provenance) = &link.context_provenance else {
        return Ok(());
    };
    provenance
        .validate()
        .map_err(|_| Error::InternalInvariant)?;
    link.selection
        .validate()
        .map_err(|_| Error::InternalInvariant)?;
    let digest = |value: &str| {
        value.len() == 64
            && value
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
    };
    if link.scope_id.is_nil()
        || link.candidate_set_id.is_nil()
        || link.caller_request_id.is_nil()
        || link.caller_principal_id.is_nil()
        || link.caller_session_id.is_nil()
        || link.result_revision < 1
        || !digest(&link.evaluation_digest)
        || link.catalogue_version.is_empty()
        || link.catalogue_version.trim() != link.catalogue_version
        || link.catalogue_version.contains('\0')
        || link.mapped_nodes.len() != link.selection.mapped_draft_node_indices.len()
        || link
            .mapped_nodes
            .iter()
            .zip(&link.selection.mapped_draft_node_indices)
            .any(|(node, index)| {
                node.draft_index != *index || node.node_id.is_nil() || node.node_revision < 1
            })
    {
        return Err(Error::InternalInvariant);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn provenance_is_complete_v2_or_legacy_none() {
        assert_eq!(decode_provenance(None, None, None), Ok(None));
        assert_eq!(
            decode_provenance(Some(Uuid::new_v4()), None, None),
            Err(Error::InternalInvariant)
        );
        assert!(
            decode_provenance(
                Some(Uuid::new_v4()),
                Some(MATRIX_REQUIREMENTS_SCHEMA.into()),
                Some("a".repeat(64))
            )
            .unwrap()
            .is_some()
        );
    }
    #[test]
    fn malformed_mapped_json_is_not_stale() {
        assert!(decode_mapped_nodes(None).unwrap().is_empty());
        assert_eq!(
            decode_mapped_nodes(Some(serde_json::json!({"draft_index": 0}))),
            Err(Error::InternalInvariant)
        );
    }
}
