use crate::{MatrixProviderBinding, MatrixProviderRequest};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryDispatchAuthorization, AdvisoryDispatchStart, AdvisoryDispatchState,
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, AdvisorySendCertainty, Error, Result,
};
use uuid::Uuid;

/// Application ceiling for a prepared Matrix request body. A host may impose
/// a smaller transport limit before calling the provider.
pub const MAX_PREPARED_MATRIX_BODY_BYTES: usize = 1024 * 1024;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixProviderIdentity {
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    pub destination: String,
    pub wire_version: String,
}

impl MatrixProviderIdentity {
    fn validate_for(&self, request: &MatrixProviderRequest) -> Result<()> {
        self.provider_profile_ref.validate()?;
        self.model_configuration.validate()?;
        if self.provider_profile_ref != *request.provider_profile_ref()
            || self.model_configuration != *request.model_configuration()
            || self.destination.is_empty()
            || self.destination.contains('\0')
            || self.wire_version.is_empty()
            || self.wire_version.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

/// Exact provider body and target fixed before any budget authorization. Its
/// private fields prevent callers from changing the request after preparation.
#[derive(PartialEq, Eq)]
pub struct PreparedMatrixAdviceAttempt {
    binding: MatrixProviderBinding,
    identity: MatrixProviderIdentity,
    body: Vec<u8>,
    body_sha256: String,
}

impl std::fmt::Debug for PreparedMatrixAdviceAttempt {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter
            .debug_struct("PreparedMatrixAdviceAttempt")
            .field("binding", &self.binding)
            .field("identity", &self.identity)
            .field("body", &"[redacted]")
            .field("body_length", &self.body.len())
            .field("body_sha256", &self.body_sha256)
            .finish()
    }
}

impl PreparedMatrixAdviceAttempt {
    pub fn new(
        request: &MatrixProviderRequest,
        identity: MatrixProviderIdentity,
        body: Vec<u8>,
    ) -> Result<Self> {
        identity.validate_for(request)?;
        if body.is_empty() || body.len() > MAX_PREPARED_MATRIX_BODY_BYTES {
            return Err(Error::RequestTooLarge);
        }
        if std::str::from_utf8(&body).is_err() {
            return Err(Error::InvalidArguments);
        }
        let body_sha256 = format!("{:x}", Sha256::digest(&body));
        Ok(Self {
            binding: request.binding().clone(),
            identity,
            body,
            body_sha256,
        })
    }

    pub fn binding(&self) -> &MatrixProviderBinding {
        &self.binding
    }

    pub fn identity(&self) -> &MatrixProviderIdentity {
        &self.identity
    }

    pub fn body(&self) -> &[u8] {
        &self.body
    }

    pub fn body_length(&self) -> usize {
        self.body.len()
    }

    pub fn body_sha256(&self) -> &str {
        &self.body_sha256
    }

