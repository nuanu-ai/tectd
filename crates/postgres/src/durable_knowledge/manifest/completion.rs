use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn validate_current_backend_knowledge(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    manifest: Option<&PipelineKnowledgeManifest>,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Option<ConsumedKnowledgeManifestRef>> {
    validate_completion_inner(
        tx,
        tenant,
        workspace,
        run,
        scope,
        slice,
        phase,
        session,
        manifest,
        None,
        Some(proofs),
        true,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn validate_completion_with_proofs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    manifest: Option<&PipelineKnowledgeManifest>,
    consumed: Option<&ConsumedKnowledgeManifestRef>,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<()> {
    validate_completion_inner(
        tx,
        tenant,
        workspace,
        run,
        scope,
        slice,
        phase,
        session,
        manifest,
        consumed,
        Some(proofs),
        false,
    )
    .await
    .map(|_| ())
}

#[allow(clippy::too_many_arguments)]
async fn validate_completion_inner(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    manifest: Option<&PipelineKnowledgeManifest>,
    consumed: Option<&ConsumedKnowledgeManifestRef>,
    mut proofs: Option<&mut crate::knowledge_lifecycle::PublicationProofScope>,
    derive_backend_binding: bool,
) -> Result<Option<ConsumedKnowledgeManifestRef>> {
    let principal: Option<Uuid> = sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let principal = principal.ok_or(Error::Forbidden)?;
    if let Some(proofs) = proofs.as_ref() {
        proofs.require_identity(tenant, workspace, principal, session)?;
    }
    let (generation, ready, current, _, gaps) =
        preview(tx, tenant, workspace, run, scope, slice, phase, principal).await?;
    if !ready {
        return if manifest.is_none() && consumed.is_none() {
            Ok(None)
        } else {
            Err(Error::ContextChanged)
        };
    }
    let Some(manifest) = manifest else {
        return Err(Error::NeedsContext);
    };
    let current_run_revision: i64 = sqlx::query_scalar("SELECT revision FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND scope_id=$4 AND slice_id=$5")
        .bind(tenant).bind(workspace).bind(run).bind(scope).bind(slice)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    let resources = load_resources(tx, tenant, workspace, Some(manifest.id), principal).await?;
    let current_resources = match proofs.as_mut() {
        Some(proofs) => {
            generic::snapshot_with_proofs(
                tx,
                tenant,
                workspace,
                principal,
                run,
                current_run_revision,
                scope,
                slice,
                phase,
                manifest.id,
                manifest.digest.clone(),
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
                current_run_revision,
                scope,
                slice,
                phase,
                manifest.id,
                manifest.digest.clone(),
            )
            .await?
        }
    };
    if !gaps.is_empty() {
        return Err(Error::NeedsContext);
    }
    if !current_resources.blocking_gaps.is_empty() {
        return Err(Error::NeedsContext);
    }
    if manifest.workspace_generation != generation || manifest.semantic_digest != current {
        return Err(Error::ContextChanged);
    }
    let Some(resources) = resources.as_ref() else {
        return Err(Error::ContextChanged);
    };
    let resource_current = {
        let stored = resources;
        stored.workspace_generation == generation
            && stored.run_revision == current_run_revision
            && stored.digest == manifest.digest
            && stored.semantic_digest == current_resources.manifest.semantic_digest
            && stored.definition_version == current_resources.manifest.definition_version
            && stored.definition_digest == current_resources.manifest.definition_digest
            && stored.method_requirements == current_resources.manifest.method_requirements
            && stored.inquiry == current_resources.manifest.inquiry
            && stored.projection_policy == current_resources.manifest.projection_policy
    };
    if !resource_current {
        return Err(Error::ContextChanged);
    }
    let resource_selected = !resources.selected.is_empty();
    if manifest.selected.is_empty() && !resource_selected {
        if consumed.is_some() {
            return Err(Error::StaleContext);
        };
        Ok(None)
    } else if derive_backend_binding
        || consumed.is_some_and(|v| v.manifest_id == manifest.id && v.digest == manifest.digest)
    {
        Ok(Some(ConsumedKnowledgeManifestRef {
            manifest_id: manifest.id,
            digest: manifest.digest.clone(),
        }))
    } else {
        Err(Error::NeedsContext)
    }
}
