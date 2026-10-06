//! Recommendation-only model routing. No provider client or execution operation lives here.
use crate::{Error, MATRIX_REQUIREMENTS_SCHEMA, MatrixPlanningSelection, Result};
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
    /// Server-derived V2 Matrix declaration binding. None is historical V1.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub context_authority: Option<ModelRouteContextAuthority>,
    pub role: ModelRouteFact<String>,
    pub tool: ModelRouteFact<String>,
    pub data_class: ModelRouteFact<String>,
    pub host_capabilities: ModelRouteFact<Vec<String>>,
    pub remaining_budget_units: ModelRouteFact<u64>,
    pub available_latency_ms: ModelRouteFact<u64>,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteContextAuthority {
    pub frozen_snapshot_id: Uuid,
    pub authority_schema: String,
    pub requirements_semantic_digest: String,
    pub operating_verification_digest: String,
}

impl ModelRouteContextAuthority {
    pub fn validate_for(&self, selection: &MatrixPlanningSelection) -> Result<()> {
        if self.frozen_snapshot_id.is_nil()
            || self.authority_schema != MATRIX_REQUIREMENTS_SCHEMA
            || !valid_sha256(&self.requirements_semantic_digest)
            || self.operating_verification_digest != selection.expected_verification_digest
        {
            return Err(Error::StaleContext);
        }
        Ok(())
    }
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
    /// An exact owner-confirmed declaration inherited by this Work revision.
    /// The adapter must resolve the declared path and ancestry; these fields
    /// alone do not confer declaration authority.
    ConfirmedWorkRequirement {
        frozen_snapshot_id: Uuid,
        requirements_semantic_digest: String,
        source_ref: String,
        work_node_id: Uuid,
        work_node_revision: i64,
    },
    /// A trusted operating observation, resolved by its source adapter.
    /// It must be current at each advisory transition.
    OperatingEvidence {
        source_ref: String,
        content_digest: String,
        observed_at_epoch_ms: i64,
        expires_at_epoch_ms: i64,
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

include!("model_routing/catalogue.rs");
impl ModelRouteWorkContext {
    /// New preparations require V2 authority. Historical V1 receipts may still
    /// deserialize for audit, but cannot authorize fresh advice.
    pub fn require_current_authority(&self) -> Result<&ModelRouteContextAuthority> {
        let authority = self.context_authority.as_ref().ok_or(Error::StaleContext)?;
        authority.validate_for(&self.approved_matrix_selection)?;
        let now = std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::StaleContext)?
            .as_millis();
        if !self.operating_facts_current_at(i64::try_from(now).map_err(|_| Error::StaleContext)?) {
            return Err(Error::StaleContext);
        }
        Ok(authority)
    }

    pub fn digest(&self) -> Result<String> {
        self.approved_matrix_selection.validate()?;
        if let Some(authority) = &self.context_authority {
            authority.validate_for(&self.approved_matrix_selection)?;
        }
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
            validate_work_fact(fact, self)?;
            if let ModelRouteFact::Known { value, .. } = fact
                && !valid_id(value)
            {
                return Err(Error::InvalidArguments);
            }
        }
        validate_host_fact(&self.host_capabilities)?;
        if let ModelRouteFact::Known { value, .. } = &self.host_capabilities {
            valid_set(value, true)?;
        }
        validate_operating_fact(&self.remaining_budget_units, self)?;
        validate_operating_fact(&self.available_latency_ms, self)?;
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
        part(
            &mut hash,
            if self.context_authority.is_some() {
                "tect.model-route-work/3"
            } else {
                "tect.model-route-work/2"
            },
        );
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
        if let Some(authority) = &self.context_authority {
            part(&mut hash, &authority.frozen_snapshot_id.to_string());
            part(&mut hash, &authority.authority_schema);
            part(&mut hash, &authority.requirements_semantic_digest);
            part(&mut hash, &authority.operating_verification_digest);
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
        // Caller assertions are usable for advisory matching, with their
        // provenance retained. They do not become observed operating facts.
        !available_work_fact(&self.role)
            || !available_work_fact(&self.tool)
            || !available_work_fact(&self.data_class)
            || matches!(self.host_capabilities, ModelRouteFact::Unknown)
            || !available_numeric_fact(&self.remaining_budget_units)
            || !available_numeric_fact(&self.available_latency_ms)
    }

    pub fn operating_facts_current_at(&self, now_epoch_ms: i64) -> bool {
        [&self.role, &self.tool, &self.data_class]
            .into_iter()
            .filter_map(ModelRouteFact::provenance)
            .chain(self.remaining_budget_units.provenance())
            .chain(self.available_latency_ms.provenance())
            .all(|provenance| match provenance {
                ModelRouteFactProvenance::OperatingEvidence {
                    observed_at_epoch_ms,
                    expires_at_epoch_ms,
                    ..
                } => *observed_at_epoch_ms <= now_epoch_ms && now_epoch_ms < *expires_at_epoch_ms,
                _ => true,
            })
    }
}

#[path = "model_routing/fact_validation.rs"]
mod fact_validation;
use fact_validation::{
    available_numeric_fact, available_work_fact, hash_fact, valid_sha256, validate_host_fact,
    validate_operating_fact, validate_work_fact,
};

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
