use super::RunScopeAdvisory;
use crate::{
    ScopeAdviceProviderError, ScopeAdviceProviderObservation, ScopeAuthoredManifestRequest,
    ScopeAuthorityObservation, ScopeAuthorityRequest, ScopeAuthorizedInvalidObservation,
    ScopeManifestSupplier, StoredScopeManifestRecord,
};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryCapability, AdvisoryDecisionPoint, AdvisoryDispatchOutcome, AdvisoryOpportunity,
    AdvisoryOpportunityInput, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
    AdvisorySendCertainty, Error, Result, ScopeConstructorManifest, WorkspaceAdvisoryConfig,
    WorkspaceAdvisoryMode,
};
use uuid::Uuid;

#[async_trait]
pub(super) trait EarlyCandidateRevision: Send {
    async fn revision(&mut self, workspace_id: Uuid, candidate_set_id: Uuid)
    -> Result<Option<i64>>;
}

#[async_trait]
impl EarlyCandidateRevision for dyn crate::UnitOfWork + '_ {
    async fn revision(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<i64>> {
        self.candidate_revision(workspace_id, candidate_set_id)
            .await
    }
}

pub(super) async fn early_no_call_target<T: EarlyCandidateRevision + ?Sized>(
    port: &mut T,
    workspace_id: Uuid,
    config: &WorkspaceAdvisoryConfig,
    request: &RunScopeAdvisory,
) -> Result<Option<(AdvisoryReason, i64)>> {
    let Some(reason) = early_no_call_reason(config, request) else {
        return Ok(None);
    };
    let revision = port
        .revision(workspace_id, request.candidate_set_id)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(Some((reason, revision)))
}

pub(super) fn early_no_call_reason(
    config: &WorkspaceAdvisoryConfig,
    request: &RunScopeAdvisory,
) -> Option<AdvisoryReason> {
    if config.mode == WorkspaceAdvisoryMode::Disabled {
        Some(AdvisoryReason::WorkspaceDisabled)
    } else if request.session_preference == AdvisoryRequestPreference::Skip {
        Some(AdvisoryReason::SessionSkip)
    } else if request.request_preference == AdvisoryRequestPreference::Skip {
        Some(AdvisoryReason::RequestSkip)
    } else {
        None
    }
}

pub(super) fn scope_opportunity_input(
    request: &RunScopeAdvisory,
    config: &WorkspaceAdvisoryConfig,
    actor: Uuid,
    session: Uuid,
    material_digest: String,
    state: AdvisoryOpportunityState,
    reason: AdvisoryReason,
    revision: Option<i64>,
) -> AdvisoryOpportunityInput {
    AdvisoryOpportunityInput {
        session_id: session,
        authorized_actor_id: actor,
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: tect_domain::ADVISORY_DECISION_POINT_VERSION,
        workflow_occurrence_key: request.request_id.to_string(),
        target_kind: "scope_candidate_set".into(),
        target_id: Some(request.candidate_set_id),
        work_revision: revision,
        source_ref: None,
        session_preference: request.session_preference,
        request_preference: request.request_preference,
        config_revision: config.revision,
        material_digest,
        state,
        primary_reason: reason,
    }
}

