use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sqlx::{Postgres, Transaction};
use tect_application::AdvisoryStore;
use tect_domain::*;
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct OpportunityRow {
    id: Uuid,
    session_id: Uuid,
    authorized_actor_id: Uuid,
    work_item_kind: String,
    work_item_id: Option<Uuid>,
    source_revision: Option<String>,
    capability: String,
    decision_point: String,
    config_revision: i64,
    session_preference: String,
    request_preference: String,
    request_key: String,
    material_digest: String,
    state: String,
    primary_reason: String,
}

#[derive(sqlx::FromRow)]
struct DispatchRow {
    id: Uuid,
    opportunity_id: Uuid,
    predecessor_dispatch_id: Option<Uuid>,
    attempt_number: i32,
    provider: String,
    model: String,
    configuration_snapshot: serde_json::Value,
    configuration_digest: String,
    material_digest: String,
    payload_digest: String,
    request_payload: Vec<u8>,
    response_payload: Option<Vec<u8>>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_ms: Option<i64>,
    state: String,
    send_certainty: String,
    outcome: Option<String>,
    retry_basis: String,
    raw_response_ref: Option<String>,
}

#[derive(sqlx::FromRow)]
struct OpportunityAuditRow {
    id: Uuid,
    workspace_id: Uuid,
    scope_id: Option<Uuid>,
    session_id: Uuid,
    authorized_actor_id: Uuid,
    work_item_kind: String,
    work_item_id: Option<Uuid>,
    source_revision: Option<String>,
    run_id: Option<Uuid>,
    phase: Option<String>,
    step: Option<String>,
    capability: String,
    decision_point: String,
    config_revision: i64,
    session_preference: String,
    request_preference: String,
    policy_version: String,
    request_key: String,
    material_digest: String,
    deterministic_baseline_ref: Option<String>,
    eligible_material_ref: Option<String>,
    state: String,
    primary_reason: String,
    parent_opportunity_id: Option<Uuid>,
    created_at: String,
    updated_at: String,
}

#[derive(sqlx::FromRow)]
struct AuditLinksRow {
    opportunity_id: Uuid,
    guarded_advice_digest: Option<String>,
    disposition_id: Option<Uuid>,
    preservation_receipt_id: Option<Uuid>,
    preservation_status: Option<String>,
    caller_receipt_id: Option<Uuid>,
    caller_link_id: Option<Uuid>,
    verifier_receipt_id: Option<Uuid>,
    observation_id: Option<Uuid>,
    observation_target_revision: Option<i64>,
    observation_status: Option<String>,
    observation_reason_codes: Option<Vec<String>>,
    observation_evidence_digest: Option<String>,
    observation_qualification: Option<String>,
}

#[derive(sqlx::FromRow)]
struct DispatchAuditRow {
    id: Uuid,
    opportunity_id: Uuid,
    predecessor_dispatch_id: Option<Uuid>,
    attempt_number: i32,
    provider: String,
    model: String,
    configuration_digest: String,
    material_digest: String,
    payload_digest: String,
    request_bytes: i64,
    response_bytes: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_ms: Option<i64>,
    state: String,
    send_certainty: String,
    outcome: Option<String>,
    retry_basis: String,
    raw_response_ref: Option<String>,
    authorized_at: String,
    send_started_at: Option<String>,
    sealed_at: Option<String>,
}

#[derive(sqlx::FromRow)]
struct AuditAggregateRow {
    opportunities: i64,
    opportunities_with_attempts: i64,
    no_call_opportunities: i64,
    authorized_attempts: i64,
    confirmed_sent_attempts: i64,
    send_unknown_attempts: i64,
    proven_unsent_attempts: i64,
    known_input_tokens: i64,
    known_output_tokens: i64,
    attempts_with_unknown_token_usage: i64,
}

#[derive(sqlx::FromRow)]
struct ReasonCountRow {
    reason: String,
    count: i64,
}

fn mode(value: &str) -> Result<WorkspaceAdvisoryMode> {
    match value {
        "disabled" => Ok(WorkspaceAdvisoryMode::Disabled),
        "optional" => Ok(WorkspaceAdvisoryMode::Optional),
        _ => Err(Error::StorageUnavailable),
    }
}

