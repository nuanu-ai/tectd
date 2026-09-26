use async_trait::async_trait;
use tect_domain::{
    AdvisoryReason, FreshScopeObservation, GuardedScopeAdvice, Result, ScopeConstructorManifest,
    ScopeDispositionRequest, ScopeDispositionRevision, ScopePreservationResult,
    SelectedSaveObservation, SelectedSaveObservationRequest,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeManifestRecord {
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub config_revision: i64,
    pub opportunity_material_digest: String,
    pub manifest: ScopeConstructorManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredScopeManifestRecord {
    pub record: ScopeManifestRecord,
    /// Canonical caller-authored input digest, absent only for legacy requests.
    pub authored_request_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePreparedAdvisoryDisposition {
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub expected_source_digest: String,
    pub reason: AdvisoryReason,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedScopeAdviceRecord {
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub dispatch_id: Uuid,
    pub dispatch_material_digest: String,
    pub config_revision: i64,
    pub advice: GuardedScopeAdvice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDispositionRecord {
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub request: ScopeDispositionRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePreservationReceiptInput {
    pub receipt_id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub disposition_id: Uuid,
    pub observation: FreshScopeObservation,
    pub result: ScopePreservationResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeCallerLinkInput {
    pub link_id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub disposition_id: Uuid,
    pub preservation_receipt_id: Uuid,
    pub caller_operation: String,
    pub caller_request_id: Uuid,
    pub caller_result_revision: i64,
    pub actor_id: Uuid,
    pub session_id: Uuid,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeVerifierReceiptInput {
    pub receipt_id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_link_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub verified_revision: i64,
    pub verifier_digest: String,
}

#[async_trait]
pub trait ScopeAdvisoryStore: Send {
    /// Terminal interpretation failure; original transport dispatch stays immutable.
    async fn finalize_scope_advisory_without_advice(
        &mut self,
        _workspace_id: Uuid,
        _opportunity_id: Uuid,
        _expected_config_revision: i64,
        _dispatch: &tect_domain::AdvisoryDispatch,
    ) -> Result<tect_domain::AdvisoryOpportunity> {
        Err(tect_domain::Error::Forbidden)
    }
    async fn prepare_scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        record: &ScopeManifestRecord,
    ) -> Result<ScopeConstructorManifest>;

    async fn prepare_authored_scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        record: &ScopeManifestRecord,
        authored_request_digest: &str,
    ) -> Result<ScopeConstructorManifest>;

    async fn scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<ScopeConstructorManifest>>;

    async fn scope_advisory_manifest_by_request_key(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<StoredScopeManifestRecord>>;

    async fn guarded_scope_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<GuardedScopeAdvice>>;

    async fn persist_guarded_scope_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedScopeAdviceRecord,
    ) -> Result<GuardedScopeAdvice>;

    async fn finalize_guarded_scope_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedScopeAdviceRecord,
    ) -> Result<GuardedScopeAdvice>;

    async fn invalidate_scope_advisory(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<()>;

    async fn finalize_prepared_scope_advisory_without_dispatch(
        &mut self,
        workspace_id: Uuid,
        record: &ScopePreparedAdvisoryDisposition,
    ) -> Result<()>;

    async fn cas_scope_advisory_disposition(
        &mut self,
        workspace_id: Uuid,
        record: ScopeDispositionRecord,
    ) -> Result<ScopeDispositionRevision>;

    async fn persist_scope_preservation_receipt(
        &mut self,
        workspace_id: Uuid,
        input: &ScopePreservationReceiptInput,
    ) -> Result<Uuid>;

    async fn link_scope_advisory_caller(
        &mut self,
        workspace_id: Uuid,
        input: &ScopeCallerLinkInput,
    ) -> Result<Uuid>;

    async fn persist_scope_verifier_receipt(
        &mut self,
        workspace_id: Uuid,
        input: &ScopeVerifierReceiptInput,
    ) -> Result<Uuid>;

    /// Internal server-computed postcondition observation; no public verifier route.
    async fn observe_selected_scope_save(
        &mut self,
        workspace_id: Uuid,
        request: &SelectedSaveObservationRequest,
    ) -> Result<SelectedSaveObservation>;

    /// Internal authenticated verifier pass; it does not grant approval or acceptance.
    async fn independently_observe_selected_scope_save(
        &mut self,
        workspace_id: Uuid,
        request: &SelectedSaveObservationRequest,
    ) -> Result<SelectedSaveObservation>;
}
