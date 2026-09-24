/// Bind a selected advisory to the existing candidate draft write in one transaction.
/// The preservation record is written before the candidate revision advances;
/// the caller link is written after its ordinary mutation receipt exists.
pub(crate) async fn selected_candidate_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
    session: Uuid,
    request: &SaveCandidateDraft,
) -> Result<Option<StoredCandidateContext>> {
    let selected = request
        .selected_advisory
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let Some(stored) = crate::scope_candidates::replay(
        tx,
        tenant,
        workspace,
        &CandidateReceiptRequest::SaveDraft(request.clone()),
    )
    .await?
    else {
        return Ok(None);
    };
    let identity: Option<(Uuid, Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT opportunity_id,disposition_id,actor_id,session_id FROM advisory_scope_caller_link \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 \
           AND caller_operation='save_draft' AND caller_request_id=$4 AND request_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.candidate_set_id)
    .bind(request.request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if identity
        != Some((
            selected.opportunity_id,
            selected.disposition_id,
            actor,
            session,
        ))
        || !actor_session_exists(tx, tenant, workspace, actor, session).await?
    {
        return Err(Error::InputConflict);
    }
    Ok(Some(stored))
}

pub(crate) async fn save_selected_candidate_draft(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
    session: Uuid,
    request: &SaveCandidateDraft,
) -> Result<StoredCandidateContext> {
    let selected = request
        .selected_advisory
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    if selected.opportunity_id.is_nil()
        || selected.disposition_id.is_nil()
        || selected.alternative_key.is_empty()
        || selected.alternative_key.len() > 64
        || !selected
            .alternative_key
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || b"._-".contains(&byte))
    {
        return Err(Error::InvalidArguments);
    }
    if let Some(stored) =
        selected_candidate_receipt(tx, tenant, workspace, actor, session, request).await?
    {
        return Ok(stored);
    }
    let manifest = load_manifest(
        tx,
        tenant,
        workspace,
        selected.opportunity_id,
        Some(request.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    if manifest.constructor != source_authored_identity() {
        return Err(Error::InputConflict);
    }
    let (advice_header, advice) = load_advice(
        tx,
        tenant,
        workspace,
        selected.opportunity_id,
        Some(request.candidate_set_id),
    )
    .await?
    .ok_or(Error::NotFound)?;
    // CAS takes this advice lock before reading the latest disposition.
    // Keep it until the candidate receipt and caller link commit together.
    lock_scope_key(tx, tenant, workspace, "disposition", &advice.id.0).await?;
    // A concurrent identical save may have committed while this transaction
    // waited on the disposition lock. Recheck its exact authenticated receipt
    // before evaluating freshness against the now-advanced candidate revision.
    if let Some(stored) =
        selected_candidate_receipt(tx, tenant, workspace, actor, session, request).await?
    {
        return Ok(stored);
    }
    let row: DispositionRow = sqlx::query_as(
        "SELECT opportunity_id,candidate_set_id,actor_id,session_id,disposition_id,request_id,advice_id,\
                revision,predecessor_id,action,selected_alternative_id,aggregate_payload \
         FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 \
           AND opportunity_id=$3 AND candidate_set_id=$4 AND disposition_id=$5",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(selected.opportunity_id)
    .bind(request.candidate_set_id)
    .bind(selected.disposition_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    if row.actor_id != actor || row.session_id != session {
        return Err(Error::InputConflict);
    }
    let latest: Option<Uuid> = sqlx::query_scalar(
        "SELECT disposition_id FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 \
         AND opportunity_id=$3 AND candidate_set_id=$4 ORDER BY revision DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(selected.opportunity_id)
    .bind(request.candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if latest != Some(selected.disposition_id) {
        return Err(Error::StaleRevision);
    }
    let disposition = load_disposition(tx, tenant, workspace, row).await?;
    if disposition.selected_id.as_ref() != Some(&selected.selected_id) {
        return Err(Error::InputConflict);
    }
    // The persistence adapters take these locks before config/source checks.
    // Acquire them in that order to avoid reversing another writer's lock order.
    lock_scope_key(
        tx,
        tenant,
        workspace,
        "preservation",
        selected.disposition_id,
    )
    .await?;
    let alternative = manifest
        .eligible(&selected.selected_id)
        .ok_or(Error::InputConflict)?;
    if manifest.source.candidate_set_revision != request.revision
        || manifest.source.snapshot_id != request.snapshot_id
        || manifest.source.input_cursor != request.input_cursor
    {
        return Err(Error::StaleRevision);
    }
    require_current_opportunity_config(
        tx,
        tenant,
        workspace,
        selected.opportunity_id,
        request.candidate_set_id,
        advice_header.config_revision,
    )
    .await?;
    require_frozen_authority(tx, tenant, workspace, &manifest.source).await?;
    if !actor_session_exists(tx, tenant, workspace, actor, session).await? {
        return Err(Error::Forbidden);
    }
    let source_ids = manifest
        .source
        .inputs
        .iter()
        .map(|input| Uuid::parse_str(&input.id).map_err(|_| Error::InvalidSource))
        .collect::<Result<std::collections::BTreeSet<_>>>()?;
    let seed = authored_seed(
        tenant,
        workspace,
        &manifest.source,
        &manifest.constructor,
        &selected.alternative_key,
    )?;
    let prior = crate::scope_candidates::load(tx, tenant, workspace, request.candidate_set_id)
        .await?
        .ok_or(Error::NotFound)?;
    let resolved = crate::scope_candidates::resolve::resolve_authored(
        tx,
        &crate::scope_candidates::resolve::ResolveContext {
            tenant_id: tenant,
            workspace_id: workspace,
            candidate_set_id: request.candidate_set_id,
            snapshot_id: request.snapshot_id,
            latest_input: manifest.source.planning_latest_input,
        },
        &request.draft,
        prior.draft.as_ref(),
        &seed,
        &source_ids,
    )
    .await?;
    if resolved != alternative.material {
        return Err(Error::InputConflict);
    }
    let observation = FreshScopeObservation {
        source: manifest.source.clone(),
        manifest: manifest.clone(),
        candidate_set_revision: request.revision,
        advice_id: advice.id.clone(),
    };
    let preservation = evaluate_scope_preservation(
        &Sha256ScopeDigest,
        &manifest,
        &advice,
        &disposition,
        &observation,
    )?;
    if !matches!(preservation.status, ScopePreservationStatus::Passed) {
        return Err(Error::StaleRevision);
    }
    let preservation_id = Uuid::new_v4();
    persist_preservation(
        tx,
        tenant,
        workspace,
        &ScopePreservationReceiptInput {
            receipt_id: preservation_id,
            request_id: request.request_id,
            opportunity_id: selected.opportunity_id,
            candidate_set_id: request.candidate_set_id,
            disposition_id: selected.disposition_id,
            observation,
            result: preservation,
        },
    )
    .await?;
    let stored =
        crate::scope_candidates::save_selected_draft(tx, tenant, workspace, request, &resolved)
            .await?;
    persist_caller_link(
        tx,
        tenant,
        workspace,
        &ScopeCallerLinkInput {
            link_id: Uuid::new_v4(),
            request_id: request.request_id,
            opportunity_id: selected.opportunity_id,
            candidate_set_id: request.candidate_set_id,
            disposition_id: selected.disposition_id,
            preservation_receipt_id: preservation_id,
            caller_operation: "save_draft".into(),
            caller_request_id: request.request_id,
            caller_result_revision: stored.context.candidate_set.revision,
            actor_id: actor,
            session_id: session,
        },
    )
    .await?;
    Ok(stored)
}