fn capability(value: &str) -> Result<AdvisoryCapability> {
    match value {
        "scope_decomposition" => Ok(AdvisoryCapability::ScopeDecomposition),
        "engineering_profile" => Ok(AdvisoryCapability::EngineeringProfile),
        "pipeline_recommendation" => Ok(AdvisoryCapability::PipelineRecommendation),
        "anti_bloat" => Ok(AdvisoryCapability::AntiBloat),
        "model_routing" => Ok(AdvisoryCapability::ModelRouting),
        _ => Err(Error::StorageUnavailable),
    }
}

fn decision_point(value: &str) -> Result<AdvisoryDecisionPoint> {
    match value {
        SCOPE_DECOMPOSITION_DECISION_POINT => {
            Ok(AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection)
        }
        ENGINEERING_PROFILE_DECISION_POINT => {
            Ok(AdvisoryDecisionPoint::EngineeringProfileBeforeSelection)
        }
        _ => Err(Error::StorageUnavailable),
    }
}

fn preference(value: &str) -> Result<AdvisoryRequestPreference> {
    match value {
        "use_workspace" => Ok(AdvisoryRequestPreference::UseWorkspace),
        "skip" => Ok(AdvisoryRequestPreference::Skip),
        _ => Err(Error::StorageUnavailable),
    }
}

fn opportunity_state(value: &str) -> Result<AdvisoryOpportunityState> {
    match value {
        "prepared" => Ok(AdvisoryOpportunityState::Prepared),
        "no_call" => Ok(AdvisoryOpportunityState::NoCall),
        "awaiting_response" => Ok(AdvisoryOpportunityState::AwaitingResponse),
        "advised" => Ok(AdvisoryOpportunityState::Advised),
        "invalidated" => Ok(AdvisoryOpportunityState::Invalidated),
        "failed" => Ok(AdvisoryOpportunityState::Failed),
        "unresolved" => Ok(AdvisoryOpportunityState::Unresolved),
        _ => Err(Error::StorageUnavailable),
    }
}

fn reason(value: &str) -> Result<AdvisoryReason> {
    match value {
        "workspace_disabled" => Ok(AdvisoryReason::WorkspaceDisabled),
        "session_skip" => Ok(AdvisoryReason::SessionSkip),
        "request_skip" => Ok(AdvisoryReason::RequestSkip),
        "choice_set_not_applicable" => Ok(AdvisoryReason::ChoiceSetNotApplicable),
        "deterministic_input_invalid" => Ok(AdvisoryReason::DeterministicInputInvalid),
        "capability_unavailable" => Ok(AdvisoryReason::CapabilityUnavailable),
        "provider_unconfigured" => Ok(AdvisoryReason::ProviderUnconfigured),
        "budget_policy_invalid" => Ok(AdvisoryReason::BudgetPolicyInvalid),
        "configuration_changed" => Ok(AdvisoryReason::ConfigurationChanged),
        "dispatch_authorized" => Ok(AdvisoryReason::DispatchAuthorized),
        "provider_response" => Ok(AdvisoryReason::ProviderResponse),
        "provider_failure" => Ok(AdvisoryReason::ProviderFailure),
        "send_unknown" => Ok(AdvisoryReason::SendUnknown),
        _ => Err(Error::StorageUnavailable),
    }
}

fn dispatch_state(value: &str) -> Result<AdvisoryDispatchState> {
    match value {
        "authorized" => Ok(AdvisoryDispatchState::Authorized),
        "sending" => Ok(AdvisoryDispatchState::Sending),
        "sealed" => Ok(AdvisoryDispatchState::Sealed),
        "cancelled" => Ok(AdvisoryDispatchState::Cancelled),
        _ => Err(Error::StorageUnavailable),
    }
}

fn send_certainty(value: &str) -> Result<AdvisorySendCertainty> {
    match value {
        "not_sent" => Ok(AdvisorySendCertainty::NotSent),
        "sent" => Ok(AdvisorySendCertainty::Sent),
        "sent_unknown" => Ok(AdvisorySendCertainty::SentUnknown),
        _ => Err(Error::StorageUnavailable),
    }
}

