use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_application::{
    AntiBloatAttemptState, AntiBloatAuthoredDelta, AntiBloatNoCall, AntiBloatPreparedRequest,
    AntiBloatProviderObservation, AntiBloatSendPermit, AntiBloatStore, Sha256ScopeDigest,
    StoredAntiBloatReview,
};
use tect_domain::{
    AdvisoryBudgetPolicy, AntiBloatApplyReceipt, AntiBloatDisposition, AntiBloatInput,
    AntiBloatObligationLink, AntiBloatPreservation, Error, ResolvedCandidateDraft, Result,
    ScopeConstructorManifest, WorkspaceAdvisoryMode, check_anti_bloat_delta, review_anti_bloat,
    scope_candidate_material_digest,
};
use uuid::Uuid;

mod apply;
mod input;
mod response;
mod review;
mod send;

#[derive(sqlx::FromRow)]
struct SelectedBindingRow {
    manifest_payload: serde_json::Value,
    draft_payload: Option<serde_json::Value>,
    obligation_links: serde_json::Value,
    non_goal_source_obligation_ids: serde_json::Value,
    mandatory_policy_obligation_ids: serde_json::Value,
    dependency_digest: String,
    source_digest: String,
    provenance: String,
    set_revision: i64,
    current_snapshot_id: Option<Uuid>,
    selected_draft_revision: i64,
    selected_material_digest: String,
    selected_alternative_id: String,
    selected_caller_link_id: Uuid,
    selected_caller_request_id: Uuid,
    caller_request_id: Uuid,
    receipt_revision: i64,
    disposition_alternative_id: Option<String>,
}

#[derive(sqlx::FromRow)]
struct CurrentSourceAuthority {
    current_snapshot_id: Option<Uuid>,
    input_cursor: i64,
    candidate_latest_input: i64,
    program_id: Uuid,
    program_revision: i64,
    program_current_latest: i64,
    program_latest_input: i64,
    planning_latest_input: i64,
    selected_sources_digest: String,
    method_revision: String,
    method_digest: String,
    registry_revision: String,
    registry_digest: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn state_name(state: &AntiBloatAttemptState) -> &'static str {
    match state {
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::Disabled) => "disabled",
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::Skipped) => "skipped",
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::NoEligibleFindings) => "no_eligible",
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::ProviderUnconfigured) => {
            "provider_unconfigured"
        }
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInvalidConfiguration) => {
            "preflight_invalid_configuration"
        }
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInvalidArguments) => {
            "preflight_invalid_arguments"
        }
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInputConflict) => {
            "preflight_input_conflict"
        }
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightRequestTooLarge) => {
            "preflight_request_too_large"
        }
        AntiBloatAttemptState::Prepared => "prepared",
        AntiBloatAttemptState::Sending => "sending",
        AntiBloatAttemptState::Ranked(_) => "ranked",
        AntiBloatAttemptState::SendUnknown => "send_unknown",
        AntiBloatAttemptState::ProviderAbstained => "provider_abstained",
        AntiBloatAttemptState::InvalidResponse => "invalid_response",
    }
}

fn parse_state(name: &str, ranked: Option<serde_json::Value>) -> Result<AntiBloatAttemptState> {
    Ok(match name {
        "disabled" => AntiBloatAttemptState::NoCall(AntiBloatNoCall::Disabled),
        "skipped" => AntiBloatAttemptState::NoCall(AntiBloatNoCall::Skipped),
        "no_eligible" => AntiBloatAttemptState::NoCall(AntiBloatNoCall::NoEligibleFindings),
        "provider_unconfigured" => {
            AntiBloatAttemptState::NoCall(AntiBloatNoCall::ProviderUnconfigured)
        }
        "preflight_invalid_configuration" => {
            AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInvalidConfiguration)
        }
        "preflight_invalid_arguments" => {
            AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInvalidArguments)
        }
        "preflight_input_conflict" => {
            AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInputConflict)
        }
        "preflight_request_too_large" => {
            AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightRequestTooLarge)
        }
        "prepared" => AntiBloatAttemptState::Prepared,
        "sending" => AntiBloatAttemptState::Sending,
        "send_unknown" => AntiBloatAttemptState::SendUnknown,
        "provider_abstained" => AntiBloatAttemptState::ProviderAbstained,
        "invalid_response" => AntiBloatAttemptState::InvalidResponse,
        "ranked" => AntiBloatAttemptState::Ranked(
            serde_json::from_value(ranked.ok_or(Error::InternalInvariant)?)
                .map_err(storage_error)?,
        ),
        _ => return Err(Error::InternalInvariant),
    })
}

#[async_trait]
impl AntiBloatStore for PgUnitOfWork {
    async fn record_preflight_no_call(
        &mut self,
        review_id: Uuid,
        reason: AntiBloatNoCall,
    ) -> Result<()> {
        response::record_preflight_no_call(self, review_id, reason).await
    }
    async fn authorized_budget_policy(
        &mut self,
        workspace_id: Uuid,
        now_unix_ms: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        self.verified_budget_policy(workspace_id, now_unix_ms).await
    }

    async fn advisory_mode(&mut self, workspace_id: Uuid) -> Result<WorkspaceAdvisoryMode> {
        input::advisory_mode(self, workspace_id).await
    }

    async fn authoritative_input(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        expected_revision: i64,
    ) -> Result<Option<AntiBloatInput>> {
        input::authoritative_input(self, workspace_id, candidate_set_id, expected_revision).await
    }

    async fn save_review(
        &mut self,
        record: StoredAntiBloatReview,
    ) -> Result<StoredAntiBloatReview> {
        review::save_review(self, record).await
    }

    async fn review(&mut self, review_id: Uuid) -> Result<Option<StoredAntiBloatReview>> {
        review::review(self, review_id).await
    }

    async fn begin_send(
        &mut self,
        saved: &StoredAntiBloatReview,
        prepared: &AntiBloatPreparedRequest,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<Option<AntiBloatSendPermit>> {
        send::begin_send(self, saved, prepared, policy).await
    }

    async fn mark_send_unknown(&mut self, review_id: Uuid) -> Result<()> {
        send::mark_send_unknown(self, review_id).await
    }

    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        raw_response: &[u8],
        response_sha256: &str,
    ) -> Result<()> {
        send::seal_response(self, permit, raw_response, response_sha256).await
    }

    async fn consume_budget(
        &mut self,
        permit: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<bool> {
        send::consume_budget(self, permit, observation).await
    }

    async fn authorized_sealed_response(
        &mut self,
        permit: &AntiBloatSendPermit,
    ) -> Result<Vec<u8>> {
        send::authorized_sealed_response(self, permit).await
    }

    async fn seal_ranked(&mut self, review_id: Uuid, ranked_ids: &[String]) -> Result<()> {
        send::seal_ranked(self, review_id, ranked_ids).await
    }

    async fn sealed_response_for_usage(&mut self, permit: &AntiBloatSendPermit) -> Result<Vec<u8>> {
        response::sealed_response_for_usage(self, permit).await
    }

    async fn seal_terminal(
        &mut self,
        permit: &AntiBloatSendPermit,
        state: AntiBloatAttemptState,
    ) -> Result<()> {
        response::seal_terminal(self, permit, state).await
    }

    async fn apply_preserved_delta(
        &mut self,
        authored: &AntiBloatAuthoredDelta,
        input: &AntiBloatInput,
        preservation: &AntiBloatPreservation,
        after: &ResolvedCandidateDraft,
    ) -> Result<AntiBloatApplyReceipt> {
        apply::apply_preserved_delta(self, authored, input, preservation, after).await
    }
}
