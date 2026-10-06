//! Authenticated, read-only caller selection. No host dispatch is exposed here.
use crate::{
    MatrixPlanningSelectionLink, MatrixRequirementsLocator, MatrixTaskSource, TransactionMode,
    WorkspaceService,
};
use tect_domain::{
    AdvisoryRequestPreference, CapturedModelRouteDecision, CapturedModelRouteDisposition, Error,
    MODEL_ROUTE_HOST_SELECTION_SCHEMA, MODEL_ROUTE_OWNED_STDIO_HOST, ModelRouteCatalogue,
    ModelRouteDecisionOutcome, ModelRouteDispositionAction, ModelRouteExecutionDimension,
    ModelRouteExecutionSourceBinding, ModelRouteFact, ModelRouteHostSelectionMaterial,
    ModelRoutePreparation, ModelRouteSourceLocator, PreparedModelRouteRecommendation,
    RequestContext, Result,
};
use uuid::Uuid;

/// Caller supplies pins and one explicit route ID, never provider/model/effort.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareModelRouteHostSelection {
    pub preparation_request_key: String,
    pub decision_id: Uuid,
    pub disposition_id: Uuid,
    pub expected_task_id: Uuid,
    pub expected_task_revision: i64,
    pub expected_work_context_digest: String,
    pub expected_catalogue_digest: String,
    pub selected_route_id: String,
    pub input_sha256: String,
    pub invocation_key: String,
}

/// Created only after current Owner/session/source checks and the read commit.
/// This is snapshot-time authorization, not continuous host execution authority.
#[derive(Debug)]
pub struct CurrentModelRouteHostSelection {
    material: ModelRouteHostSelectionMaterial,
    material_json: String,
    material_sha256: String,
}

impl CurrentModelRouteHostSelection {
    pub fn material(&self) -> &ModelRouteHostSelectionMaterial {
        &self.material
    }
    pub fn material_json(&self) -> &str {
        &self.material_json
    }
    pub fn material_sha256(&self) -> &str {
        &self.material_sha256
    }
}