fn dispatch_outcome(value: Option<String>) -> Result<Option<AdvisoryDispatchOutcome>> {
    value
        .map(|value| match value.as_str() {
            "provider_response" => Ok(AdvisoryDispatchOutcome::ProviderResponse),
            "provider_failure" => Ok(AdvisoryDispatchOutcome::ProviderFailure),
            _ => Err(Error::StorageUnavailable),
        })
        .transpose()
}

fn retry_basis(value: &str) -> Result<AdvisoryRetryBasis> {
    match value {
        "initial" => Ok(AdvisoryRetryBasis::Initial),
        "proven_not_sent" => Ok(AdvisoryRetryBasis::ProvenNotSent),
        "known_retryable_response" => Ok(AdvisoryRetryBasis::KnownRetryableResponse),
        "verified_provider_idempotency" => Ok(AdvisoryRetryBasis::VerifiedProviderIdempotency),
        _ => Err(Error::StorageUnavailable),
    }
}

fn dispatch_from_row(row: &DispatchRow) -> Result<AdvisoryDispatch> {
    Ok(AdvisoryDispatch {
        id: row.id,
        opportunity_id: row.opportunity_id,
        predecessor_dispatch_id: row.predecessor_dispatch_id,
        attempt_number: row.attempt_number,
        provider: row.provider.clone(),
        model: row.model.clone(),
        configuration_digest: row.configuration_digest.clone(),
        material_digest: row.material_digest.clone(),
        payload_digest: row.payload_digest.clone(),
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        latency_ms: row.latency_ms,
        state: dispatch_state(&row.state)?,
        send_certainty: send_certainty(&row.send_certainty)?,
        outcome: dispatch_outcome(row.outcome.clone())?,
        retry_basis: retry_basis(&row.retry_basis)?,
        raw_response_ref: row.raw_response_ref.clone(),
    })
}

fn opportunity_audit_from_row(row: OpportunityAuditRow) -> Result<AdvisoryAuditOpportunity> {
    Ok(AdvisoryAuditOpportunity {
        id: row.id,
        workspace_id: row.workspace_id,
        scope_id: row.scope_id,
        session_id: row.session_id,
        authorized_actor_id: row.authorized_actor_id,
        work_item_kind: row.work_item_kind,
        work_item_id: row.work_item_id,
        source_revision: row.source_revision,
        run_id: row.run_id,
        phase: row.phase,
        step: row.step,
        capability: capability(&row.capability)?,
        decision_point: decision_point(&row.decision_point)?,
        config_revision: row.config_revision,
        session_preference: preference(&row.session_preference)?,
        request_preference: preference(&row.request_preference)?,
        policy_version: row.policy_version,
        request_key: row.request_key,
        material_digest: row.material_digest,
        deterministic_baseline_ref: row.deterministic_baseline_ref,
        eligible_material_ref: row.eligible_material_ref,
        state: opportunity_state(&row.state)?,
        primary_reason: reason(&row.primary_reason)?,
        parent_opportunity_id: row.parent_opportunity_id,
        created_at: row.created_at,
        updated_at: row.updated_at,
        guarded_advice_id: None,
        guarded_advice_digest: None,
        disposition_id: None,
        preservation_receipt_id: None,
        preservation_status: None,
        caller_receipt_id: None,
        caller_link_id: None,
        verifier_receipt_id: None,
        selected_save_observation: None,
    })
}

fn apply_audit_links(opportunity: &mut AdvisoryAuditOpportunity, links: &AuditLinksRow) {
    opportunity.guarded_advice_digest = links.guarded_advice_digest.clone();
    opportunity.disposition_id = links.disposition_id;
    opportunity.preservation_receipt_id = links.preservation_receipt_id;
    opportunity.preservation_status = links.preservation_status.clone();
    opportunity.caller_receipt_id = links.caller_receipt_id;
    opportunity.caller_link_id = links.caller_link_id;
    opportunity.verifier_receipt_id = links.verifier_receipt_id;
    opportunity.selected_save_observation = links.observation_id.map(|id| {
        AdvisorySelectedSaveObservation {
            id,
            target_revision: links.observation_target_revision.expect("observation revision"),
            status: match links.observation_status.as_deref() {
                Some("passed") => SelectedSaveObservationStatus::Passed,
                Some("failed") => SelectedSaveObservationStatus::Failed,
                _ => unreachable!("database observation status constraint"),
            },
            reason_codes: links.observation_reason_codes.clone().expect("observation reasons"),
            evidence_digest: links.observation_evidence_digest.clone().expect("observation digest"),
            qualification: links.observation_qualification.clone().expect("observation qualification"),
            establishes_independent_approval: false,
            establishes_current_acceptance: false,
        }
    });
}