pub(super) fn validate_invalid_observation(
    request: &ScopeAuthorityRequest,
    observation: &ScopeAuthorizedInvalidObservation,
) -> Result<()> {
    if (
        observation.workspace_id,
        observation.actor_id,
        observation.session_id,
        observation.candidate_set_id,
    ) != (
        request.workspace_id,
        request.actor_id,
        request.session_id,
        request.candidate_set_id,
    ) {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) fn validate_observation(
    request: &ScopeAuthorityRequest,
    observation: &ScopeAuthorityObservation,
) -> Result<()> {
    if (
        observation.workspace_id,
        observation.actor_id,
        observation.session_id,
        observation.candidate_set_id,
    ) != (
        request.workspace_id,
        request.actor_id,
        request.session_id,
        request.candidate_set_id,
    ) || observation.source.candidate_set_id != request.candidate_set_id
    {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn supply_scope_manifest(
    supplier: &dyn ScopeManifestSupplier,
    tenant_id: Uuid,
    observation: &ScopeAuthorityObservation,
    request: &RunScopeAdvisory,
) -> Result<ScopeConstructorManifest> {
    let manifest = if let Some(authored_scope_set) = &request.authored_scope_set {
        if authored_scope_set.expected_candidate_set_revision
            != observation.source.candidate_set_revision
        {
            return Err(Error::StaleRevision);
        }
        supplier
            .supply_authored(&ScopeAuthoredManifestRequest {
                tenant_id,
                observation: observation.clone(),
                authored_scope_set: authored_scope_set.clone(),
            })
            .await?
    } else {
        supplier.supply(observation).await?
    };
    manifest.validate(&crate::Sha256ScopeDigest)?;
    if manifest.source != observation.source || manifest.obligations != observation.obligations {
        return Err(Error::InputConflict);
    }
    Ok(manifest)
}

pub(super) fn validate_authored_replay_binding<'a>(
    stored: &StoredScopeManifestRecord,
    opportunity: Option<&'a AdvisoryOpportunity>,
    request: &RunScopeAdvisory,
    config: &WorkspaceAdvisoryConfig,
    actor_id: Uuid,
    session_id: Uuid,
    authored_request_digest: &str,
) -> Result<&'a AdvisoryOpportunity> {
    let opportunity = opportunity.ok_or(Error::StorageUnavailable)?;
    let manifest = &stored.record.manifest;
    manifest.validate(&crate::Sha256ScopeDigest)?;
    if stored.authored_request_digest.as_deref() != Some(authored_request_digest)
        || stored.record.opportunity_id != opportunity.id
        || opportunity.workflow_occurrence_key != request.request_id.to_string()
        || opportunity.authorized_actor_id != actor_id
        || opportunity.session_id != session_id
        || opportunity.target_kind != "scope_candidate_set"
        || opportunity.target_id != Some(request.candidate_set_id)
        || stored.record.candidate_set_id != request.candidate_set_id
        || manifest.source.candidate_set_id != request.candidate_set_id
        || manifest.source.candidate_set_revision
            != request
                .authored_scope_set
                .as_ref()
                .ok_or(Error::InputConflict)?
                .expected_candidate_set_revision
        || stored.record.config_revision != config.revision
        || stored.record.opportunity_material_digest != manifest.whole_set_digest
        || opportunity.work_revision != Some(manifest.source.candidate_set_revision)
        || opportunity.config_revision != config.revision
        || opportunity.material_digest != manifest.whole_set_digest
    {
        return Err(Error::InputConflict);
    }
    Ok(opportunity)
}

pub(super) fn validate_authored_no_call_replay<'a>(
    opportunity: Option<&'a AdvisoryOpportunity>,
    request: &RunScopeAdvisory,
    config: &WorkspaceAdvisoryConfig,
    actor_id: Uuid,
    session_id: Uuid,
) -> Result<&'a AdvisoryOpportunity> {
    let opportunity = opportunity.ok_or(Error::StorageUnavailable)?;
    let authored = request
        .authored_scope_set
        .as_ref()
        .ok_or(Error::InputConflict)?;
    let expected_revision =
        if opportunity.primary_reason == AdvisoryReason::DeterministicInputInvalid {
            None
        } else {
            Some(authored.expected_candidate_set_revision)
        };
    let expected_digest = no_call_digest(request, config.revision, opportunity.primary_reason)?;
    if opportunity.state != AdvisoryOpportunityState::NoCall
        || opportunity.workflow_occurrence_key != request.request_id.to_string()
        || opportunity.authorized_actor_id != actor_id
        || opportunity.session_id != session_id
        || opportunity.target_kind != "scope_candidate_set"
        || opportunity.target_id != Some(request.candidate_set_id)
        || opportunity.work_revision != expected_revision
        || opportunity.config_revision != config.revision
        || opportunity.material_digest != expected_digest
    {
        return Err(Error::InputConflict);
    }
    Ok(opportunity)
}

pub(super) fn failed_observation() -> ScopeAdviceProviderObservation {
    ScopeAdviceProviderObservation {
        send_certainty: AdvisorySendCertainty::NotSent,
        outcome: AdvisoryDispatchOutcome::ProviderFailure,
        answers: None,
        response_payload: None,
        input_tokens: None,
        output_tokens: None,
        latency_ms: None,
        raw_response_ref: None,
        failure_reason: None,
    }
}

pub(super) fn provider_error_observation(
    error: ScopeAdviceProviderError,
) -> ScopeAdviceProviderObservation {
    match error {
        ScopeAdviceProviderError::ProvenNotSent => failed_observation(),
        ScopeAdviceProviderError::SentUnknown {
            raw_response_ref,
            latency_ms,
        } => ScopeAdviceProviderObservation {
            send_certainty: AdvisorySendCertainty::SentUnknown,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            answers: None,
            response_payload: None,
            input_tokens: None,
            output_tokens: None,
            latency_ms: Some(latency_ms),
            raw_response_ref,
            failure_reason: None,
        },
    }
}

pub(super) fn normalize_provider_success(
    observation: ScopeAdviceProviderObservation,
) -> ScopeAdviceProviderObservation {
    if observation.send_certainty == AdvisorySendCertainty::Sent {
        observation
    } else {
        provider_error_observation(ScopeAdviceProviderError::SentUnknown {
            raw_response_ref: observation.raw_response_ref,
            latency_ms: observation.latency_ms.unwrap_or(0),
        })
    }
}

pub(super) fn no_call_digest(
    request: &RunScopeAdvisory,
    config_revision: i64,
    reason: AdvisoryReason,
) -> Result<String> {
    let mut value = serde_json::json!({
        "schema": "tect.scope-advisory-no-call/1",
        "request_id": request.request_id,
        "candidate_set_id": request.candidate_set_id,
        "session_preference": request.session_preference.as_str(),
        "request_preference": request.request_preference.as_str(),
        "config_revision": config_revision,
        "reason": reason.as_str(),
    });
    if let Some(authored) = &request.authored_scope_set {
        value["authored_request_digest"] =
            serde_json::Value::String(authored_request_digest(authored)?);
    }
    Ok(sha256(
        &serde_json::to_vec(&value).map_err(Error::invalid_arguments_from)?,
    ))
}

pub(super) fn authored_request_digest(authored: &super::AuthoredScopeSet) -> Result<String> {
    authored.validate()?;
    let bytes = serde_json::to_vec(authored).map_err(Error::invalid_arguments_from)?;
    let mut digest = Sha256::new();
    digest.update(b"tect.scope-advisory-authored-request/1\0");
    digest.update(bytes);
    Ok(format!("{:x}", digest.finalize()))
}

pub(super) fn sha256(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
