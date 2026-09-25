//! Immutable Slice 05 recommendation receipts. No execution path is present.
use async_trait::async_trait;
use serde::{Serialize, de::DeserializeOwned};
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    CapturedModelRouteDecision, CapturedModelRouteDisposition, ModelRouteAbstainReason,
    ModelRouteDecisionInput, ModelRouteDecisionOutcome, ModelRouteDecisionStore,
    ModelRouteDispositionAction, ModelRoutePreparation, ModelRouteRecommendationBasis,
    ModelRouteRecommendationStore, ModelRouteSelectionRead, PreparedModelRouteRecommendation,
};
use tect_domain::{
    AdvisoryRequestPreference, Error, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA, ModelRouteFact,
    ModelRouteFactProvenance, ModelRouteHostCapabilities, Result, WorkspaceAdvisoryMode,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

fn encode<T: Serialize>(value: &T) -> Result<Value> {
    serde_json::to_value(value).map_err(storage_error)
}

fn decode<T: DeserializeOwned>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|error| error.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" || code == "23514" || code == "23503" => Error::InputConflict,
        _ => storage_error(error),
    }
}

async fn config(
    uow: &mut PgUnitOfWork,
    workspace_id: Uuid,
) -> Result<ModelRouteRecommendationBasis> {
    let tenant = uow.tenant_id()?;
    let row = sqlx::query(
        "SELECT mode,revision FROM advisory_workspace_config \
         WHERE tenant_id=$1 AND workspace_id=$2 FOR SHARE",
    )
    .bind(tenant)
    .bind(workspace_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(ModelRouteRecommendationBasis {
            advisory_mode: WorkspaceAdvisoryMode::Disabled,
            advisory_config_revision: 0,
        });
    };
    let mode: String = row.try_get("mode").map_err(storage_error)?;
    let advisory_mode = match mode.as_str() {
        "disabled" => WorkspaceAdvisoryMode::Disabled,
        "optional" => WorkspaceAdvisoryMode::Optional,
        _ => return Err(Error::StorageUnavailable),
    };
    Ok(ModelRouteRecommendationBasis {
        advisory_mode,
        advisory_config_revision: row.try_get("revision").map_err(storage_error)?,
    })
}

fn validate_host_fact(fact: &ModelRouteFact<Vec<String>>) -> Result<Option<String>> {
    match fact {
        ModelRouteFact::Unknown => Ok(None),
        ModelRouteFact::Known { value, provenance } => {
            let ModelRouteFactProvenance::Host { evidence_ref } = provenance else {
                return Err(Error::InputConflict);
            };
            let prefix = format!("{MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA}:v");
            let rest = evidence_ref
                .strip_prefix(&prefix)
                .ok_or(Error::InputConflict)?;
            let (version, digest) = rest.split_once(':').ok_or(Error::InputConflict)?;
            let version = version.parse::<u64>().map_err(|_| Error::InputConflict)?;
            let snapshot = ModelRouteHostCapabilities {
                schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
                version,
                capabilities: value.clone(),
            };
            if snapshot.digest().map_err(|_| Error::InputConflict)? != digest {
                return Err(Error::InputConflict);
            }
            Ok(Some(evidence_ref.clone()))
        }
    }
}

fn expected_preparation(
    prepared: &PreparedModelRouteRecommendation,
    mode: WorkspaceAdvisoryMode,
) -> Result<ModelRoutePreparation> {
    let eligible = prepared.eligible.as_ref();
    Ok(if mode == WorkspaceAdvisoryMode::Disabled {
        ModelRoutePreparation::WorkspaceDisabled
    } else if prepared.session_preference == AdvisoryRequestPreference::Skip {
        ModelRoutePreparation::SessionSkip
    } else if prepared.request_preference == AdvisoryRequestPreference::Skip {
        ModelRoutePreparation::RequestSkip
    } else if eligible.is_none() {
        ModelRoutePreparation::CapabilityUnavailable
    } else if prepared.work.has_unknown_facts() {
        ModelRoutePreparation::UnknownWorkFacts
    } else if eligible.is_some_and(|eligible| eligible.route_ids.is_empty()) {
        ModelRoutePreparation::NoEligibleRoutes
    } else {
        ModelRoutePreparation::Prepared
    })
}