impl WorkspaceService {
    /// Authenticated source read, also exposed by the read-only host query.
    /// Its serialized material is not portable dispatch authorization.
    pub async fn prepare_model_route_host_selection(
        &self,
        context: &RequestContext,
        request: &PrepareModelRouteHostSelection,
    ) -> Result<CurrentModelRouteHostSelection> {
        validate_request(request)?;
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadWrite).await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let prepared = tx
            .model_route_recommendation_store()
            .ok_or(Error::Forbidden)?
            .by_request(workspace.id, &request.preparation_request_key)
            .await?
            .ok_or(Error::NotFound)?;
        // Reuse the existing candidate/task/declaration lock order before
        // supplementing that check with the original bound source locator.
        crate::model_route_authority::validate_current_route_authority(
            self,
            &mut *tx,
            workspace.id,
            identity.principal_id,
            &prepared,
        )
        .await?;
        let source = tx
            .matrix_task_source(workspace.id, request.expected_task_id)
            .await?
            .ok_or(Error::NotFound)?;
        let binding = source
            .requirements_binding
            .as_ref()
            .ok_or(Error::InputConflict)?;
        crate::matrix_verification::lock_and_load_bound_matrix_context(
            tx.matrix_requirements_context_store()
                .ok_or(Error::Forbidden)?,
            workspace.id,
            identity.principal_id,
            binding,
        )
        .await
        .map_err(|_| Error::StaleContext)?;
        let locked = tx
            .lock_matrix_task(workspace.id, request.expected_task_id)
            .await?
            .ok_or(Error::NotFound)?;
        let reread = tx
            .matrix_task_source(workspace.id, request.expected_task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if reread != source || locked != source.revision {
            return Err(Error::StaleRevision);
        }
        let decision = tx
            .model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .decision_by_id(workspace.id, request.decision_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .validate_current_decision(&decision)
            .await?;
        let disposition = tx
            .model_route_decision_store()
            .ok_or(Error::Forbidden)?
            .disposition_by_id(workspace.id, request.disposition_id)
            .await?
            .ok_or(Error::NotFound)?;
        let link = tx
            .matrix_planning_selection_link(
                workspace.id,
                prepared.work.selection_link.candidate_set_id,
                prepared.work.selection_link.caller_request_id,
            )
            .await?
            .ok_or(Error::StaleContext)?;
        let preference = tx
            .session_advisory_preference(workspace.id, session.id)
            .await?
            .preference;
        let (host_provider, catalogue_provider) = self.model_route_advisory_inputs();
        let material = selection_material(
            request,
            workspace.id,
            identity.principal_id,
            session.id,
            preference,
            &prepared,
            &decision,
            &disposition,
            &source,
            &link,
            catalogue_provider.catalogue()?.as_ref(),
            &host_provider.host_capabilities()?,
        )?;
        let material_json = material.canonical_json()?;
        let material_sha256 = material.canonical_digest()?;
        tx.commit().await?;
        Ok(CurrentModelRouteHostSelection {
            material,
            material_json,
            material_sha256,
        })
    }
}

fn validate_request(request: &PrepareModelRouteHostSelection) -> Result<()> {
    if request.decision_id.is_nil()
        || request.disposition_id.is_nil()
        || request.expected_task_id.is_nil()
        || request.expected_task_revision < 1
        || !key(&request.preparation_request_key, 256)
        || !key(&request.selected_route_id, 128)
        || !key(&request.invocation_key, 256)
        || !sha(&request.expected_work_context_digest)
        || !sha(&request.expected_catalogue_digest)
        || !sha(&request.input_sha256)
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}
fn key(value: &str, limit: usize) -> bool {
    !value.is_empty()
        && value.len() <= limit
        && value.trim() == value
        && !value.chars().any(char::is_control)
}
fn sha(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
}

#[allow(clippy::too_many_arguments)]
fn selection_material(
    request: &PrepareModelRouteHostSelection,
    workspace_id: Uuid,
    actor_id: Uuid,
    session_id: Uuid,
    preference: AdvisoryRequestPreference,
    prepared: &PreparedModelRouteRecommendation,
    decision: &CapturedModelRouteDecision,
    disposition: &CapturedModelRouteDisposition,
    source: &MatrixTaskSource,
    link: &MatrixPlanningSelectionLink,
    current_catalogue: Option<&ModelRouteCatalogue>,
    current_host: &ModelRouteFact<Vec<String>>,
) -> Result<ModelRouteHostSelectionMaterial> {
    validate_request(request)?;
    let selection = &prepared.work.approved_matrix_selection;
    let authority = prepared.work.require_current_authority()?;
    let binding = source
        .requirements_binding
        .as_ref()
        .ok_or(Error::InputConflict)?;
    let provenance = link
        .context_provenance
        .as_ref()
        .ok_or(Error::InputConflict)?;
    let catalogue = prepared.catalogue.as_ref().ok_or(Error::InputConflict)?;
    let eligible = catalogue.eligible(&prepared.work)?;
    if workspace_id.is_nil()
        || actor_id.is_nil()
        || session_id.is_nil()
        || prepared.workspace_id != workspace_id
        || prepared.origin_session_id.is_none_or(|id| id.is_nil())
        || prepared.request_key != request.preparation_request_key
        || prepared.preparation != ModelRoutePreparation::Prepared
        || prepared.session_preference != preference
        || preference == AdvisoryRequestPreference::Skip
        || prepared.request_preference == AdvisoryRequestPreference::Skip
        || current_catalogue != Some(catalogue)
        || current_host != &prepared.work.host_capabilities
        || prepared.eligible.as_ref() != Some(&eligible)
        || eligible.catalogue_digest != request.expected_catalogue_digest
        || eligible.work_context_digest != request.expected_work_context_digest
        || selection.task_id != request.expected_task_id
        || selection.task_revision != request.expected_task_revision
        || source.revision.task_id != selection.task_id
        || source.revision.revision != selection.task_revision
        || source.revision.input_digest != selection.expected_input_digest
        || source.revision.choice_set_digest.as_deref()
            != Some(selection.expected_choice_set_digest.as_str())
        || binding.snapshot_id != authority.frozen_snapshot_id
        || binding.authority_schema != authority.authority_schema
        || binding.semantic_digest != authority.requirements_semantic_digest
        || provenance.frozen_snapshot_id != binding.snapshot_id
        || provenance.authority_schema != binding.authority_schema
        || provenance.requirements_semantic_digest != binding.semantic_digest
        || link.selection != *selection
        || link.candidate_set_id != prepared.work.selection_link.candidate_set_id
        || link.caller_request_id != prepared.work.selection_link.caller_request_id
        || link.caller_principal_id.is_nil()
        || link.caller_session_id.is_nil()
        || link.scope_id.is_nil()
        || link.result_revision < 1
        || !sha(&link.evaluation_digest)
        || link.catalogue_version.is_empty()
        || link
            .mapped_nodes
            .iter()
            .filter(|node| {
                node.draft_index == prepared.work.selection_link.mapped_draft_node_index
                    && node.node_id == prepared.work.selection_link.mapped_work_node_id
                    && node.node_revision == prepared.work.selection_link.mapped_work_node_revision
            })
            .count()
            != 1
        || source.revision.request_id.is_nil()
        || source.revision.recorded_by_principal_id.is_nil()
        || source.revision.recorded_by_session_id.is_nil()
        || decision.id != request.decision_id
        || decision.prepared != *prepared
        || disposition.id != request.disposition_id
        || disposition.decision_id != decision.id
        || disposition.workspace_id != workspace_id
        || disposition.actor_id != actor_id
        || disposition.action != ModelRouteDispositionAction::Accept
        || prepared.routes.recommended_route_id.is_some()
        || prepared.routes.observed_actual.is_some()
        || decision.routes.observed_actual.is_some()
        || decision.routes.requested_route_id != prepared.routes.requested_route_id
        || !eligible.route_ids.contains(&request.selected_route_id)
    {
        return Err(Error::InputConflict);
    }
    let ModelRouteDecisionOutcome::Recommended { route_id } = &decision.outcome else {
        return Err(Error::InputConflict);
    };
    if decision.routes.recommended_route_id.as_deref() != Some(route_id)
        || !eligible.route_ids.contains(route_id)
    {
        return Err(Error::InputConflict);
    }
    let tect_domain::ModelRouteDecisionInput::Ranking(ranking) = &decision.input else {
        return Err(Error::InputConflict);
    };
    if eligible.recommendation(ranking)?.as_deref() != Some(route_id) {
        return Err(Error::InputConflict);
    }
    let dimension = |id: &str| -> Result<ModelRouteExecutionDimension> {
        let route = catalogue
            .routes
            .iter()
            .find(|route| route.id == id)
            .ok_or(Error::InputConflict)?;
        Ok(ModelRouteExecutionDimension {
            route_id: route.id.clone(),
            provider: route.provider.clone(),
            model: route.model.clone(),
            effort: route.effort.clone(),
        })
    };
    let selected_route = dimension(&request.selected_route_id)?;
    // This material targets the existing owned-stdio OpenAI observer. Other
    // catalogue providers remain audit data, not a fallback or alias here.
    if selected_route.provider != "openai" {
        return Err(Error::InputConflict);
    }
    Ok(ModelRouteHostSelectionMaterial {
        schema: MODEL_ROUTE_HOST_SELECTION_SCHEMA.into(),
        intended_host_kind: MODEL_ROUTE_OWNED_STDIO_HOST.into(),
        workspace_id,
        invoking_actor_id: actor_id,
        invoking_session_id: session_id,
        preparation: prepared.clone(),
        decision: decision.clone(),
        disposition: disposition.clone(),
        source_binding: ModelRouteExecutionSourceBinding {
            locator: source_locator(&binding.locator),
            source_request_id: source.revision.request_id,
            source_recorded_by_actor_id: source.revision.recorded_by_principal_id,
            source_recorded_by_session_id: source.revision.recorded_by_session_id,
            frozen_snapshot_id: binding.snapshot_id,
            authority_schema: binding.authority_schema.clone(),
            requirements_semantic_digest: binding.semantic_digest.clone(),
            matrix_save_actor_id: link.caller_principal_id,
            matrix_save_session_id: link.caller_session_id,
            scope_id: link.scope_id,
            result_revision: link.result_revision,
            matrix_evaluation_digest: link.evaluation_digest.clone(),
            matrix_catalogue_version: link.catalogue_version.clone(),
        },
        requested_route: prepared
            .routes
            .requested_route_id
            .as_deref()
            .map(dimension)
            .transpose()?,
        recommended_route: Some(dimension(route_id)?),
        configured_route: selected_route.clone(),
        selected_route,
        input_sha256: request.input_sha256.clone(),
        invocation_key: request.invocation_key.clone(),
    })
}

fn source_locator(locator: &MatrixRequirementsLocator) -> ModelRouteSourceLocator {
    match *locator {
        MatrixRequirementsLocator::Program { program_id } => {
            ModelRouteSourceLocator::Program { program_id }
        }
        MatrixRequirementsLocator::Scope {
            program_id,
            scope_id,
        } => ModelRouteSourceLocator::Scope {
            program_id,
            scope_id,
        },
        MatrixRequirementsLocator::Slice {
            program_id,
            scope_id,
            candidate_set_id,
            work_candidate_id,
            expected_work_revision,
        } => ModelRouteSourceLocator::Slice {
            program_id,
            scope_id,
            candidate_set_id,
            work_candidate_id,
            expected_work_revision,
        },
        MatrixRequirementsLocator::OpenedSlice { slice_id } => {
            ModelRouteSourceLocator::OpenedSlice { slice_id }
        }
    }
}

#[cfg(test)]
mod tests;
