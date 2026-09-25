//! Recommendation-only model routing. No provider client or execution operation lives here.
use crate::{Error, MatrixPlanningSelection, Result};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MODEL_ROUTE_CATALOGUE_SCHEMA: &str = "tect.model-routes/1";
pub const MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA: &str = "tect.model-route-host-capabilities/1";

/// A separate host assertion about capabilities available to this daemon.
/// It is never inferred from a route catalogue or caller-authored Work prose.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ModelRouteHostCapabilities {
    pub schema: String,
    pub version: u64,
    pub capabilities: Vec<String>,
}

impl ModelRouteHostCapabilities {
    pub fn validate(&self) -> Result<()> {
        if self.schema != MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA || self.version == 0 {
            return Err(Error::InvalidArguments);
        }
        valid_set(&self.capabilities, true)
    }

    pub fn digest(&self) -> Result<String> {
        self.validate()?;
        let mut hash = Sha256::new();
        part(&mut hash, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA);
        number(&mut hash, self.version);
        let mut values = self.capabilities.clone();
        values.sort();
        number(&mut hash, values.len() as u64);
        for value in values {
            part(&mut hash, &value);
        }
        Ok(format!("{:x}", hash.finalize()))
    }

    pub fn fact(&self) -> Result<ModelRouteFact<Vec<String>>> {
        let digest = self.digest()?;
        Ok(ModelRouteFact::Known {
            value: self.capabilities.clone(),
            provenance: ModelRouteFactProvenance::Host {
                evidence_ref: format!(
                    "{MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA}:v{}:{digest}",
                    self.version
                ),
            },
        })
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
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

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteCatalogue {
    pub schema: String,
    pub version: u64,
    /// Configured server-side entries; no provider or model is supplied by Jev.
    pub routes: Vec<ModelRoute>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRouteWorkContext {
    /// The store supplies the approved Matrix disposition from persisted state.
    pub approved_matrix_selection: MatrixPlanningSelection,
    /// Persisted native save receipt and one mapped Work node, never inferred from prose.
    pub selection_link: ModelRouteSelectionLink,
    pub role: ModelRouteFact<String>,
    pub tool: ModelRouteFact<String>,
    pub data_class: ModelRouteFact<String>,
    pub host_capabilities: ModelRouteFact<Vec<String>>,
    pub remaining_budget_units: ModelRouteFact<u64>,
    pub available_latency_ms: ModelRouteFact<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRouteSelectionLink {
    pub candidate_set_id: Uuid,
    pub caller_request_id: Uuid,
    pub mapped_draft_node_index: usize,
    pub mapped_work_node_id: Uuid,
    pub mapped_work_node_revision: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRouteFact<T> {
    Known {
        value: T,
        provenance: ModelRouteFactProvenance,
    },
    Unknown,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub enum ModelRouteFactProvenance {
    /// Explicit caller-authored fact attached to the exact saved Work node.
    Caller {
        source_ref: String,
        work_node_id: Uuid,
        work_node_revision: i64,
    },
    /// Host-owned capability discovery; caller assertions are insufficient.
    Host { evidence_ref: String },
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct EligibleModelRoutes {
    pub catalogue_version: u64,
    pub catalogue_digest: String,
    pub work_context_digest: String,
    /// Configured IDs also retain an ineligible caller request as an audit fact.
    pub configured_route_ids: Vec<String>,
    pub route_ids: Vec<String>,
}

/// Ordered IDs returned by an optional adviser. Empty IDs mean abstention.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ModelRouteRanking {
    pub catalogue_digest: String,
    pub work_context_digest: String,
    pub ranked_route_ids: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
pub struct ObservedModelRoute {
    /// Host evidence may identify a route unknown to the current catalogue.
    pub route_id: Option<String>,
    pub provider: String,
    pub model: String,
    pub effort: String,
    pub evidence_ref: String,
}

/// These three facts are independent. None is an instruction to dispatch.
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
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
        let (role, tool, data_class, capabilities, budget, latency) = match (
            &work.role,
            &work.tool,
            &work.data_class,
            &work.host_capabilities,
            &work.remaining_budget_units,
            &work.available_latency_ms,
        ) {
            (
                ModelRouteFact::Known { value: role, .. },
                ModelRouteFact::Known { value: tool, .. },
                ModelRouteFact::Known {
                    value: data_class, ..
                },
                ModelRouteFact::Known {
                    value: capabilities,
                    ..
                },
                ModelRouteFact::Known { value: budget, .. },
                ModelRouteFact::Known { value: latency, .. },
            ) => (role, tool, data_class, capabilities, budget, latency),
            _ => {
                let mut configured_route_ids: Vec<_> =
                    self.routes.iter().map(|route| route.id.clone()).collect();
                configured_route_ids.sort();
                return Ok(EligibleModelRoutes {
                    catalogue_version: self.version,
                    catalogue_digest,
                    work_context_digest,
                    configured_route_ids,
                    route_ids: Vec::new(),
                });
            }
        };
        let capabilities: BTreeSet<_> = capabilities.iter().collect();
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
                    && route.allowed_roles.contains(role)
                    && route.allowed_tools.contains(tool)
                    && route.allowed_data_classes.contains(data_class)
                    && route
                        .required_host_capabilities
                        .iter()
                        .all(|capability| capabilities.contains(capability))
                    && *budget >= route.minimum_budget_units
                    && *latency >= route.minimum_latency_ms
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
        let link = &self.selection_link;
        if link.candidate_set_id.is_nil()
            || link.caller_request_id.is_nil()
            || link.mapped_work_node_id.is_nil()
            || link.mapped_work_node_revision < 1
            || !self
                .approved_matrix_selection
                .mapped_draft_node_indices
                .contains(&link.mapped_draft_node_index)
        {
            return Err(Error::InvalidArguments);
        }
        for fact in [&self.role, &self.tool, &self.data_class] {
            validate_fact(fact, false)?;
            if let ModelRouteFact::Known { value, .. } = fact
                && !valid_id(value)
            {
                return Err(Error::InvalidArguments);
            }
        }
        validate_fact(&self.host_capabilities, true)?;
        if let ModelRouteFact::Known { value, .. } = &self.host_capabilities {
            valid_set(value, true)?;
        }
        validate_fact(&self.remaining_budget_units, false)?;
        validate_fact(&self.available_latency_ms, false)?;
        for provenance in [
            self.role.provenance(),
            self.tool.provenance(),
            self.data_class.provenance(),
            self.remaining_budget_units.provenance(),
            self.available_latency_ms.provenance(),
        ] {
            if let Some(ModelRouteFactProvenance::Caller {
                work_node_id,
                work_node_revision,
                ..
            }) = provenance
                && (*work_node_id != link.mapped_work_node_id
                    || *work_node_revision != link.mapped_work_node_revision)
            {
                return Err(Error::InvalidArguments);
            }
        }
        let mut hash = Sha256::new();
        part(&mut hash, "tect.model-route-work/2");
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
        part(&mut hash, &link.candidate_set_id.to_string());
        part(&mut hash, &link.caller_request_id.to_string());
        number(&mut hash, link.mapped_draft_node_index as u64);
        part(&mut hash, &link.mapped_work_node_id.to_string());
        number(&mut hash, link.mapped_work_node_revision as u64);
        for fact in [&self.role, &self.tool, &self.data_class] {
            hash_fact(&mut hash, fact, |hash, value| part(hash, value));
        }
        hash_fact(&mut hash, &self.host_capabilities, |hash, values| {
            let mut sorted = values.clone();
            sorted.sort();
            number(hash, sorted.len() as u64);
            for value in sorted {
                part(hash, &value);
            }
        });
        hash_fact(&mut hash, &self.remaining_budget_units, |hash, value| {
            number(hash, *value)
        });
        hash_fact(&mut hash, &self.available_latency_ms, |hash, value| {
            number(hash, *value)
        });
        Ok(format!("{:x}", hash.finalize()))
    }

    pub fn has_unknown_facts(&self) -> bool {
        matches!(self.role, ModelRouteFact::Unknown)
            || matches!(self.tool, ModelRouteFact::Unknown)
            || matches!(self.data_class, ModelRouteFact::Unknown)
            || matches!(self.host_capabilities, ModelRouteFact::Unknown)
            || matches!(self.remaining_budget_units, ModelRouteFact::Unknown)
            || matches!(self.available_latency_ms, ModelRouteFact::Unknown)
    }
}

impl<T> ModelRouteFact<T> {
    fn provenance(&self) -> Option<&ModelRouteFactProvenance> {
        match self {
            Self::Known { provenance, .. } => Some(provenance),
            Self::Unknown => None,
        }
    }
}

fn validate_fact<T>(fact: &ModelRouteFact<T>, host_owned: bool) -> Result<()> {
    match fact {
        ModelRouteFact::Unknown => Ok(()),
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Host { evidence_ref },
            ..
        } if host_owned && valid_ref(evidence_ref) => Ok(()),
        ModelRouteFact::Known {
            provenance:
                ModelRouteFactProvenance::Caller {
                    source_ref,
                    work_node_id,
                    work_node_revision,
                },
            ..
        } if !host_owned
            && valid_ref(source_ref)
            && !work_node_id.is_nil()
            && *work_node_revision >= 1 =>
        {
            Ok(())
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn hash_fact<T>(
    hash: &mut Sha256,
    fact: &ModelRouteFact<T>,
    value_hash: impl FnOnce(&mut Sha256, &T),
) {
    match fact {
        ModelRouteFact::Unknown => part(hash, "unknown"),
        ModelRouteFact::Known { value, provenance } => {
            part(hash, "known");
            match provenance {
                ModelRouteFactProvenance::Caller {
                    source_ref,
                    work_node_id,
                    work_node_revision,
                } => {
                    part(hash, "caller");
                    part(hash, source_ref);
                    part(hash, &work_node_id.to_string());
                    number(hash, *work_node_revision as u64);
                }
                ModelRouteFactProvenance::Host { evidence_ref } => {
                    part(hash, "host");
                    part(hash, evidence_ref);
                }
            }
            value_hash(hash, value);
        }
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
        if let Some(actual) = &observed_actual
            && (actual.route_id.as_deref().is_some_and(|id| !valid_id(id))
                || !valid_id(&actual.provider)
                || !valid_id(&actual.model)
                || !valid_id(&actual.effort)
                || !valid_ref(&actual.evidence_ref))
        {
            return Err(Error::InvalidArguments);
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