pub(crate) async fn current_preparation(
    uow: &mut PgUnitOfWork,
    prepared: &PreparedModelRouteRecommendation,
) -> Result<(String, Option<String>)> {
    let work = &prepared.work;
    let link = &work.selection_link;
    let selection = &work.approved_matrix_selection;
    let tenant = uow.tenant_id()?;
    // Lock the mutable candidate set before reading its current saved revision.
    let locked: Option<Uuid> = sqlx::query_scalar(
        "SELECT id FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR SHARE",
    )
    .bind(tenant)
    .bind(prepared.workspace_id)
    .bind(link.candidate_set_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if locked.is_none() {
        return Err(Error::StaleContext);
    }
    let basis = config(uow, prepared.workspace_id).await?;
    if basis.advisory_config_revision != prepared.advisory_config_revision {
        return Err(Error::StaleRevision);
    }
    let mut fresh = uow
        .approved_work_context(
            prepared.workspace_id,
            selection.disposition_id,
            link.candidate_set_id,
            link.caller_request_id,
            link.mapped_work_node_id,
            link.mapped_work_node_revision,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    let host_ref = validate_host_fact(&work.host_capabilities)?;
    fresh.host_capabilities = work.host_capabilities.clone();
    if fresh != *work {
        return Err(Error::StaleContext);
    }
    let catalogue_digest = match &prepared.catalogue {
        Some(catalogue) => {
            let eligible = catalogue.eligible(work).map_err(|_| Error::InputConflict)?;
            if prepared.eligible.as_ref() != Some(&eligible) {
                return Err(Error::InputConflict);
            }
            Some(catalogue.digest().map_err(|_| Error::InputConflict)?)
        }
        None if prepared.eligible.is_none() => None,
        None => return Err(Error::InputConflict),
    };
    if prepared.preparation != expected_preparation(prepared, basis.advisory_mode)?
        || prepared.routes.recommended_route_id.is_some()
        || prepared.routes.observed_actual.is_some()
    {
        return Err(Error::InputConflict);
    }
    let expected_routes = match &prepared.eligible {
        Some(eligible) => {
            eligible.record(prepared.routes.requested_route_id.clone(), None, None)?
        }
        None if prepared.routes.requested_route_id.is_none() => prepared.routes.clone(),
        None => return Err(Error::InputConflict),
    };
    if expected_routes != prepared.routes {
        return Err(Error::InputConflict);
    }
    Ok((catalogue_digest.unwrap_or_default(), host_ref))
}

#[async_trait]
impl ModelRouteRecommendationStore for PgUnitOfWork {
    async fn validate_current(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<()> {
        let stored = self
            .by_request(prepared.workspace_id, &prepared.request_key)
            .await?
            .ok_or(Error::StaleContext)?;
        if stored != *prepared {
            return Err(Error::InputConflict);
        }
        current_preparation(self, prepared).await.map(|_| ())
    }
    async fn by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedModelRouteRecommendation>> {
        let tenant = self.tenant_id()?;
        let row: Option<Value> = sqlx::query_scalar(
            "SELECT prepared_payload FROM model_route_preparations \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_key=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request_key)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode).transpose()
    }

    async fn load_basis(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
    ) -> Result<Option<ModelRouteRecommendationBasis>> {
        let tenant = self.tenant_id()?;
        let linked: Option<Uuid> = sqlx::query_scalar(
            "SELECT disposition_id FROM matrix_planning_selection_links \
             WHERE tenant_id=$1 AND workspace_id=$2 AND disposition_id=$3 LIMIT 1",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(disposition_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if linked.is_none() {
            return Ok(None);
        }
        config(self, workspace_id).await.map(Some)
    }

    async fn capture(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<PreparedModelRouteRecommendation> {
        if !self.is_read_write() || prepared.workspace_id.is_nil() {
            return Err(Error::Forbidden);
        }
        if let Some(saved) = self
            .by_request(prepared.workspace_id, &prepared.request_key)
            .await?
        {
            return if saved == *prepared {
                Ok(saved)
            } else {
                Err(Error::InputConflict)
            };
        }
        let (catalogue_digest, host_ref) = current_preparation(self, prepared).await?;
        let work = &prepared.work;
        let selection = &work.approved_matrix_selection;
        let link = &work.selection_link;
        let mode = config(self, prepared.workspace_id).await?;
        let tenant = self.tenant_id()?;
        sqlx::query(
            "INSERT INTO model_route_preparations \
             (tenant_id,workspace_id,request_key,disposition_id,candidate_set_id,caller_request_id, \
              work_node_id,work_node_revision,task_id,task_revision,advisory_mode, \
              advisory_config_revision,work_digest,catalogue_digest,host_capability_evidence_ref,prepared_payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(tenant)
        .bind(prepared.workspace_id)
        .bind(&prepared.request_key)
        .bind(selection.disposition_id)
        .bind(link.candidate_set_id)
        .bind(link.caller_request_id)
        .bind(link.mapped_work_node_id)
        .bind(link.mapped_work_node_revision)
        .bind(selection.task_id)
        .bind(selection.task_revision)
        .bind(mode.advisory_mode.as_str())
        .bind(mode.advisory_config_revision)
        .bind(work.digest()?)
        .bind(if catalogue_digest.is_empty() { None } else { Some(catalogue_digest) })
        .bind(host_ref)
        .bind(encode(prepared)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        Ok(prepared.clone())
    }
}

#[async_trait]
impl ModelRouteDecisionStore for PgUnitOfWork {
    async fn sealed_provider_ranking(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<tect_application::ModelRouteSealedRankingEvidence>> {
        crate::model_route_attempt_store::sealed_ranking(self, workspace_id, request_key).await
    }
    async fn decision_by_id(
        &mut self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CapturedModelRouteDecision>> {
        let tenant = self.tenant_id()?;
        let value: Option<Value> = sqlx::query_scalar(
            "SELECT decision_payload FROM model_route_decisions WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        ).bind(tenant).bind(workspace_id).bind(id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        value.map(decode).transpose()
    }

    async fn decision_by_preparation(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<CapturedModelRouteDecision>> {
        let tenant = self.tenant_id()?;
        let value: Option<Value> = sqlx::query_scalar(
            "SELECT decision_payload FROM model_route_decisions WHERE tenant_id=$1 AND workspace_id=$2 AND preparation_request_key=$3",
        ).bind(tenant).bind(workspace_id).bind(request_key)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        value.map(decode).transpose()
    }

    async fn capture_decision(
        &mut self,
        value: &CapturedModelRouteDecision,
    ) -> Result<CapturedModelRouteDecision> {
        if !self.is_read_write()
            || value.id.is_nil()
            || value.routes.observed_actual.is_some()
            || value.prepared.routes.observed_actual.is_some()
        {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        // The preparation row is immutable and runtime has SELECT/INSERT only.
        // current_preparation locks the mutable saved candidate-set head below.
        let stored: Option<Value> = sqlx::query_scalar(
            "SELECT prepared_payload FROM model_route_preparations \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_key=$3",
        )
        .bind(tenant)
        .bind(value.prepared.workspace_id)
        .bind(&value.prepared.request_key)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let stored: PreparedModelRouteRecommendation = decode(stored.ok_or(Error::StaleContext)?)?;
        if stored != value.prepared {
            return Err(Error::InputConflict);
        }
        current_preparation(self, &stored).await?;
        let audit_state = crate::model_route_attempt_store::audit_state(
            self,
            stored.workspace_id,
            &stored.request_key,
        )
        .await?;
        match &value.input {
            ModelRouteDecisionInput::NoCall if audit_state.as_deref() != Some("no_call") => {
                return Err(Error::Forbidden);
            }
            ModelRouteDecisionInput::Abstain
                if matches!(audit_state.as_deref(), Some("send_unknown" | "raw_sealed")) =>
            {
                return Err(Error::InputConflict);
            }
            ModelRouteDecisionInput::Ranking(_) if audit_state.as_deref() != Some("parsed") => {
                return Err(Error::Forbidden);
            }
            _ => {}
        }
        let expected_outcome = match (&stored.preparation, &value.input) {
            (ModelRoutePreparation::Prepared, ModelRouteDecisionInput::Ranking(ranking)) => {
                let proof = self
                    .sealed_provider_ranking(stored.workspace_id, &stored.request_key)
                    .await?
                    .ok_or(Error::Forbidden)?;
                if proof.verify(&stored)? != Some(ranking.clone()) {
                    return Err(Error::InputConflict);
                }
                let eligible = stored.eligible.as_ref().ok_or(Error::InputConflict)?;
                match eligible.recommendation(ranking)? {
                    Some(route_id) => ModelRouteDecisionOutcome::Recommended { route_id },
                    None => ModelRouteDecisionOutcome::Abstained {
                        reason: ModelRouteAbstainReason::EmptyRanking,
                    },
                }
            }
            (ModelRoutePreparation::Prepared, ModelRouteDecisionInput::Abstain) => {
                let reason = match self
                    .sealed_provider_ranking(stored.workspace_id, &stored.request_key)
                    .await?
                {
                    None => ModelRouteAbstainReason::Explicit,
                    Some(proof) => {
                        if proof.verify(&stored)?.is_some() {
                            return Err(Error::InputConflict);
                        }
                        match proof.outcome {
                            tect_domain::ModelRouteRankingWireOutcome::Abstained {
                                reason: tect_domain::ModelRouteWireAbstainReason::NoPreference,
                            } => ModelRouteAbstainReason::ProviderNoPreference,
                            tect_domain::ModelRouteRankingWireOutcome::Abstained {
                                reason:
                                    tect_domain::ModelRouteWireAbstainReason::InsufficientEvidence,
                            } => ModelRouteAbstainReason::ProviderInsufficientEvidence,
                            _ => return Err(Error::InputConflict),
                        }
                    }
                };
                ModelRouteDecisionOutcome::Abstained { reason }
            }
            (ModelRoutePreparation::Prepared, ModelRouteDecisionInput::NoCall) => {
                ModelRouteDecisionOutcome::Abstained {
                    reason: ModelRouteAbstainReason::NoCall,
                }
            }
            (reason, ModelRouteDecisionInput::NoCall) => {
                ModelRouteDecisionOutcome::NoRoute { reason: *reason }
            }
            _ => return Err(Error::InputConflict),
        };
        if expected_outcome != value.outcome {
            return Err(Error::InputConflict);
        }
        let recommended = match &expected_outcome {
            ModelRouteDecisionOutcome::Recommended { route_id } => Some(route_id.clone()),
            _ => None,
        };
        let expected_routes = match &stored.eligible {
            Some(eligible) => eligible.record(
                stored.routes.requested_route_id.clone(),
                recommended.clone(),
                None,
            )?,
            None if recommended.is_none() => stored.routes.clone(),
            None => return Err(Error::InputConflict),
        };
        if expected_routes != value.routes {
            return Err(Error::InputConflict);
        }
        let (kind, reason) = match &expected_outcome {
            ModelRouteDecisionOutcome::Recommended { .. } => ("recommended", None),
            ModelRouteDecisionOutcome::Abstained { reason } => {
                ("abstained", Some(format!("{reason:?}")))
            }
            ModelRouteDecisionOutcome::NoRoute { reason } => {
                ("no_route", Some(format!("{reason:?}")))
            }
        };
        sqlx::query(
            "INSERT INTO model_route_decisions \
             (tenant_id,workspace_id,id,preparation_request_key,outcome_kind,requested_route_id, \
              recommended_route_id,reason,ranking,decision_payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(tenant)
        .bind(stored.workspace_id)
        .bind(value.id)
        .bind(&stored.request_key)
        .bind(kind)
        .bind(&value.routes.requested_route_id)
        .bind(recommended)
        .bind(reason)
        .bind(match &value.input {
            ModelRouteDecisionInput::Ranking(ranking) => Some(encode(ranking)?),
            _ => None,
        })
        .bind(encode(value)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        Ok(value.clone())
    }

    async fn disposition_by_id(
        &mut self,
        workspace_id: Uuid,
        id: Uuid,
    ) -> Result<Option<CapturedModelRouteDisposition>> {
        let tenant = self.tenant_id()?;
        let value: Option<Value> = sqlx::query_scalar(
            "SELECT disposition_payload FROM model_route_dispositions WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        ).bind(tenant).bind(workspace_id).bind(id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        value.map(decode).transpose()
    }

    async fn disposition_by_decision(
        &mut self,
        workspace_id: Uuid,
        decision_id: Uuid,
    ) -> Result<Option<CapturedModelRouteDisposition>> {
        let tenant = self.tenant_id()?;
        let value: Option<Value> = sqlx::query_scalar(
            "SELECT disposition_payload FROM model_route_dispositions WHERE tenant_id=$1 AND workspace_id=$2 AND decision_id=$3",
        ).bind(tenant).bind(workspace_id).bind(decision_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        value.map(decode).transpose()
    }

    async fn capture_disposition(
        &mut self,
        value: &CapturedModelRouteDisposition,
    ) -> Result<CapturedModelRouteDisposition> {
        if !self.is_read_write()
            || value.id.is_nil()
            || value.workspace_id.is_nil()
            || value.actor_id.is_nil()
            || self.principal_id()? != value.actor_id
        {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        // Decision receipts are immutable too; recheck the current mutable
        // Work/Matrix basis under current_preparation's candidate-set lock.
        let decision: Option<Value> = sqlx::query_scalar(
            "SELECT decision_payload FROM model_route_decisions WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        ).bind(tenant).bind(value.workspace_id).bind(value.decision_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let decision: CapturedModelRouteDecision = decode(decision.ok_or(Error::StaleContext)?)?;
        if !matches!(
            decision.outcome,
            ModelRouteDecisionOutcome::Recommended { .. }
        ) {
            return Err(Error::InputConflict);
        }
        current_preparation(self, &decision.prepared).await?;
        let action = match value.action {
            ModelRouteDispositionAction::Accept => "accept",
            ModelRouteDispositionAction::Reject => "reject",
        };
        sqlx::query(
            "INSERT INTO model_route_dispositions \
             (tenant_id,workspace_id,id,decision_id,actor_id,action,rationale,disposition_payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8)",
        )
        .bind(tenant)
        .bind(value.workspace_id)
        .bind(value.id)
        .bind(value.decision_id)
        .bind(value.actor_id)
        .bind(action)
        .bind(&value.rationale)
        .bind(encode(value)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        Ok(value.clone())
    }
}

#[cfg(test)]
mod tests;
