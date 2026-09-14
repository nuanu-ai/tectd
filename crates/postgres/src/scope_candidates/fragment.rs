use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn fragment(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    snapshot_id: Option<Uuid>,
    source_ref_id: Uuid,
    cursor: usize,
    max_bytes: usize,
) -> Result<CandidateTextFragment> {
    if max_bytes < 4 {
        return Err(Error::InvalidArguments);
    }
    let program: Option<(Uuid, bool)> = sqlx::query_as(
        "SELECT p.id,p.payload_erased FROM scope_candidate_sets s JOIN programs p \
         ON p.tenant_id=s.tenant_id AND p.workspace_id=s.workspace_id AND p.id=s.program_id \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let Some((program, program_erased)) = program else {
        return Err(Error::NotFound);
    };
    crate::planning_knowledge::require_owned_payload_identity(
        transaction,
        tenant_id,
        workspace_id,
        &["programs"],
        Some(program),
    )
    .await?;
    if program_erased {
        return Err(Error::KnowledgePayloadErased);
    }
    let row = sqlx::query_as::<_, FragmentRow>(
        "SELECT r.snapshot_id,r.kind,r.input_sequence,r.program_field,r.label,c.body \
         FROM scope_candidate_source_refs r \
         JOIN scope_candidate_sets s ON s.tenant_id=r.tenant_id AND s.workspace_id=r.workspace_id \
          AND s.id=r.candidate_set_id \
         JOIN scope_candidate_contents c ON c.tenant_id=r.tenant_id \
          AND c.workspace_id=r.workspace_id AND c.digest=r.body_digest \
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.candidate_set_id=$3 AND r.id=$4 \
           AND (($5::uuid IS NULL AND s.current_snapshot_id=r.snapshot_id) OR r.snapshot_id=$5)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(source_ref_id)
    .bind(snapshot_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    let FragmentRow {
        snapshot_id,
        kind,
        input_sequence,
        program_field,
        label,
        body,
    } = row;
    if cursor > body.len() || !body.is_char_boundary(cursor) {
        return Err(Error::InvalidArguments);
    }
    let mut end = cursor.saturating_add(max_bytes).min(body.len());
    while end > cursor && !body.is_char_boundary(end) {
        end -= 1;
    }
    if cursor < body.len() && end == cursor {
        return Err(Error::InternalInvariant);
    }
    let next_source_ref_id = if kind == "program_field" || kind == "program_success" {
        let refs: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM scope_candidate_source_refs \
             WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND snapshot_id=$4 \
               AND kind IN ('program_field','program_success') \
             ORDER BY CASE program_field WHEN 'name' THEN 1 WHEN 'intent' THEN 2 \
               WHEN 'basis' THEN 3 WHEN 'boundaries' THEN 4 WHEN 'constraints' THEN 5 \
               WHEN 'success' THEN 6 ELSE 7 END,id",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(snapshot_id)
        .fetch_all(&mut **transaction)
        .await
        .map_err(storage_error)?;
        refs.iter()
            .position(|id| *id == source_ref_id)
            .and_then(|position| refs.get(position + 1))
            .copied()
    } else {
        None
    };
    Ok(CandidateTextFragment {
        source_ref: CandidateSourceRef {
            id: source_ref_id,
            kind: parse_source_kind(&kind)?,
            input_sequence,
            program_field,
            label,
        },
        snapshot_id,
        cursor,
        next_cursor: (end < body.len()).then_some(end),
        next_source_ref_id,
        text: body[cursor..end].to_owned(),
    })
}