fn dispatch_audit_from_row(row: DispatchAuditRow) -> Result<AdvisoryAuditDispatch> {
    Ok(AdvisoryAuditDispatch {
        id: row.id,
        opportunity_id: row.opportunity_id,
        predecessor_dispatch_id: row.predecessor_dispatch_id,
        attempt_number: row.attempt_number,
        provider: row.provider,
        model: row.model,
        configuration_digest: row.configuration_digest,
        material_digest: row.material_digest,
        payload_digest: row.payload_digest,
        request_bytes: row.request_bytes,
        response_bytes: row.response_bytes,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        latency_ms: row.latency_ms,
        state: dispatch_state(&row.state)?,
        send_certainty: send_certainty(&row.send_certainty)?,
        outcome: dispatch_outcome(row.outcome)?,
        retry_basis: retry_basis(&row.retry_basis)?,
        raw_response_ref: row.raw_response_ref,
        authorized_at: row.authorized_at,
        send_started_at: row.send_started_at,
        sealed_at: row.sealed_at,
    })
}

fn count(value: i64) -> Result<u64> {
    u64::try_from(value).map_err(|_| Error::StorageUnavailable)
}

fn audit_aggregate_from_rows(
    row: AuditAggregateRow,
    reasons: Vec<ReasonCountRow>,
) -> Result<AdvisoryAuditAggregate> {
    let no_call_by_reason = reasons
        .into_iter()
        .map(|row| {
            Ok(AdvisoryReasonCount {
                reason: reason(&row.reason)?,
                count: count(row.count)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    Ok(AdvisoryAuditAggregate {
        opportunities: count(row.opportunities)?,
        opportunities_with_attempts: count(row.opportunities_with_attempts)?,
        no_call_opportunities: count(row.no_call_opportunities)?,
        no_call_by_reason,
        authorized_attempts: count(row.authorized_attempts)?,
        confirmed_sent_attempts: count(row.confirmed_sent_attempts)?,
        send_unknown_attempts: count(row.send_unknown_attempts)?,
        proven_unsent_attempts: count(row.proven_unsent_attempts)?,
        known_input_tokens: count(row.known_input_tokens)?,
        known_output_tokens: count(row.known_output_tokens)?,
        attempts_with_unknown_token_usage: count(row.attempts_with_unknown_token_usage)?,
    })
}

fn dispatch_matches_authorization(
    row: &DispatchRow,
    input: &AdvisoryDispatchAuthorization,
) -> Result<bool> {
    Ok(row.id == input.dispatch_id
        && row.predecessor_dispatch_id == input.predecessor_dispatch_id
        && row.provider == input.provider
        && row.model == input.model
        && row.configuration_snapshot == input.configuration_snapshot
        && row.configuration_digest == input.configuration_digest
        && row.material_digest == input.material_digest
        && row.payload_digest == input.payload_digest
        && row.request_payload == input.request_payload
        && retry_basis(&row.retry_basis)? == input.retry_basis)
}

fn dispatch_matches_seal(row: &DispatchRow, seal: &AdvisoryDispatchSeal) -> Result<bool> {
    Ok(row.id == seal.dispatch_id
        && row.response_payload == seal.response_payload
        && row.input_tokens == seal.input_tokens
        && row.output_tokens == seal.output_tokens
        && row.latency_ms == seal.latency_ms
        && send_certainty(&row.send_certainty)? == seal.send_certainty
        && dispatch_outcome(row.outcome.clone())? == Some(seal.outcome)
        && row.raw_response_ref == seal.raw_response_ref)
}
