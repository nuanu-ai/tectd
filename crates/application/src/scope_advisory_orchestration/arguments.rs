use tect_domain::{AdvisoryOpportunityState, AdvisoryReason};
use uuid::Uuid;

pub(super) struct ScopeCaptureIdentity {
    pub(super) actor: Uuid,
    pub(super) session: Uuid,
    pub(super) material_digest: String,
}

pub(super) struct ScopeCaptureStatus {
    pub(super) state: AdvisoryOpportunityState,
    pub(super) reason: AdvisoryReason,
}

pub(super) struct ScopeDispatchMetadata<'a> {
    pub(super) dispatch_id: Uuid,
    pub(super) opportunity_id: Uuid,
    pub(super) provider: &'a str,
    pub(super) adapter_version: &'a str,
    pub(super) material_digest: String,
}