    pub fn validate_for(&self, request: &MatrixProviderRequest) -> Result<()> {
        self.identity.validate_for(request)?;
        if self.binding != *request.binding() {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    /// Transfer the original body allocation to the transport adapter.
    pub fn into_parts(
        self,
    ) -> (
        MatrixProviderBinding,
        MatrixProviderIdentity,
        Vec<u8>,
        String,
    ) {
        (self.binding, self.identity, self.body, self.body_sha256)
    }
}

/// One-use capability for an exact Matrix transport attempt. Only application
/// orchestration can mint it after the dispatch-start transaction commits.
pub struct MatrixStartedDispatchPermit {
    opportunity_id: Uuid,
    dispatch_id: Uuid,
    configuration_digest: String,
    binding: MatrixProviderBinding,
    identity: MatrixProviderIdentity,
    body_length: usize,
    body_sha256: String,
}

impl MatrixStartedDispatchPermit {
    pub(crate) fn after_committed_start(
        started: &AdvisoryDispatchStart,
        authorization: &AdvisoryDispatchAuthorization,
        prepared: &PreparedMatrixAdviceAttempt,
    ) -> Result<Self> {
        let dispatch = &started.dispatch;
        let configuration_bytes = serde_json::to_vec(&authorization.configuration_snapshot)
            .map_err(Error::invalid_arguments_from)?;
        let configuration_digest = format!("{:x}", Sha256::digest(&configuration_bytes));
        let config = &authorization.configuration_snapshot;
        if !started.should_send
            || dispatch.state != AdvisoryDispatchState::Sending
            || dispatch.send_certainty != AdvisorySendCertainty::SentUnknown
            || dispatch.id != authorization.dispatch_id
            || dispatch.opportunity_id != authorization.opportunity_id
            || dispatch.configuration_digest != authorization.configuration_digest
            || authorization.configuration_digest != configuration_digest
            || dispatch.material_digest != authorization.material_digest
            || dispatch.payload_digest != authorization.payload_digest
            || authorization.payload_digest != prepared.body_sha256
            || authorization.request_payload != prepared.body
            || authorization.model != prepared.identity.model_configuration.model
            || config.get("provider_profile_ref")
                != Some(&serde_json::json!(prepared.identity.provider_profile_ref))
            || config.get("model_configuration")
                != Some(&serde_json::json!(prepared.identity.model_configuration))
            || config.get("destination") != Some(&serde_json::json!(prepared.identity.destination))
            || config.get("wire_version")
                != Some(&serde_json::json!(prepared.identity.wire_version))
            || config.get("request_body_length") != Some(&serde_json::json!(prepared.body_length()))
            || config.get("request_body_sha256") != Some(&serde_json::json!(prepared.body_sha256))
            || prepared.body_length() != prepared.body.len()
            || authorization.validate().is_err()
        {
            return Err(Error::InputConflict);
        }
        Ok(Self {
            opportunity_id: dispatch.opportunity_id,
            dispatch_id: dispatch.id,
            configuration_digest: dispatch.configuration_digest.clone(),
            binding: prepared.binding.clone(),
            identity: prepared.identity.clone(),
            body_length: prepared.body_length(),
            body_sha256: prepared.body_sha256.clone(),
        })
    }

    pub fn permits(
        &self,
        opportunity_id: Uuid,
        dispatch_id: Uuid,
        configuration_digest: &str,
        prepared: &PreparedMatrixAdviceAttempt,
    ) -> bool {
        self.opportunity_id == opportunity_id
            && self.dispatch_id == dispatch_id
            && self.configuration_digest == configuration_digest
            && self.binding == prepared.binding
            && self.identity == prepared.identity
            && self.body_length == prepared.body_length()
            && self.body_sha256 == prepared.body_sha256
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixBudgetRequest {
    pub workspace_id: Uuid,
    pub actor_id: Uuid,
    pub binding: MatrixProviderBinding,
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    pub destination: String,
    pub wire_version: String,
    pub body_length: usize,
    pub body_sha256: String,
}

impl MatrixBudgetRequest {
    pub fn from_prepared(
        workspace_id: Uuid,
        actor_id: Uuid,
        prepared: &PreparedMatrixAdviceAttempt,
    ) -> Result<Self> {
        if workspace_id.is_nil() || actor_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        Ok(Self {
            workspace_id,
            actor_id,
            binding: prepared.binding.clone(),
            provider_profile_ref: prepared.identity.provider_profile_ref.clone(),
            model_configuration: prepared.identity.model_configuration.clone(),
            destination: prepared.identity.destination.clone(),
            wire_version: prepared.identity.wire_version.clone(),
            body_length: prepared.body_length(),
            body_sha256: prepared.body_sha256.clone(),
        })
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixBudgetAuthorization {
    pub policy_id: String,
}

#[async_trait]
pub trait MatrixBudgetPolicy: Send + Sync {
    /// Pure decision; implementations must not reserve or charge. `None` denies.
    async fn authorize(
        &self,
        request: &MatrixBudgetRequest,
    ) -> Result<Option<MatrixBudgetAuthorization>>;
}

#[derive(Debug, Default)]
pub struct DenyMatrixBudget;

#[async_trait]
impl MatrixBudgetPolicy for DenyMatrixBudget {
    async fn authorize(
        &self,
        _: &MatrixBudgetRequest,
    ) -> Result<Option<MatrixBudgetAuthorization>> {
        Ok(None)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{DisabledMatrixAdviceProvider, MatrixAdviceProvider};

    #[tokio::test]
    async fn disabled_identity_and_default_budget_deny() {
        let provider = DisabledMatrixAdviceProvider;
        assert_eq!(provider.identity(), None);
        let budget = DenyMatrixBudget;
        let request = MatrixBudgetRequest {
            workspace_id: Uuid::new_v4(),
            actor_id: Uuid::new_v4(),
            binding: MatrixProviderBinding {
                task_id: Uuid::new_v4(),
                task_revision: 1,
                input_digest: "input".into(),
                choice_set_id: "choice".into(),
                choice_set_version: 1,
                choice_set_digest: "choice-digest".into(),
                evaluation_digest: "evaluation".into(),
            },
            provider_profile_ref: AdvisoryProviderProfileRef { id: "test".into() },
            model_configuration: AdvisoryModelConfiguration {
                model: "test".into(),
            },
            destination: "test-target".into(),
            wire_version: "test-wire/1".into(),
            body_length: 2,
            body_sha256: "digest".into(),
        };
        assert_eq!(budget.authorize(&request).await, Ok(None));
    }
}
