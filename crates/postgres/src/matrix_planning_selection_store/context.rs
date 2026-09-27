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
/// without owner-only writer locks. Its attestation path holds the parent
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
    let provenance = link
        .context_provenance
        .as_ref()
        .ok_or(Error::StaleContext)?;
    let source = tx
        .matrix_task_source(workspace_id, link.selection.task_id)
        .await?
        .ok_or(Error::StaleRevision)?;
    let binding = source.requirements_binding.ok_or(Error::StaleContext)?;
    if binding.snapshot_id != provenance.frozen_snapshot_id
        || binding.authority_schema != provenance.authority_schema
        || binding.semantic_digest != provenance.requirements_semantic_digest
        || binding.authority_schema != MATRIX_REQUIREMENTS_SCHEMA
    {
        return Err(Error::StaleContext);
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
        || current.choice_set_digest.as_deref()
            != Some(link.selection.expected_choice_set_digest.as_str())
    {
        return Err(Error::StaleRevision);
    }
    let current_principal = tx.principal_id()?;
    let lineage = tx
        .matrix_requirements_lineage(workspace_id, current_principal, &binding.locator, false)
        .await
        .map_err(|_| Error::StaleContext)?;
    if lock_authority {
        for anchor in &lineage {
            tx.lock_matrix_requirements_head(workspace_id, *anchor)
                .await
                .map_err(|_| Error::StaleContext)?;
        }
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
        .await
        .map_err(|_| Error::StaleContext)?;
    let effective = resolve_matrix_requirements(&lineage, &revisions, MATRIX_REQUIREMENTS_SCHEMA)
        .map_err(|_| Error::StaleContext)?;
    if effective.semantic_digest() != binding.semantic_digest {
        return Err(Error::StaleContext);
    }
    let set = current.choice_set.as_ref().ok_or(Error::StaleContext)?;
    set.validate(&current.input)
        .map_err(|_| Error::StaleContext)?;
    if !set
        .candidates
        .iter()
        .any(|candidate| candidate.candidate_id == link.selection.selected_choice_id)
    {
        return Err(Error::StaleContext);
    }
    let record = tx
        .context_matrix_verification_for_revision(
            workspace_id,
            current.task_id,
            current.revision,
            &current.input_digest,
            binding.snapshot_id,
        )
        .await?
        .ok_or(Error::StaleContext)?;
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
    let validated = evaluate_context_matrix_verification(
        &current.task_id.to_string(),
        &current.revision.to_string(),
        &binding.snapshot_id.to_string(),
        &current.input,
        &frozen.effective,
        &record,
        now,
    )
    .map_err(|_| Error::StaleContext)?;
    let composition = compose_confirmed_requirements_matrix(
        &current.task_id.to_string(),
        &current.revision.to_string(),
        &binding.snapshot_id.to_string(),
        &current.input,
        &frozen.effective,
        &validated,
        now,
    )
    .map_err(|_| Error::StaleContext)?;
    let digest =
        context_matrix_verified_evaluation_digest(&current.input, &composition, set, &record)
            .map_err(|_| Error::StaleContext)?;
    Ok((
        digest,
        composition.composition().catalogue_version.to_owned(),
    ))
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

pub(super) fn decode_mapped_nodes(value: Option<Value>) -> Result<Vec<MatrixPlanningMappedNode>> {
    let Some(value) = value else {
        return Ok(Vec::new());
    };
    let persisted: Vec<PersistedMappedNode> =
        serde_json::from_value(value).map_err(storage_error)?;
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
