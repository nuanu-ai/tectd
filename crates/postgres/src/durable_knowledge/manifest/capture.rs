use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn capture(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    run_revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
) -> Result<Option<PipelineKnowledgeManifest>> {
    capture_inner(
        tx,
        tenant,
        workspace,
        run,
        run_revision,
        scope,
        slice,
        phase,
        session,
        None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn capture_with_proofs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    run_revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Option<PipelineKnowledgeManifest>> {
    capture_inner(
        tx,
        tenant,
        workspace,
        run,
        run_revision,
        scope,
        slice,
        phase,
        session,
        Some(proofs),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
async fn capture_inner(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    run_revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    mut proofs: Option<&mut crate::knowledge_lifecycle::PublicationProofScope>,
) -> Result<Option<PipelineKnowledgeManifest>> {
    let state:Option<(i64,bool)>=sqlx::query_as("SELECT generation,capability_ready FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2").bind(tenant).bind(workspace).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((generation, true)) = state else {
        return Ok(None);
    };
    delivery::require_identity_ready(tx).await?;
    let principal: Option<Uuid> = sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let principal = principal.ok_or(Error::Forbidden)?;
    if let Some(proofs) = proofs.as_ref() {
        proofs.require_identity(tenant, workspace, principal, session)?;
    }
    let inquiry_projection = inquiry::load(tx, tenant, workspace, run).await?;
    let (revisions, unresolved) = if inquiry_projection
        .as_ref()
        .and_then(|value| value.stage())
        .is_some()
    {
        (Vec::new(), Vec::new())
    } else {
        projection(tx, tenant, workspace, run, scope, slice, phase, principal).await?
    };
    for value in &revisions {
        let rows = rdf::native_rows(
            tx,
            tenant,
            workspace,
            value.unit_id,
            value.revision,
            value.publication_event_id,
        )
        .await?;
        rdf::validate_rows(&rows, value)?;
    }
    let selected = items(&revisions);
    if serde_json::to_vec(&(&selected, &unresolved))
        .map_err(storage_error)?
        .len()
        > DK_MAX_MANIFEST_BYTES
    {
        return Err(Error::CapacityExceeded);
    }
    let semantic_digest = legacy::semantic(&selected, &unresolved)?;
    let id = Uuid::new_v4();
    let mut resource = match proofs.as_mut() {
        Some(proofs) => {
            generic::snapshot_with_proofs(
                tx,
                tenant,
                workspace,
                principal,
                run,
                run_revision,
                scope,
                slice,
                phase,
                id,
                String::new(),
                session,
                proofs,
            )
            .await?
        }
        None => {
            generic::snapshot(
                tx,
                tenant,
                workspace,
                principal,
                run,
                run_revision,
                scope,
                slice,
                phase,
                id,
                String::new(),
            )
            .await?
        }
    }
    .manifest;
    let base_digest = digest(&(
        id,
        generation,
        run,
        run_revision,
        phase,
        &selected,
        &unresolved,
        &semantic_digest,
        &resource.semantic_digest,
        &resource.definition_version,
        &resource.definition_digest,
        &resource.method_requirements,
        &resource.selected,
        &resource.unresolved_needs,
        &resource.freshness_warnings,
    ))?;
    let digest_value = if resource.inquiry.is_some() {
        digest(&(&base_digest, &resource.inquiry, &resource.projection_policy))?
    } else {
        base_digest
    };
    resource.digest = digest_value.clone();
    let value = PipelineKnowledgeManifest {
        id,
        digest: digest_value,
        semantic_digest,
        workspace_generation: generation,
        run_id: run,
        run_revision,
        phase_id: phase.into(),
        selected,
        unresolved_needs: unresolved,
    };
    if serde_json::to_vec(&(&value, &resource))
        .map_err(storage_error)?
        .len()
        > DK_MAX_MANIFEST_BYTES
    {
        return Err(Error::CapacityExceeded);
    }
    sqlx::query("INSERT INTO pipeline_knowledge_manifests(id,tenant_id,workspace_id,run_id,run_revision,phase_id,workspace_generation,digest,semantic_digest,selected,unresolved_needs,contract_version,definition_version,definition_digest,method_requirements,selected_resources,resource_unresolved_needs,freshness_warnings,resource_semantic_digest,resource_inquiry,resource_projection_policy) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'dk-2',$12,$13,$14,$15,$16,$17,$18,$19,$20)")
        .bind(id).bind(tenant).bind(workspace).bind(run).bind(run_revision).bind(phase).bind(generation).bind(&value.digest).bind(&value.semantic_digest).bind(json(&value.selected)?).bind(json(&value.unresolved_needs)?)
        .bind(&resource.definition_version).bind(&resource.definition_digest).bind(json(&resource.method_requirements)?).bind(json(&resource.selected)?).bind(json(&resource.unresolved_needs)?).bind(json(&resource.freshness_warnings)?).bind(&resource.semantic_digest)
        .bind(resource.inquiry.as_ref().map(json).transpose()?)
        .bind(inquiry_projection.as_ref().map(|value| value.policy_name()))
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_pipeline_manifest_copies(tx, tenant, workspace, id)
        .await?;
    crate::knowledge_maintenance::register_manifest_consumers(tx, tenant, workspace, id).await?;
    Ok(Some(value))
}
