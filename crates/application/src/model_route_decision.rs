//! Immutable recommendation-only outcome and explicit agent disposition.
//! Neither operation invokes or schedules a model route.
use crate::{
    CapturedModelRouteDecision, CapturedModelRouteDisposition, ModelRouteAbstainReason,
    ModelRouteDecisionInput, ModelRouteDecisionOutcome, ModelRouteDecisionStore,
    ModelRouteDispositionAction, ModelRoutePreparation, ModelRouteRecommendationStore,
};
use tect_domain::{Error, ModelRouteRankingWireOutcome, ModelRouteWireAbstainReason, Result};
use uuid::Uuid;

pub struct DecideModelRouteRecommendation {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub preparation_request_key: String,
    pub input: ModelRouteDecisionInput,
}

impl DecideModelRouteRecommendation {
    pub async fn decide(
        &self,
        preparations: &mut dyn ModelRouteRecommendationStore,
        decisions: &mut dyn ModelRouteDecisionStore,
    ) -> Result<CapturedModelRouteDecision> {
        if self.id.is_nil() || self.workspace_id.is_nil() || self.preparation_request_key.is_empty()
        {
            return Err(Error::InvalidArguments);
        }
        let prepared = preparations
            .by_request(self.workspace_id, &self.preparation_request_key)
            .await?
            .ok_or(Error::NotFound)?;
        if prepared.workspace_id != self.workspace_id
            || prepared.request_key != self.preparation_request_key
            || prepared.routes.recommended_route_id.is_some()
            || prepared.routes.observed_actual.is_some()
        {
            return Err(Error::InputConflict);
        }
        match (&prepared.catalogue, &prepared.eligible) {
            (Some(catalogue), Some(eligible))
                if catalogue.eligible(&prepared.work)? == *eligible => {}
            (None, None) => {}
            _ => return Err(Error::InputConflict),
        }
        let outcome = if prepared.preparation == ModelRoutePreparation::Prepared {
            let eligible = prepared.eligible.as_ref().ok_or(Error::InternalInvariant)?;
            if eligible.route_ids.is_empty() {
                return Err(Error::InternalInvariant);
            }
            match &self.input {
                ModelRouteDecisionInput::Ranking(ranking) => {
                    let recommended = eligible.recommendation(ranking)?;
                    let evidence = decisions
                        .sealed_provider_ranking(self.workspace_id, &self.preparation_request_key)
                        .await?
                        .ok_or(Error::Forbidden)?;
                    if evidence.verify(&prepared)? != Some(ranking.clone()) {
                        return Err(Error::InputConflict);
                    }
                    match recommended {
                        Some(route_id) => ModelRouteDecisionOutcome::Recommended { route_id },
                        None => ModelRouteDecisionOutcome::Abstained {
                            reason: ModelRouteAbstainReason::EmptyRanking,
                        },
                    }
                }
                ModelRouteDecisionInput::Abstain => {
                    let reason = match decisions
                        .sealed_provider_ranking(self.workspace_id, &self.preparation_request_key)
                        .await?
                    {
                        None => ModelRouteAbstainReason::Explicit,
                        Some(evidence) => {
                            if evidence.verify(&prepared)?.is_some() {
                                return Err(Error::InputConflict);
                            }
                            match evidence.outcome {
                                ModelRouteRankingWireOutcome::Abstained {
                                    reason: ModelRouteWireAbstainReason::NoPreference,
                                } => ModelRouteAbstainReason::ProviderNoPreference,
                                ModelRouteRankingWireOutcome::Abstained {
                                    reason: ModelRouteWireAbstainReason::InsufficientEvidence,
                                } => ModelRouteAbstainReason::ProviderInsufficientEvidence,
                                _ => return Err(Error::InputConflict),
                            }
                        }
                    };
                    ModelRouteDecisionOutcome::Abstained { reason }
                }
                ModelRouteDecisionInput::NoCall => ModelRouteDecisionOutcome::Abstained {
                    reason: ModelRouteAbstainReason::NoCall,
                },
            }
        } else {
            if !matches!(self.input, ModelRouteDecisionInput::NoCall) {
                return Err(Error::InvalidArguments);
            }
            ModelRouteDecisionOutcome::NoRoute {
                reason: prepared.preparation,
            }
        };
        let recommended = match &outcome {
            ModelRouteDecisionOutcome::Recommended { route_id } => Some(route_id.clone()),
            _ => None,
        };
        let routes = match &prepared.eligible {
            Some(eligible) => eligible.record(
                prepared.routes.requested_route_id.clone(),
                recommended,
                None,
            )?,
            None if recommended.is_none() => prepared.routes.clone(),
            None => return Err(Error::InternalInvariant),
        };
        let value = CapturedModelRouteDecision {
            id: self.id,
            prepared,
            input: self.input.clone(),
            outcome,
            routes,
        };
        if let Some(saved) = decisions.decision_by_id(self.workspace_id, self.id).await? {
            return if saved == value {
                Ok(saved)
            } else {
                Err(Error::InputConflict)
            };
        }
        if let Some(saved) = decisions
            .decision_by_preparation(self.workspace_id, &self.preparation_request_key)
            .await?
        {
            return if saved == value {
                Ok(saved)
            } else {
                Err(Error::InputConflict)
            };
        }
        let saved = decisions.capture_decision(&value).await?;
        if saved != value {
            return Err(Error::InternalInvariant);
        }
        Ok(saved)
    }
}

pub struct DispositionModelRouteRecommendation {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub decision_id: Uuid,
    pub actor_id: Uuid,
    pub action: ModelRouteDispositionAction,
    pub rationale: String,
}

impl DispositionModelRouteRecommendation {
    pub async fn record(
        &self,
        decisions: &mut dyn ModelRouteDecisionStore,
    ) -> Result<CapturedModelRouteDisposition> {
        if self.id.is_nil()
            || self.workspace_id.is_nil()
            || self.decision_id.is_nil()
            || self.actor_id.is_nil()
            || self.rationale.trim().is_empty()
            || self.rationale.len() > 4096
            || self.rationale.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        let decision = decisions
            .decision_by_id(self.workspace_id, self.decision_id)
            .await?
            .ok_or(Error::NotFound)?;
        if decision.prepared.workspace_id != self.workspace_id
            || !matches!(
                decision.outcome,
                ModelRouteDecisionOutcome::Recommended { .. }
            )
        {
            return Err(Error::InputConflict);
        }
        let value = CapturedModelRouteDisposition {
            id: self.id,
            decision_id: self.decision_id,
            workspace_id: self.workspace_id,
            actor_id: self.actor_id,
            action: self.action,
            rationale: self.rationale.clone(),
        };
        if let Some(saved) = decisions
            .disposition_by_id(self.workspace_id, self.id)
            .await?
        {
            return if saved == value {
                Ok(saved)
            } else {
                Err(Error::InputConflict)
            };
        }
        if let Some(saved) = decisions
            .disposition_by_decision(self.workspace_id, self.decision_id)
            .await?
        {
            return if saved == value {
                Ok(saved)
            } else {
                Err(Error::InputConflict)
            };
        }
        let saved = decisions.capture_disposition(&value).await?;
        if saved != value {
            return Err(Error::InternalInvariant);
        }
        Ok(saved)
    }
}

#[cfg(test)]
mod tests;
