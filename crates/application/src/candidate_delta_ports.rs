use async_trait::async_trait;
use tect_domain::{CandidateDeltaBatch, CandidateDeltaReceipt, Result};
use uuid::Uuid;

/// Additive port for operation-oriented candidate mutations.  It is kept
/// separate from ScopeCandidateStore so existing adapters and mocks retain the
/// full-snapshot contract unchanged.
#[async_trait]
pub trait CandidateDeltaStore: Send {
    async fn apply_candidate_delta(
        &mut self,
        workspace_id: Uuid,
        request: &CandidateDeltaBatch,
    ) -> Result<CandidateDeltaReceipt>;

    async fn candidate_delta_status(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        idempotency_key: &str,
    ) -> Result<Option<CandidateDeltaReceipt>>;
}
