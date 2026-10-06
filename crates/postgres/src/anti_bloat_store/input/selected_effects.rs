use super::*;

pub(super) async fn add_selected_effects(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    scope_id: Uuid,
    scope_candidate_id: Uuid,
    slice_set_id: Uuid,
    obligations: &mut BTreeMap<String, AntiBloatProtectedObligation>,
    verifier_readback: bool,
) -> Result<()> {
    let tenant = uow.tenant_id()?;
    let current_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 \
         AND id=$3 AND scope_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice_set_id)
    .bind(scope_id)
    .fetch_one(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let links: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT caller_request_id,result_revision FROM matrix_planning_selection_links \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 ORDER BY result_revision,caller_request_id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice_set_id)
    .fetch_all(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if links.is_empty() {
        return Ok(());
    }
    let current = links
        .iter()
        .filter(|(_, revision)| *revision == current_revision)
        .collect::<Vec<_>>();
    if current.len() != 1 {
        return Err(Error::StaleContext);
    }
    let caller_request = current[0].0;
    let snapshot = uow
        .matrix_planning_effect_snapshot(workspace, slice_set_id, caller_request, false)
        .await?
        .ok_or(Error::StaleContext)?;
    let material = snapshot.material(workspace)?;
    if material.scope_id != scope_id || material.context_provenance.is_none() {
        return Err(Error::StaleContext);
    }
    // Reuse the S02 authority check: task, verified V2 source binding, current
    // declaration lineage, owner choice and saved receipt are re-evaluated.
    let (evaluation_digest, catalogue_version) = if verifier_readback {
        crate::matrix_planning_selection_store::current_context_evaluation_for_verifier(
            uow,
            workspace,
            &snapshot.link,
        )
        .await?
    } else {
        crate::matrix_planning_selection_store::current_context_evaluation(
            uow,
            workspace,
            &snapshot.link,
        )
        .await?
    };
    if evaluation_digest != snapshot.link.evaluation_digest
        || catalogue_version != snapshot.link.catalogue_version
    {
        return Err(Error::StaleContext);
    }
    let effect_digest = material.canonical_digest()?;
    let attestations: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT id,verdict,effect_digest FROM matrix_planning_effect_attestations \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 \
           AND caller_request_id=$4 AND result_revision=$5 ORDER BY id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice_set_id)
    .bind(caller_request)
    .bind(current_revision)
    .fetch_all(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if attestations.is_empty()
        || attestations
            .iter()
            .any(|(_, verdict, digest)| verdict != "match" || digest != &effect_digest)
    {
        return Err(Error::StaleContext);
    }
    add_obligation(
        obligations,
        AntiBloatProtectedObligation {
            id: format!(
                "matrix-choice:{}:{}",
                material.task_id, material.selected_choice.candidate_id
            ),
            content_digest: content_digest(&material.selected_choice)?,
            origin: AntiBloatObligationOrigin::MatrixSelectedChoice,
            scope_candidate_id: Some(scope_candidate_id),
        },
    )?;
    for node in &material.nodes {
        add_obligation(
            obligations,
            AntiBloatProtectedObligation {
                id: format!("matrix-node:{slice_set_id}:{}", node.node_id),
                content_digest: content_digest(&node.body)?,
                origin: AntiBloatObligationOrigin::MatrixMappedNode,
                scope_candidate_id: Some(scope_candidate_id),
            },
        )?;
    }
    let provenance = material
        .context_provenance
        .as_ref()
        .ok_or(Error::StaleContext)?;
    let frozen = uow
        .frozen_matrix_requirements_by_id(workspace, provenance.frozen_snapshot_id)
        .await?
        .ok_or(Error::StaleContext)?;
    if frozen.effective.semantic_digest() != provenance.requirements_semantic_digest
        || frozen.effective.schema() != provenance.authority_schema
    {
        return Err(Error::StaleContext);
    }
    for (path, resolved) in frozen.effective.values() {
        let anchor = serde_json::to_string(&resolved.source.anchor).map_err(storage_error)?;
        add_obligation(
            obligations,
            AntiBloatProtectedObligation {
                id: format!("matrix-declaration:{anchor}:{}", path.fact_path()),
                content_digest: content_digest(&resolved.value)?,
                origin: AntiBloatObligationOrigin::MatrixDeclaration,
                scope_candidate_id: match resolved.source.anchor {
                    RequirementsAnchor::Program { .. } => None,
                    _ => Some(scope_candidate_id),
                },
            },
        )?;
    }
    let effect_ids = attestations
        .iter()
        .map(|(id, _, _)| *id)
        .collect::<BTreeSet<_>>();
    let selected: Vec<(Uuid, Uuid, Uuid, serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT d.opportunity_id,d.work_node_id,c.match_effect_attestation_id,c.manifest_payload,d.result_payload \
         FROM pipeline_advice_dispositions d JOIN pipeline_advice_contexts c \
           ON (c.tenant_id,c.workspace_id,c.opportunity_id)= \
              (d.tenant_id,d.workspace_id,d.opportunity_id) \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.candidate_set_id=$3 ORDER BY d.disposition_id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice_set_id)
    .fetch_all(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    for (opportunity_id, work_id, attestation_id, manifest_json, disposition_json) in selected {
        if !effect_ids.contains(&attestation_id)
            || !material.nodes.iter().any(|node| node.node_id == work_id)
        {
            return Err(Error::StaleContext);
        }
        let manifest: tect_domain::PipelineRecommendationManifest =
            serde_json::from_value(manifest_json).map_err(|_| Error::StaleContext)?;
        let disposition: tect_domain::PipelineDispositionResult =
            serde_json::from_value(disposition_json).map_err(|_| Error::StaleContext)?;
        manifest
            .validate_digest()
            .map_err(|_| Error::StaleContext)?;
        let option_id = disposition
            .selected_option_id
            .as_deref()
            .ok_or(Error::StaleContext)?;
        let option = manifest
            .options
            .iter()
            .find(|option| option.id == option_id)
            .ok_or(Error::StaleContext)?;
        if manifest.work_id != work_id
            || disposition.work_id != work_id
            || disposition.request.manifest_digest != manifest.digest
            || disposition.selected_kind != Some(option.kind)
        {
            return Err(Error::StaleContext);
        }
        let basis = if verifier_readback {
            crate::pipeline_disposition_store::load_basis_for_verifier(
                uow,
                workspace,
                opportunity_id,
            )
            .await?
        } else {
            crate::pipeline_disposition_store::load_basis(uow, workspace, opportunity_id).await?
        }
        .ok_or(Error::StaleContext)?;
        let current = if verifier_readback {
            crate::pipeline_disposition_store::is_current_for_verifier(uow, workspace, &basis)
                .await?
        } else {
            crate::pipeline_disposition_store::is_current(uow, workspace, &basis).await?
        };
        if !current
            || basis.prepared.context.candidate_set_id != slice_set_id
            || basis.prepared.context.work_node_id != work_id
            || basis.prepared.context.match_effect_attestation_id != attestation_id
            || basis.prepared.manifest != manifest
            || disposition
                .request
                .resolve(
                    disposition.id,
                    &basis.prepared.manifest,
                    &basis.saved_work,
                    &basis.advice,
                )
                .map_err(|_| Error::StaleContext)?
                != disposition
        {
            return Err(Error::StaleContext);
        }
        add_obligation(
            obligations,
            AntiBloatProtectedObligation {
                id: format!("pipeline-option:{slice_set_id}:{work_id}:{option_id}"),
                content_digest: content_digest(option)?,
                origin: AntiBloatObligationOrigin::PipelineSelectedOption,
                scope_candidate_id: Some(scope_candidate_id),
            },
        )?;
    }
    Ok(())
}
