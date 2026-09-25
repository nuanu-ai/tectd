use super::*;

pub(super) async fn load_pipeline_recommendation_basis(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    work_node_id: Uuid,
    for_update: bool,
) -> Result<Option<PipelineRecommendationBasis>> {
    let tenant = uow.tenant_id()?;
    let mut query = String::from(
        "SELECT c.scope_id,c.revision AS set_revision,s.id AS planning_snapshot_id, \
                s.source_snapshot_id,s.source_candidate_set_revision, \
                source.selected_sources_digest,s.catalogue, \
                draft.set_revision AS draft_revision, \
                l.caller_request_id,l.disposition_id,l.task_id,l.task_revision, \
                l.selected_choice_id,l.input_digest,l.choice_set_digest, \
                l.verification_digest,l.evaluation_digest,l.catalogue_version, \
                a.id AS attestation_id,a.effect_digest,a.verifier_principal_id, \
                a.verdict \
         FROM slice_candidate_sets c \
         JOIN native_scopes n ON (n.tenant_id,n.workspace_id,n.id)= \
              (c.tenant_id,c.workspace_id,c.scope_id) \
         JOIN slice_planning_snapshots s ON \
              (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)= \
              (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
         JOIN scope_candidate_sets source_set ON \
              (source_set.tenant_id,source_set.workspace_id,source_set.id)= \
              (n.tenant_id,n.workspace_id,n.source_candidate_set_id) \
         JOIN scope_candidate_snapshots source ON \
              (source.tenant_id,source.workspace_id,source.candidate_set_id,source.id)= \
              (source_set.tenant_id,source_set.workspace_id,source_set.id,s.source_snapshot_id) \
         JOIN slice_candidate_drafts draft ON \
              (draft.tenant_id,draft.workspace_id,draft.candidate_set_id)= \
              (c.tenant_id,c.workspace_id,c.id) \
         JOIN slice_candidate_reviews review ON \
              (review.tenant_id,review.workspace_id,review.candidate_set_id,review.set_revision)= \
              (c.tenant_id,c.workspace_id,c.id,c.revision) \
         JOIN matrix_planning_effect_attestations a ON \
              (a.tenant_id,a.workspace_id,a.candidate_set_id,a.result_revision)= \
              (draft.tenant_id,draft.workspace_id,draft.candidate_set_id,draft.set_revision) \
         JOIN matrix_planning_selection_links l ON \
              (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)= \
              (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id) \
         JOIN native_planning_receipts receipt ON \
              (receipt.tenant_id,receipt.workspace_id,receipt.entity_id, \
               receipt.operation,receipt.request_id)= \
              (l.tenant_id,l.workspace_id,l.candidate_set_id, \
               l.operation,l.caller_request_id) \
         JOIN advisory_matrix_disposition d ON \
              (d.tenant_id,d.workspace_id,d.disposition_id)= \
              (l.tenant_id,l.workspace_id,l.disposition_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 \
           AND c.status='ready' AND c.latest_input=s.planning_latest_input \
           AND n.revision=s.scope_revision AND NOT n.payload_erased \
           AND n.source_snapshot_id=s.source_snapshot_id \
           AND source_set.current_snapshot_id=source.id \
           AND source_set.revision=s.source_candidate_set_revision \
           AND draft.set_revision=(SELECT MAX(latest.set_revision) \
                FROM slice_candidate_drafts latest \
                WHERE latest.tenant_id=c.tenant_id \
                  AND latest.workspace_id=c.workspace_id \
                  AND latest.candidate_set_id=c.id) \
           AND draft.set_revision < c.revision \
           AND NOT draft.payload_erased AND draft.payload IS NOT NULL \
           AND NOT review.payload_erased AND review.payload IS NOT NULL \
           AND review.payload->>'verdict'='ready' \
           AND review.payload->>'revision'=c.revision::text \
           AND NOT receipt.payload_erased \
           AND receipt.request_payload IS NOT NULL \
           AND receipt.result_payload IS NOT NULL \
           AND receipt.result_payload->'draft'=draft.payload \
           AND receipt.result_payload#>>'{candidate_set,revision}'=draft.set_revision::text \
           AND a.verdict='match' AND l.scope_id=c.scope_id \
           AND l.result_revision=draft.set_revision \
           AND d.outcome='selected' AND d.selected_choice_id=l.selected_choice_id \
           AND d.task_id=l.task_id AND d.matrix_task_revision=l.task_revision \
           AND NOT EXISTS (SELECT 1 FROM native_slices opened \
               WHERE opened.tenant_id=c.tenant_id AND opened.workspace_id=c.workspace_id \
                 AND opened.scope_id=c.scope_id AND opened.candidate_id=$4) \
         ORDER BY a.verified_at DESC,a.id DESC LIMIT 1",
    );
    if for_update {
        // The source snapshot and Matrix evidence are read-only to the runtime
        // role. Lock the mutable scope/source heads that the INSERT trigger
        // does not recheck; the trigger locks and rechecks draft/effect state.
        query.push_str(" FOR SHARE OF c,n,s,source_set");
    }
    let rows = sqlx::query(&query)
        .bind(tenant)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(work_node_id)
        .fetch_all(&mut **uow.transaction()?)
        .await
        .map_err(storage_error)?;
    let Some(row) = rows.into_iter().next() else {
        return Ok(None);
    };
    let caller_request_id: Uuid = row.try_get("caller_request_id").map_err(storage_error)?;
    let snapshot = uow
        .matrix_planning_effect_snapshot(
            workspace_id,
            candidate_set_id,
            caller_request_id,
            for_update,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    let attestation_digest: String = row.try_get("effect_digest").map_err(storage_error)?;
    let attestation_verifier: Uuid = row
        .try_get("verifier_principal_id")
        .map_err(storage_error)?;
    let draft_revision: i64 = row.try_get("draft_revision").map_err(storage_error)?;
    let ready_revision: i64 = row.try_get("set_revision").map_err(storage_error)?;
    if reviewed_effect_digest(&snapshot, workspace_id, draft_revision, ready_revision)?
        != attestation_digest
        || attestation_verifier == snapshot.link.caller_principal_id
        || attestation_verifier == snapshot.matrix_owner_principal_id
        || snapshot.link.selection.disposition_id
            != row
                .try_get::<Uuid, _>("disposition_id")
                .map_err(storage_error)?
    {
        return Err(Error::StaleContext);
    }
    let work = snapshot
        .saved_nodes
        .iter()
        .find(|node| node.id() == work_node_id)
        .cloned()
        .ok_or(Error::StaleContext)?;
    let SliceCandidateNode::Work {
        revision: work_revision,
        ..
    } = &work
    else {
        return Err(Error::StaleContext);
    };
    let work_revision = *work_revision;
    let task_id: Uuid = row.try_get("task_id").map_err(storage_error)?;
    let task = if for_update {
        uow.lock_matrix_task(workspace_id, task_id).await?
    } else {
        uow.matrix_task(workspace_id, task_id).await?
    }
    .ok_or(Error::StaleContext)?;
    let task_revision: i64 = row.try_get("task_revision").map_err(storage_error)?;
    let input_digest: String = row.try_get("input_digest").map_err(storage_error)?;
    let choice_set_digest: String = row.try_get("choice_set_digest").map_err(storage_error)?;
    let verification_digest: String = row.try_get("verification_digest").map_err(storage_error)?;
    if task.revision != task_revision
        || task.input_digest != input_digest
        || task.choice_set_digest.as_deref() != Some(choice_set_digest.as_str())
    {
        return Err(Error::StaleContext);
    }
    let verification = uow
        .matrix_verification_for_revision(workspace_id, task_id, task_revision, &input_digest)
        .await?
        .ok_or(Error::StaleContext)?;
    if verification.digest != verification_digest
        || verification.owner_principal != task.recorded_by_principal_id.to_string()
        || verification.verifier_principal == verification.owner_principal
    {
        return Err(Error::StaleContext);
    }
    let now: i64 = sqlx::query_scalar(
        "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
    )
    .fetch_one(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let validated = evaluate_matrix_verification(
        &task_id.to_string(),
        &task_revision.to_string(),
        &task.input,
        &verification,
        now,
    )
    .map_err(|_| Error::StaleContext)?;
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        task_id.to_string(),
        task_revision.to_string(),
        task.input.clone(),
    )
    .map_err(|_| Error::StaleContext)?;
    let composition = compose_independently_verified_owner_matrix(&reported, &validated)
        .map_err(|_| Error::StaleContext)?;
    let choice_set = task.choice_set.as_ref().ok_or(Error::StaleContext)?;
    if matrix_verified_disposition_digest(&task.input, &composition, choice_set, &validated)
        .map_err(|_| Error::StaleContext)?
        != row
            .try_get::<String, _>("evaluation_digest")
            .map_err(storage_error)?
        || composition.catalogue_version
            != row
                .try_get::<String, _>("catalogue_version")
                .map_err(storage_error)?
    {
        return Err(Error::StaleContext);
    }
    let selected_choice_id: String = row.try_get("selected_choice_id").map_err(storage_error)?;
    if !choice_set
        .candidates
        .iter()
        .any(|choice| choice.candidate_id == selected_choice_id)
    {
        return Err(Error::StaleContext);
    }
    let catalogue: PipelineCatalogueSnapshot = serde_json::from_value(
        row.try_get::<Value, _>("catalogue")
            .map_err(storage_error)?,
    )
    .map_err(|_| Error::StaleContext)?;
    catalogue.validate().map_err(|_| Error::StaleContext)?;
    let scope_id: Uuid = row.try_get("scope_id").map_err(storage_error)?;
    let saved_mandatory_card_ids = composition
        .mandatory_cards
        .iter()
        .map(|card| card.id.to_string())
        .collect();
    let source = PipelineRecommendationSource {
        work,
        current_work_revision: work_revision,
        matrix: PipelineMatrixBasis {
            input: task.input.clone(),
            choice_set: choice_set.clone(),
            composition,
            selected_choice_id: selected_choice_id.clone(),
            current_selected_choice_id: selected_choice_id,
            current_task_revision: task_revision.to_string(),
            choice_set_digest: choice_set_digest.clone(),
            current_choice_set_digest: choice_set_digest,
            verification_digest: verification_digest.clone(),
            current_verification_digest: verification_digest,
            saved_mandatory_card_ids,
        },
        catalogue,
        definitions: Vec::new(),
        compatibility_policy: PipelineCompatibilityPolicy::unavailable(),
        evidence_refs: Vec::new(),
    };
    Ok(Some(PipelineRecommendationBasis {
        scope_id,
        candidate_set_id,
        candidate_set_revision: row.try_get("set_revision").map_err(storage_error)?,
        planning_snapshot_id: row.try_get("planning_snapshot_id").map_err(storage_error)?,
        source_snapshot_id: row.try_get("source_snapshot_id").map_err(storage_error)?,
        source_candidate_set_revision: row
            .try_get("source_candidate_set_revision")
            .map_err(storage_error)?,
        selected_sources_digest: row
            .try_get("selected_sources_digest")
            .map_err(storage_error)?,
        matrix_disposition_id: row.try_get("disposition_id").map_err(storage_error)?,
        match_effect_attestation_id: row.try_get("attestation_id").map_err(storage_error)?,
        source,
    }))
}
