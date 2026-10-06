use crate::{TransactionMode, UnitOfWork, WorkspaceService};
use tect_domain::{
    Error, PrincipalRole, RequestContext, Result, SelectedSaveObservation,
    SelectedSaveObservationRequest, Session, Workspace,
};
use uuid::Uuid;

/// Public verifier input. Identity and session are bound to the host credential.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifySelectedSave {
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub candidate_set_id: Uuid,
    pub caller_link_id: Uuid,
    pub caller_receipt_request_id: Uuid,
    pub target_revision: i64,
}

impl WorkspaceService {
    async fn verifier_candidate_transaction(
        &self,
        context: &RequestContext,
    ) -> Result<(Box<dyn UnitOfWork>, Workspace, Session)> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        // Serialize observation with native-session revocation before reading its binding.
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        Ok((tx, workspace, session))
    }

    /// Authenticate malformed verification input with the same trusted verifier
    /// role, session lock, and binding checks as a valid verification request.
    pub async fn authenticate_candidate_advisory_verifier_session(
        &self,
        context: &RequestContext,
    ) -> Result<()> {
        let (tx, _, _) = self.verifier_candidate_transaction(context).await?;
        tx.commit().await
    }

    pub async fn verify_selected_save(
        &self,
        context: &RequestContext,
        request: &VerifySelectedSave,
    ) -> Result<SelectedSaveObservation> {
        let (mut tx, workspace, session) = self.verifier_candidate_transaction(context).await?;
        let bound = request.bind_session(session.id)?;
        let observed = tx
            .independently_observe_selected_scope_save(workspace.id, &bound)
            .await?;
        tx.commit().await?;
        Ok(observed)
    }
}

impl VerifySelectedSave {
    fn bind_session(&self, session_id: Uuid) -> Result<SelectedSaveObservationRequest> {
        let bound = SelectedSaveObservationRequest {
            request_id: self.request_id,
            opportunity_id: self.opportunity_id,
            candidate_set_id: self.candidate_set_id,
            caller_link_id: self.caller_link_id,
            caller_receipt_request_id: self.caller_receipt_request_id,
            target_revision: self.target_revision,
            session_id,
        };
        if !bound.valid() {
            return Err(Error::InvalidArguments);
        }
        Ok(bound)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request() -> VerifySelectedSave {
        VerifySelectedSave {
            request_id: Uuid::from_u128(1),
            opportunity_id: Uuid::from_u128(2),
            candidate_set_id: Uuid::from_u128(3),
            caller_link_id: Uuid::from_u128(4),
            caller_receipt_request_id: Uuid::from_u128(5),
            target_revision: 1,
        }
    }

    #[test]
    fn observation_binds_only_the_authenticated_session() {
        let request = request();
        let session_id = Uuid::from_u128(6);
        assert_eq!(
            request.bind_session(session_id).unwrap(),
            SelectedSaveObservationRequest {
                request_id: request.request_id,
                opportunity_id: request.opportunity_id,
                candidate_set_id: request.candidate_set_id,
                caller_link_id: request.caller_link_id,
                caller_receipt_request_id: request.caller_receipt_request_id,
                target_revision: request.target_revision,
                session_id,
            }
        );
    }

    #[test]
    fn malformed_observation_ids_and_revision_are_rejected() {
        let mut cases = Vec::new();
        for index in 0..5 {
            let mut invalid = request();
            match index {
                0 => invalid.request_id = Uuid::nil(),
                1 => invalid.opportunity_id = Uuid::nil(),
                2 => invalid.candidate_set_id = Uuid::nil(),
                3 => invalid.caller_link_id = Uuid::nil(),
                _ => invalid.caller_receipt_request_id = Uuid::nil(),
            }
            cases.push(invalid);
        }
        for revision in [0, -1] {
            let mut invalid = request();
            invalid.target_revision = revision;
            cases.push(invalid);
        }
        for invalid in cases {
            assert_eq!(
                invalid.bind_session(Uuid::from_u128(6)),
                Err(Error::InvalidArguments)
            );
        }
        assert_eq!(
            request().bind_session(Uuid::nil()),
            Err(Error::InvalidArguments)
        );
    }
}
