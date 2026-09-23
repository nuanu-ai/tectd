use async_trait::async_trait;
use tect_domain::{
    FreshScopeObservation, GuardedScopeAdvice, Result, ScopeConstructorManifest,
    ScopeDispositionRequest, ScopeDispositionRevision, ScopePreservationResult,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeManifestRecord {
    pub opportunity_id: Uuid,
    pub case_id: Uuid,
    pub config_revision: i64,
    pub opportunity_material_digest: String,
    pub manifest: ScopeConstructorManifest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedScopeAdviceRecord {
    pub opportunity_id: Uuid,
    pub case_id: Uuid,
    pub dispatch_id: Uuid,
    pub dispatch_material_digest: String,
    pub config_revision: i64,
    pub advice: GuardedScopeAdvice,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeDispositionRecord {
    pub opportunity_id: Uuid,
    pub case_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub request: ScopeDispositionRequest,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopePreservationReceiptInput {
    pub receipt_id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub case_id: Uuid,
    pub disposition_id: Uuid,
    pub observation: FreshScopeObservation,
    pub result: ScopePreservationResult,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeCallerLinkInput {
    pub link_id: Uuid,
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub case_id: Uuid,
    pub disposition_id: Uuid,
    pub preservation_receipt_id: Uuid,
    pub candidate_set_id: Uuid,
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
    pub case_id: Uuid,
    pub caller_link_id: Uuid,
    pub candidate_set_id: Uuid,
    pub actor_id: Uuid,
    pub session_id: Uuid,
    pub verified_revision: i64,
    pub verifier_digest: String,
}

#[async_trait]
pub trait ScopeAdvisoryStore: Send {
    async fn prepare_scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        record: &ScopeManifestRecord,
    ) -> Result<ScopeConstructorManifest>;

    async fn scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<ScopeConstructorManifest>>;

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
}
