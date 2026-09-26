//! Finite, exact Slice 05 adviser wire. This is advice, never model execution.
use crate::{
    EligibleModelRoutes, Error, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA, ModelRoute,
    ModelRouteCatalogue, ModelRouteFact, ModelRouteFactProvenance, ModelRouteHostCapabilities,
    ModelRouteRanking, ModelRouteWorkContext, Result,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const MODEL_ROUTE_RANKING_WIRE_SCHEMA: &str = "tect.model-route-ranking/1";
pub const MAX_MODEL_ROUTE_RANKING_WIRE_BYTES: usize = 256 * 1024;

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteRankingBinding {
    pub workspace_id: Uuid,
    pub preparation_request_key: String,
    pub work_context_digest: String,
    pub catalogue_digest: String,
    pub host_evidence_ref: String,
    pub eligible_route_ids: Vec<String>,
    pub adviser_model: String,
}

impl ModelRouteRankingBinding {
    pub fn digest(&self) -> Result<String> {
        if self.workspace_id.is_nil()
            || self.preparation_request_key.is_empty()
            || self.preparation_request_key.len() > 256
            || self.preparation_request_key.contains('\0')
            || !hex_digest(&self.work_context_digest)
            || !hex_digest(&self.catalogue_digest)
            || host_ref_version(&self.host_evidence_ref).is_none()
            || self.adviser_model.is_empty()
            || self.adviser_model.len() > 128
            || self.adviser_model.chars().any(char::is_control)
            || self.eligible_route_ids.is_empty()
            || self.eligible_route_ids.len() > 64
            || self.eligible_route_ids.iter().any(|id| id.is_empty())
            || self
                .eligible_route_ids
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::InvalidArguments);
        }
        Ok(sha(
            &serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)?
        ))
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
pub struct ModelRouteRankingWireRequest {
    pub schema: String,
    pub binding: ModelRouteRankingBinding,
    pub binding_digest: String,
    pub work: ModelRouteWorkContext,
    pub eligible_routes: Vec<ModelRoute>,
}

