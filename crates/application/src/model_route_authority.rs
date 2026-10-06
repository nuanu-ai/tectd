//! Current external Matrix trust and live route registries at authority transitions.
//! Historical reads and committed-send raw sealing deliberately do not use this gate.
use crate::{PreparedModelRouteRecommendation, UnitOfWork, WorkspaceService};
use tect_domain::{Error, ModelRouteCatalogue, ModelRouteFact, Result};
use uuid::Uuid;

pub(crate) async fn validate_current_route_authority(
    service: &WorkspaceService,
    tx: &mut dyn UnitOfWork,
    workspace_id: Uuid,
    principal_id: Uuid,
    prepared: &PreparedModelRouteRecommendation,
) -> Result<()> {
    if prepared.workspace_id != workspace_id {
        return Err(Error::StaleContext);
    }
    // Retain the adapter's candidate/task/declaration lock order and saved Work checks.
    tx.model_route_recommendation_store()
        .ok_or(Error::Forbidden)?
        .validate_current(prepared)
        .await?;
    let authority = prepared.work.require_current_authority()?;
    let selection = &prepared.work.approved_matrix_selection;
    // This existing seam checks the singleton disposition digest and invokes the
    // CURRENT validator for every operating evidence binding, never trusting SQL alone.
    let (evaluation_digest, catalogue_version, provenance) =
        crate::native_planning::validate_selected_matrix_plan(
            service,
            tx,
            workspace_id,
            principal_id,
            selection,
        )
        .await?;
    let link = tx
        .matrix_planning_selection_link(
            workspace_id,
            prepared.work.selection_link.candidate_set_id,
            prepared.work.selection_link.caller_request_id,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    if link.selection != *selection
        || link.evaluation_digest != evaluation_digest
        || link.catalogue_version != catalogue_version
        || link.context_provenance.as_ref() != Some(&provenance)
        || provenance.frozen_snapshot_id != authority.frozen_snapshot_id
        || provenance.authority_schema != authority.authority_schema
        || provenance.requirements_semantic_digest != authority.requirements_semantic_digest
    {
        return Err(Error::StaleContext);
    }
    let (host_provider, catalogue_provider) = service.model_route_advisory_inputs();
    require_current_route_inputs(
        prepared,
        catalogue_provider.catalogue()?.as_ref(),
        &host_provider.host_capabilities()?,
    )
}

pub(crate) fn require_current_route_inputs(
    prepared: &PreparedModelRouteRecommendation,
    current_catalogue: Option<&ModelRouteCatalogue>,
    current_host: &ModelRouteFact<Vec<String>>,
) -> Result<()> {
    prepared.work.require_current_authority()?;
    if current_catalogue != prepared.catalogue.as_ref()
        || current_host != &prepared.work.host_capabilities
    {
        return Err(Error::StaleContext);
    }
    let eligible = current_catalogue
        .map(|catalogue| catalogue.eligible(&prepared.work))
        .transpose()?;
    if eligible != prepared.eligible {
        return Err(Error::StaleContext);
    }
    Ok(())
}
