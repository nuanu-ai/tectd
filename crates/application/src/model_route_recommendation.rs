//! Prepare a durable recommendation opportunity without ranking or dispatch.
use crate::{
    ModelRouteCatalogueProvider, ModelRoutePreparation, ModelRouteRecommendationStore,
    PreparedModelRouteRecommendation,
};
use tect_domain::{
    AdvisoryRequestPreference, Error, ModelRouteRecord, Result, WorkspaceAdvisoryMode,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PrepareModelRouteRecommendation {
    pub workspace_id: Uuid,
    pub disposition_id: Uuid,
    pub expected_task_id: Uuid,
    pub expected_task_revision: i64,
    pub expected_candidate_set_id: Uuid,
    pub expected_caller_request_id: Uuid,
    pub expected_mapped_work_node_id: Uuid,
    pub expected_mapped_work_node_revision: i64,
    pub request_key: String,
    pub requested_route_id: Option<String>,
    pub session_preference: AdvisoryRequestPreference,
    pub request_preference: AdvisoryRequestPreference,
}

impl PrepareModelRouteRecommendation {
    fn validate(&self) -> Result<()> {
        if self.workspace_id.is_nil()
            || self.disposition_id.is_nil()
            || self.expected_task_id.is_nil()
            || self.expected_task_revision < 1
            || self.expected_candidate_set_id.is_nil()
            || self.expected_caller_request_id.is_nil()
            || self.expected_mapped_work_node_id.is_nil()
            || self.expected_mapped_work_node_revision < 1
            || self.request_key.is_empty()
            || self.request_key.len() > 256
            || self.request_key.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }

    pub async fn prepare(
        &self,
        store: &mut dyn ModelRouteRecommendationStore,
        catalogue_provider: &dyn ModelRouteCatalogueProvider,
    ) -> Result<PreparedModelRouteRecommendation> {
        self.validate()?;
        if let Some(saved) = store
            .by_request(self.workspace_id, &self.request_key)
            .await?
        {
            if saved.workspace_id != self.workspace_id
                || saved.work.approved_matrix_selection.disposition_id != self.disposition_id
                || saved.work.approved_matrix_selection.task_id != self.expected_task_id
                || saved.work.approved_matrix_selection.task_revision != self.expected_task_revision
                || !self.matches_selection_link(&saved.work)
                || saved.routes.requested_route_id != self.requested_route_id
                || saved.session_preference != self.session_preference
                || saved.request_preference != self.request_preference
            {
                return Err(Error::InputConflict);
            }
            return Ok(saved);
        }
        let basis = store
            .load_basis(self.workspace_id, self.disposition_id)
            .await?
            .ok_or(Error::NotFound)?;
        let selection = &basis.work.approved_matrix_selection;
        selection.validate()?;
        if selection.disposition_id != self.disposition_id
            || selection.task_id != self.expected_task_id
            || selection.task_revision != self.expected_task_revision
            || !self.matches_selection_link(&basis.work)
        {
            return Err(Error::StaleContext);
        }
        basis.work.digest()?;
        let catalogue = catalogue_provider.catalogue()?;
        let eligible = catalogue
            .as_ref()
            .map(|snapshot| snapshot.eligible(&basis.work))
            .transpose()?;
        // Even an ineligible request is retained when it names a configured ID.
        // An unconfigured request is rejected; no catalogue cannot validate one.
        let routes = match &eligible {
            Some(eligible) => {
                eligible.record(self.requested_route_id.clone(), None, basis.observed_actual)?
            }
            None if self.requested_route_id.is_some() => return Err(Error::InvalidArguments),
            None => ModelRouteRecord {
                requested_route_id: None,
                recommended_route_id: None,
                observed_actual: basis.observed_actual,
            },
        };
        let preparation = if basis.advisory_mode == WorkspaceAdvisoryMode::Disabled {
            ModelRoutePreparation::WorkspaceDisabled
        } else if self.session_preference == AdvisoryRequestPreference::Skip {
            ModelRoutePreparation::SessionSkip
        } else if self.request_preference == AdvisoryRequestPreference::Skip {
            ModelRoutePreparation::RequestSkip
        } else if eligible.is_none() {
            ModelRoutePreparation::CapabilityUnavailable
        } else if basis.work.has_unknown_facts() {
            ModelRoutePreparation::UnknownWorkFacts
        } else if eligible
            .as_ref()
            .is_some_and(|set| set.route_ids.is_empty())
        {
            ModelRoutePreparation::NoEligibleRoutes
        } else {
            ModelRoutePreparation::Prepared
        };
        let prepared = PreparedModelRouteRecommendation {
            workspace_id: self.workspace_id,
            request_key: self.request_key.clone(),
            session_preference: self.session_preference,
            request_preference: self.request_preference,
            work: basis.work,
            catalogue,
            eligible,
            preparation,
            routes,
        };
        let saved = store.capture(&prepared).await?;
        if saved != prepared {
            return Err(Error::InternalInvariant);
        }
        Ok(saved)
    }

    fn matches_selection_link(&self, work: &tect_domain::ModelRouteWorkContext) -> bool {
        let link = &work.selection_link;
        link.candidate_set_id == self.expected_candidate_set_id
            && link.caller_request_id == self.expected_caller_request_id
            && link.mapped_work_node_id == self.expected_mapped_work_node_id
            && link.mapped_work_node_revision == self.expected_mapped_work_node_revision
    }
}

#[cfg(test)]
mod tests;