impl ModelRouteRankingWireRequest {
    pub fn new(
        workspace_id: Uuid,
        preparation_request_key: &str,
        work: &ModelRouteWorkContext,
        catalogue: &ModelRouteCatalogue,
        eligible: &EligibleModelRoutes,
        adviser_model: &str,
    ) -> Result<Self> {
        if catalogue.eligible(work)? != *eligible || eligible.route_ids.is_empty() {
            return Err(Error::InputConflict);
        }
        let ModelRouteFact::Known {
            value: capabilities,
            provenance: ModelRouteFactProvenance::Host { evidence_ref },
            ..
        } = &work.host_capabilities
        else {
            return Err(Error::InputConflict);
        };
        let version = host_ref_version(evidence_ref).ok_or(Error::InputConflict)?;
        let snapshot = ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version,
            capabilities: capabilities.clone(),
        };
        if !evidence_ref.ends_with(&snapshot.digest()?) {
            return Err(Error::InputConflict);
        }
        let routes = eligible
            .route_ids
            .iter()
            .map(|id| {
                catalogue
                    .routes
                    .iter()
                    .find(|route| &route.id == id)
                    .cloned()
            })
            .collect::<Option<Vec<_>>>()
            .ok_or(Error::InputConflict)?;
        let binding = ModelRouteRankingBinding {
            workspace_id,
            preparation_request_key: preparation_request_key.into(),
            work_context_digest: work.digest()?,
            catalogue_digest: catalogue.digest()?,
            host_evidence_ref: evidence_ref.clone(),
            eligible_route_ids: eligible.route_ids.clone(),
            adviser_model: adviser_model.into(),
        };
        let binding_digest = binding.digest()?;
        let request = Self {
            schema: MODEL_ROUTE_RANKING_WIRE_SCHEMA.into(),
            binding,
            binding_digest,
            work: work.clone(),
            eligible_routes: routes,
        };
        request.validate()?;
        Ok(request)
    }

    pub fn validate(&self) -> Result<()> {
        let ModelRouteFact::Known {
            value: capabilities,
            provenance: ModelRouteFactProvenance::Host { evidence_ref },
        } = &self.work.host_capabilities
        else {
            return Err(Error::InputConflict);
        };
        let version = host_ref_version(evidence_ref).ok_or(Error::InputConflict)?;
        let snapshot = ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version,
            capabilities: capabilities.clone(),
        };
        if self.schema != MODEL_ROUTE_RANKING_WIRE_SCHEMA
            || self.binding.digest()? != self.binding_digest
            || self.work.digest()? != self.binding.work_context_digest
            || evidence_ref != &self.binding.host_evidence_ref
            || !evidence_ref.ends_with(&snapshot.digest()?)
            || self.eligible_routes.len() != self.binding.eligible_route_ids.len()
            || self
                .eligible_routes
                .iter()
                .zip(&self.binding.eligible_route_ids)
                .any(|(route, id)| &route.id != id || !route.enabled)
        {
            return Err(Error::InputConflict);
        }
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)?;
        if bytes.len() > MAX_MODEL_ROUTE_RANKING_WIRE_BYTES {
            return Err(Error::RequestTooLarge);
        }
        Ok(())
    }

    pub fn bytes(&self) -> Result<Vec<u8>> {
        self.validate()?;
        serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)
    }
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum ModelRouteRankingWireOutcome {
    Ranked { route_ids: Vec<String> },
    Abstained { reason: ModelRouteWireAbstainReason },
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum ModelRouteWireAbstainReason {
    NoPreference,
    InsufficientEvidence,
}

#[derive(Debug, Clone, Serialize, Deserialize, PartialEq, Eq)]
#[serde(deny_unknown_fields)]
struct WireResponse {
    schema: String,
    binding_digest: String,
    adviser_model: String,
    outcome: ModelRouteRankingWireOutcome,
}

pub fn parse_model_route_ranking_response(
    request: &ModelRouteRankingWireRequest,
    raw: &[u8],
) -> Result<ModelRouteRankingWireOutcome> {
    request.validate()?;
    if raw.is_empty() || raw.len() > MAX_MODEL_ROUTE_RANKING_WIRE_BYTES {
        return Err(Error::InvalidArguments);
    }
    let response: WireResponse =
        serde_json::from_slice(raw).map_err(|_| Error::InvalidArguments)?;
    if response.schema != MODEL_ROUTE_RANKING_WIRE_SCHEMA
        || response.binding_digest != request.binding_digest
        || response.adviser_model != request.binding.adviser_model
    {
        return Err(Error::InputConflict);
    }
    validate_model_route_ranking_outcome(request, &response.outcome)?;
    Ok(response.outcome)
}

/// Validate a trusted native codec's typed result without rewriting its raw seal.
pub fn validate_model_route_ranking_outcome(
    request: &ModelRouteRankingWireRequest,
    outcome: &ModelRouteRankingWireOutcome,
) -> Result<()> {
    request.validate()?;
    if let ModelRouteRankingWireOutcome::Ranked { route_ids } = outcome {
        let expected = &request.binding.eligible_route_ids;
        let actual: BTreeSet<_> = route_ids.iter().collect();
        if route_ids.len() != expected.len()
            || actual.len() != expected.len()
            || !expected.iter().all(|id| actual.contains(id))
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}

pub fn model_route_ranking_from_wire(
    request: &ModelRouteRankingWireRequest,
    outcome: &ModelRouteRankingWireOutcome,
) -> Option<ModelRouteRanking> {
    match outcome {
        ModelRouteRankingWireOutcome::Ranked { route_ids } => Some(ModelRouteRanking {
            catalogue_digest: request.binding.catalogue_digest.clone(),
            work_context_digest: request.binding.work_context_digest.clone(),
            ranked_route_ids: route_ids.clone(),
        }),
        ModelRouteRankingWireOutcome::Abstained { .. } => None,
    }
}

pub fn model_route_wire_sha256(bytes: &[u8]) -> String {
    sha(bytes)
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

fn host_ref_version(value: &str) -> Option<u64> {
    let prefix = format!("{MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA}:v");
    let (version, digest) = value.strip_prefix(&prefix)?.split_once(':')?;
    hex_digest(digest)
        .then(|| version.parse::<u64>().ok())
        .flatten()
        .filter(|v| *v > 0)
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

#[cfg(test)]
mod tests;
