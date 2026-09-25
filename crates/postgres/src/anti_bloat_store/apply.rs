use super::*;

pub(super) async fn apply_preserved_delta(
    uow: &mut PgUnitOfWork,
    review_id: Uuid,
    input: &AntiBloatInput,
    finding_id: &str,
    disposition: AntiBloatDisposition,
    preservation: &AntiBloatPreservation,
    delta: &CandidateDeltaBatch,
    after: &ResolvedCandidateDraft,
) -> Result<AntiBloatApplyReceipt> {
    if !uow.is_read_write() || disposition != AntiBloatDisposition::Narrow {
        return Err(Error::Forbidden);
    }
    let saved = uow.review(review_id).await?.ok_or(Error::NotFound)?;
    if &saved.input != input || saved.review.candidate_set_id != delta.candidate_set_id {
        return Err(Error::InputConflict);
    }
    let tenant = uow.tenant_id()?;
    let workspace = saved.workspace_id;
    // Serialize both new writes and replay with native candidate writers.
    let revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 \
         AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(delta.candidate_set_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    type Existing = (
        String,
        serde_json::Value,
        serde_json::Value,
        serde_json::Value,
        String,
        String,
        Option<String>,
        Option<Uuid>,
        Option<i64>,
        Option<i64>,
        Option<String>,
        Option<String>,
        serde_json::Value,
    );
    let existing: Option<Existing> = sqlx::query_as(
        "SELECT finding_id,preservation_payload,delta_payload,after_payload, \
         after_material_digest,caller_idempotency_key,caller_operation,caller_request_id, \
         from_revision,to_revision,source_digest,before_material_digest,caller_receipt \
         FROM scope_anti_bloat_caller_links \
         WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(review_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let preservation_json = serde_json::to_value(preservation).map_err(storage_error)?;
    let delta_json = serde_json::to_value(delta).map_err(storage_error)?;
    let after_json = serde_json::to_value(after).map_err(storage_error)?;
    if let Some((
        stored_finding,
        stored_preservation,
        stored_delta,
        stored_after,
        stored_after_digest,
        stored_key,
        stored_operation,
        stored_request_id,
        stored_from_revision,
        stored_to_revision,
        stored_source_digest,
        stored_before_digest,
        stored_receipt,
    )) = existing
    {
        if stored_finding != finding_id
            || stored_preservation != preservation_json
            || stored_delta != delta_json
            || stored_after != after_json
            || stored_after_digest != preservation.after_material_digest
            || stored_key != delta.idempotency_key
            || stored_operation.as_deref() != Some("anti_bloat_narrow")
            || stored_from_revision != Some(delta.expected_revision)
            || stored_source_digest.as_deref() != Some(preservation.source_digest.as_str())
            || stored_before_digest.as_deref() != Some(preservation.before_material_digest.as_str())
        {
            return Err(Error::InputConflict);
        }
        let receipt: AntiBloatApplyReceipt =
            serde_json::from_value(stored_receipt.clone()).map_err(storage_error)?;
        if receipt.review_id != review_id
            || receipt.candidate_set_id != delta.candidate_set_id
            || receipt.idempotency_key != delta.idempotency_key
            || Some(receipt.caller_request_id) != stored_request_id
            || Some(receipt.from_revision) != stored_from_revision
            || Some(receipt.to_revision) != stored_to_revision
            || receipt.source_digest != preservation.source_digest
            || receipt.before_material_digest != preservation.before_material_digest
            || receipt.after_material_digest != preservation.after_material_digest
        {
            return Err(Error::InputConflict);
        }
        let native: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
            "SELECT r.result_payload,d.payload FROM scope_candidate_receipts r \
             JOIN scope_candidate_drafts d ON (d.tenant_id,d.workspace_id,d.candidate_set_id,d.set_revision)= \
                 (r.tenant_id,r.workspace_id,r.candidate_set_id,r.result_revision) \
             WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.candidate_set_id=$3 \
               AND r.operation='anti_bloat_narrow' AND r.request_id=$4 AND r.result_revision=$5",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(delta.candidate_set_id)
        .bind(receipt.caller_request_id)
        .bind(receipt.to_revision)
        .fetch_optional(&mut **uow.transaction()?)
        .await
        .map_err(storage_error)?;
        if native != Some((stored_receipt, after_json)) {
            return Err(Error::InputConflict);
        }
        return Ok(receipt);
    }
    if revision != Some(delta.expected_revision)
        || uow
            .authoritative_input(workspace, delta.candidate_set_id, delta.expected_revision)
            .await?
            .as_ref()
            != Some(input)
        || review_anti_bloat(&Sha256ScopeDigest, input)? != saved.review
    {
        return Err(Error::InputConflict);
    }
    let checked = check_anti_bloat_delta(
        &Sha256ScopeDigest,
        input,
        &saved.review,
        finding_id,
        disposition,
        delta,
        after,
    )
    .map_err(|_| Error::InputConflict)?;
    if &checked != preservation {
        return Err(Error::InputConflict);
    }
    let source = &input.manifest.source;
    let authority: Option<CurrentSourceAuthority> = sqlx::query_as(
        "SELECT c.current_snapshot_id,c.input_cursor,c.latest_input AS candidate_latest_input, \
                c.program_id,p.revision AS program_revision,p.latest_input AS program_current_latest, \
                s.program_latest_input,s.planning_latest_input,s.selected_sources_digest, \
                s.method_revision,s.method_digest,s.registry_revision,s.registry_digest \
         FROM scope_candidate_sets c JOIN programs p \
           ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
         JOIN scope_candidate_snapshots s \
           ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)= \
              (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 FOR UPDATE OF p",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(delta.candidate_set_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let Some(authority) = authority else {
        return Err(Error::InputConflict);
    };
    if authority.current_snapshot_id != Some(source.snapshot_id)
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
    crate::scope_advisory::require_persisted_fragments(
        uow.transaction()?,
        tenant,
        workspace,
        source,
        &input.manifest.obligations,
    )
    .await?;
    let before = &input
        .manifest
        .eligible(&input.selected_id)
        .ok_or(Error::InputConflict)?
        .material;
    if scope_candidate_material_digest(&Sha256ScopeDigest, before)?
        != preservation.before_material_digest
        || scope_candidate_material_digest(&Sha256ScopeDigest, after)?
            != preservation.after_material_digest
    {
        return Err(Error::InputConflict);
    }
    let receipt = AntiBloatApplyReceipt {
        review_id,
        candidate_set_id: delta.candidate_set_id,
        idempotency_key: delta.idempotency_key.clone(),
        caller_request_id: Uuid::new_v4(),
        from_revision: delta.expected_revision,
        to_revision: delta
            .expected_revision
            .checked_add(1)
            .ok_or(Error::StorageUnavailable)?,
        source_digest: preservation.source_digest.clone(),
        before_material_digest: preservation.before_material_digest.clone(),
        after_material_digest: preservation.after_material_digest.clone(),
    };
    let request_payload = serde_json::json!({
        "review_id": review_id,
        "finding_id": finding_id,
        "disposition": disposition,
        "preservation": preservation,
        "delta": delta,
        "after": after,
    });
    crate::scope_candidates::save_preserved_anti_bloat_draft(
        uow.transaction()?,
        tenant,
        workspace,
        &receipt,
        source.snapshot_id,
        source.input_cursor,
        before,
        after,
        request_payload,
    )
    .await?;
    sqlx::query(
        "INSERT INTO scope_anti_bloat_caller_links \
         (tenant_id,workspace_id,review_id,candidate_set_id,finding_id,disposition, \
          preservation_payload,delta_payload,after_payload,after_material_digest, \
          caller_idempotency_key,caller_receipt,caller_operation,caller_request_id, \
          from_revision,to_revision,source_digest,before_material_digest) \
         VALUES ($1,$2,$3,$4,$5,'narrow',$6,$7,$8,$9,$10,$11,'anti_bloat_narrow',$12,$13,$14,$15,$16)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(review_id)
    .bind(delta.candidate_set_id)
    .bind(finding_id)
    .bind(preservation_json)
    .bind(delta_json)
    .bind(after_json)
    .bind(&preservation.after_material_digest)
    .bind(&delta.idempotency_key)
    .bind(serde_json::to_value(&receipt).map_err(storage_error)?)
    .bind(receipt.caller_request_id)
    .bind(receipt.from_revision)
    .bind(receipt.to_revision)
    .bind(&receipt.source_digest)
    .bind(&receipt.before_material_digest)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    Ok(receipt)
}
