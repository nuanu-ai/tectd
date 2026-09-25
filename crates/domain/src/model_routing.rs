//! Recommendation-only model routing. No provider client or execution operation lives here.
use crate::{Error, MatrixPlanningSelection, Result};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;

pub const MODEL_ROUTE_CATALOGUE_SCHEMA: &str = "tect.model-routes/1";

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRoute {
    pub id: String,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub enabled: bool,
    /// Exact approved Matrix choice IDs for which this route may be recommended.
    pub allowed_matrix_choice_ids: Vec<String>,
    pub allowed_roles: Vec<String>,
    pub allowed_tools: Vec<String>,
    pub allowed_data_classes: Vec<String>,
    pub required_host_capabilities: Vec<String>,
    /// Minimum budget available to consider this route, in caller-defined units.
    pub minimum_budget_units: u64,
    /// Minimum time available to consider this route; zero means no floor.
    pub minimum_latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteCatalogue {
    pub schema: String,
    pub version: u64,
    /// Configured server-side entries; no provider or model is supplied by Jev.
    pub routes: Vec<ModelRoute>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteWorkContext {
    /// The caller supplies an already approved, exact Matrix disposition.
    pub approved_matrix_selection: MatrixPlanningSelection,
    pub role: String,
    pub tool: String,
    pub data_class: String,
    pub host_capabilities: Vec<String>,
    pub remaining_budget_units: u64,
    pub available_latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct EligibleModelRoutes {
    pub catalogue_version: u64,
    pub catalogue_digest: String,
    pub work_context_digest: String,
    /// Configured IDs also retain an ineligible caller request as an audit fact.
    pub configured_route_ids: Vec<String>,
    pub route_ids: Vec<String>,
}

/// Ordered IDs returned by an optional adviser. Empty IDs mean abstention.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteRanking {
    pub catalogue_digest: String,
    pub work_context_digest: String,
    pub ranked_route_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ObservedModelRoute {
    /// Host evidence may identify a route unknown to the current catalogue.
    pub route_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub evidence_ref: String,
}

/// These three facts are independent. None is an instruction to dispatch.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteRecord {
    pub requested_route_id: Option<String>,
    pub recommended_route_id: Option<String>,
    pub observed_actual: Option<ObservedModelRoute>,
}

impl ModelRouteCatalogue {
    pub fn validate(&self) -> Result<()> {
        if self.schema != MODEL_ROUTE_CATALOGUE_SCHEMA
            || self.version == 0
            || self.routes.len() > 64
        {
            return Err(Error::InvalidArguments);
        }
        let mut ids = BTreeSet::new();
        for route in &self.routes {
            if !valid_id(&route.id)
                || !valid_id(&route.provider)
                || !valid_id(&route.model)
                || !valid_id(&route.effort)
                || !ids.insert(&route.id)
            {
                return Err(Error::InvalidArguments);
            }
            valid_set(&route.allowed_roles, false)?;
            valid_matrix_choice_set(&route.allowed_matrix_choice_ids)?;
            valid_set(&route.allowed_tools, false)?;
            valid_set(&route.allowed_data_classes, false)?;
            valid_set(&route.required_host_capabilities, true)?;
        }
        Ok(())
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let mut hash = Sha256::new();
        part(&mut hash, MODEL_ROUTE_CATALOGUE_SCHEMA);
        number(&mut hash, self.version);
        let mut routes: Vec<_> = self.routes.iter().collect();
        routes.sort_by(|a, b| a.id.cmp(&b.id));
        number(&mut hash, routes.len() as u64);
        for route in routes {
            for value in [&route.id, &route.provider, &route.model, &route.effort] {
                part(&mut hash, value);
            }
            number(&mut hash, u64::from(route.enabled));
            let mut choices = route.allowed_matrix_choice_ids.clone();
            choices.sort();
            number(&mut hash, choices.len() as u64);
            for choice in choices {
                part(&mut hash, &choice);
            }
            for values in [
                &route.allowed_roles,
                &route.allowed_tools,
                &route.allowed_data_classes,
                &route.required_host_capabilities,
            ] {
                let mut sorted = values.clone();
                sorted.sort();
                number(&mut hash, sorted.len() as u64);
                for value in sorted {
                    part(&mut hash, &value);
                }
            }
            number(&mut hash, route.minimum_budget_units);
            number(&mut hash, route.minimum_latency_ms);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    pub fn eligible(&self, work: &ModelRouteWorkContext) -> Result<EligibleModelRoutes> {
        let catalogue_digest = self.digest()?;
        let work_context_digest = work.digest()?;
        let capabilities: BTreeSet<_> = work.host_capabilities.iter().collect();
        let mut configured_route_ids: Vec<_> =
            self.routes.iter().map(|route| route.id.clone()).collect();
        configured_route_ids.sort();
        let mut route_ids: Vec<_> = self
            .routes
            .iter()
            .filter(|route| {
                route.enabled
                    && route
                        .allowed_matrix_choice_ids
                        .contains(&work.approved_matrix_selection.selected_choice_id)
                    && route.allowed_roles.contains(&work.role)
                    && route.allowed_tools.contains(&work.tool)
                    && route.allowed_data_classes.contains(&work.data_class)
                    && route
                        .required_host_capabilities
                        .iter()
                        .all(|capability| capabilities.contains(capability))
                    && work.remaining_budget_units >= route.minimum_budget_units
                    && work.available_latency_ms >= route.minimum_latency_ms
            })
            .map(|route| route.id.clone())
            .collect();
        route_ids.sort();
        Ok(EligibleModelRoutes {
            catalogue_version: self.version,
            catalogue_digest,
            work_context_digest,
            configured_route_ids,
            route_ids,
        })
    }
}

impl ModelRouteWorkContext {
    pub fn digest(&self) -> Result<String> {
        self.approved_matrix_selection.validate()?;
        for id in [&self.role, &self.tool, &self.data_class] {
            if !valid_id(id) {
                return Err(Error::InvalidArguments);
            }
        }
        valid_set(&self.host_capabilities, true)?;
        let mut hash = Sha256::new();
        part(&mut hash, "tect.model-route-work/1");
        let selection = &self.approved_matrix_selection;
        part(&mut hash, &selection.task_id.to_string());
        number(&mut hash, selection.task_revision as u64);
        part(&mut hash, &selection.disposition_id.to_string());
        part(&mut hash, &selection.selected_choice_id);
        for digest in [
            &selection.expected_input_digest,
            &selection.expected_choice_set_digest,
            &selection.expected_verification_digest,
        ] {
            part(&mut hash, digest);
        }
        number(&mut hash, selection.mapped_draft_node_indices.len() as u64);
        for index in &selection.mapped_draft_node_indices {
            number(&mut hash, *index as u64);
        }
        for id in [&self.role, &self.tool, &self.data_class] {
            part(&mut hash, id);
        }
        let mut capabilities = self.host_capabilities.clone();
        capabilities.sort();
        number(&mut hash, capabilities.len() as u64);
        for capability in capabilities {
            part(&mut hash, &capability);
        }
        number(&mut hash, self.remaining_budget_units);
        number(&mut hash, self.available_latency_ms);
        Ok(format!("{:x}", hash.finalize()))
    }
}

impl EligibleModelRoutes {
    /// A provider reply cannot add, duplicate, or refer to stale route IDs.
    /// An empty ranking is a valid abstention, including when no routes qualify.
    pub fn recommendation(&self, ranking: &ModelRouteRanking) -> Result<Option<String>> {
        if self.catalogue_digest != ranking.catalogue_digest
            || self.work_context_digest != ranking.work_context_digest
        {
            return Err(Error::StaleRevision);
        }
        let mut seen = BTreeSet::new();
        for id in &ranking.ranked_route_ids {
            if !self.route_ids.contains(id) || !seen.insert(id) {
                return Err(Error::InvalidArguments);
            }
        }
        Ok(ranking.ranked_route_ids.first().cloned())
    }

    pub fn record(
        &self,
        requested_route_id: Option<String>,
        recommended_route_id: Option<String>,
        observed_actual: Option<ObservedModelRoute>,
    ) -> Result<ModelRouteRecord> {
        if requested_route_id
            .as_ref()
            .is_some_and(|id| !self.configured_route_ids.contains(id))
            || recommended_route_id
                .as_ref()
                .is_some_and(|id| !self.route_ids.contains(id))
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(actual) = &observed_actual {
            if actual.route_id.as_deref().is_some_and(|id| !valid_id(id))
                || !valid_id(&actual.provider)
                || !valid_id(&actual.model)
                || !valid_id(&actual.effort)
                || !valid_ref(&actual.evidence_ref)
            {
                return Err(Error::InvalidArguments);
            }
        }
        Ok(ModelRouteRecord {
            requested_route_id,
            recommended_route_id,
            observed_actual,
        })
    }
}

fn valid_id(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 128
        && value.bytes().all(|byte| {
            byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_' | b'-' | b'/' | b':')
        })
}

fn valid_ref(value: &str) -> bool {
    !value.is_empty() && value.len() <= 512 && value.trim() == value && !value.contains('\0')
}

fn valid_set(values: &[String], allow_empty: bool) -> Result<()> {
    if values.len() > 32 || (!allow_empty && values.is_empty()) {
        return Err(Error::InvalidArguments);
    }
    let mut seen = BTreeSet::new();
    for value in values {
        if !valid_id(value) || !seen.insert(value) {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}

fn valid_matrix_choice_set(values: &[String]) -> Result<()> {
    if values.is_empty() || values.len() > 32 {
        return Err(Error::InvalidArguments);
    }
    let mut seen = BTreeSet::new();
    for value in values {
        if value.is_empty()
            || value.len() > 4096
            || value.trim() != value
            || value.contains('\0')
            || !seen.insert(value)
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}

fn part(hash: &mut Sha256, value: &str) {
    number(hash, value.len() as u64);
    hash.update(value.as_bytes());
}

fn number(hash: &mut Sha256, value: u64) {
    hash.update(value.to_be_bytes());
}

#[cfg(test)]
#[path = "model_routing_tests.rs"]
mod tests;
