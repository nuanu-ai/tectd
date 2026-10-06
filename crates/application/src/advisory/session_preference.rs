use super::*;
use tect_domain::{Error, RequestContext, SessionAdvisoryPreference, SetSessionAdvisoryPreference};

use async_trait::async_trait;

#[async_trait]
trait PreferenceSessionBinding: Send {
    async fn session(
        &mut self,
        host_id: uuid::Uuid,
        native_id: &str,
    ) -> Result<Option<tect_domain::Session>>;
    async fn workspace(&mut self, id: uuid::Uuid) -> Result<Option<tect_domain::Workspace>>;
    async fn is_member(
        &mut self,
        workspace_id: uuid::Uuid,
        principal_id: uuid::Uuid,
    ) -> Result<bool>;
    async fn session_principal(&mut self, session_id: uuid::Uuid) -> Result<uuid::Uuid>;
}

#[async_trait]
impl PreferenceSessionBinding for dyn UnitOfWork + '_ {
    async fn session(
        &mut self,
        host_id: uuid::Uuid,
        native_id: &str,
    ) -> Result<Option<tect_domain::Session>> {
        UnitOfWork::session(self, host_id, native_id).await
    }
    async fn workspace(&mut self, id: uuid::Uuid) -> Result<Option<tect_domain::Workspace>> {
        UnitOfWork::workspace(self, id).await
    }
    async fn is_member(
        &mut self,
        workspace_id: uuid::Uuid,
        principal_id: uuid::Uuid,
    ) -> Result<bool> {
        UnitOfWork::is_member(self, workspace_id, principal_id).await
    }
    async fn session_principal(&mut self, session_id: uuid::Uuid) -> Result<uuid::Uuid> {
        UnitOfWork::session_principal(self, session_id).await
    }
}

// Preference is an own-session narrowing control, not Owner configuration authority.
async fn bound_preference_session<T: PreferenceSessionBinding + ?Sized>(
    tx: &mut T,
    context: &RequestContext,
    identity: &tect_domain::HostIdentity,
) -> Result<(tect_domain::Workspace, tect_domain::Session)> {
    let session = tx
        .session(identity.host_id, &context.native_session_id)
        .await?
        .ok_or(Error::WorkspaceNotOpen)?;
    if session.host_id != identity.host_id || session.native_session_id != context.native_session_id
    {
        return Err(Error::Forbidden);
    }
    if session.revoked {
        return Err(Error::SessionRevoked);
    }
    let workspace = tx
        .workspace(session.workspace_id)
        .await?
        .ok_or(Error::Forbidden)?;
    if workspace.id != session.workspace_id || workspace.key != context.workspace_key {
        return Err(Error::SessionWorkspaceMismatch);
    }
    if !tx.is_member(workspace.id, identity.principal_id).await?
        || tx.session_principal(session.id).await? != identity.principal_id
    {
        return Err(Error::Forbidden);
    }
    Ok((workspace, session))
}

impl WorkspaceService {
    async fn session_preference_transaction(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(
        Box<dyn UnitOfWork>,
        tect_domain::Workspace,
        tect_domain::Session,
    )> {
        let (mut tx, identity) = self.authenticated(context, mode).await?;
        if mode == TransactionMode::ReadWrite {
            tx.lock_native_session(identity.host_id, &context.native_session_id)
                .await?;
        }
        let (workspace, session) = bound_preference_session(&mut *tx, context, &identity).await?;
        Ok((tx, workspace, session))
    }

    pub async fn session_advisory_preference(
        &self,
        context: &RequestContext,
    ) -> Result<SessionAdvisoryPreference> {
        let (mut tx, workspace, session) = self
            .session_preference_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let preference = tx
            .session_advisory_preference(workspace.id, session.id)
            .await?;
        tx.commit().await?;
        Ok(preference)
    }

    pub async fn set_session_advisory_preference(
        &self,
        context: &RequestContext,
        request: &SetSessionAdvisoryPreference,
    ) -> Result<SessionAdvisoryPreference> {
        request.validate()?;
        // The bound native session and this lock are also used by Scope send authorization.
        let (mut tx, workspace, session) = self
            .session_preference_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let preference = tx
            .set_session_advisory_preference(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(preference)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        AdvisoryRequestPreference, HostAuth, HostIdentity, PrincipalRole, Session, Workspace,
    };
    use uuid::Uuid;

    struct Binding {
        session: Option<Session>,
        workspace: Workspace,
        principal: Uuid,
        member: bool,
        preference: SessionAdvisoryPreference,
        accesses: usize,
    }

    #[async_trait]
    impl PreferenceSessionBinding for Binding {
        async fn session(&mut self, _host: Uuid, _native: &str) -> Result<Option<Session>> {
            // Deliberately return the row even for a mismatched query: the helper must fence it.
            Ok(self.session.clone())
        }
        async fn workspace(&mut self, _id: Uuid) -> Result<Option<Workspace>> {
            Ok(Some(self.workspace.clone()))
        }
        async fn is_member(&mut self, _workspace: Uuid, _principal: Uuid) -> Result<bool> {
            Ok(self.member)
        }
        async fn session_principal(&mut self, _session: Uuid) -> Result<Uuid> {
            Ok(self.principal)
        }
    }

    impl Binding {
        async fn access(
            &mut self,
            context: &RequestContext,
            identity: &HostIdentity,
            request: Option<&SetSessionAdvisoryPreference>,
        ) -> Result<SessionAdvisoryPreference> {
            // Exercise the production binding adapter before any test preference get/set.
            let (workspace, session) = bound_preference_session(self, context, identity).await?;
            assert_eq!(workspace.id, self.workspace.id);
            assert_eq!(session.id, self.session.as_ref().unwrap().id);
            self.accesses += 1;
            if let Some(request) = request {
                request.validate()?;
                if self.preference.revision != request.expected_revision {
                    return Err(Error::InputConflict);
                }
                self.preference = SessionAdvisoryPreference {
                    preference: request.preference,
                    revision: self
                        .preference
                        .revision
                        .checked_add(1)
                        .ok_or(Error::InvalidArguments)?,
                };
            }
            Ok(self.preference)
        }
    }

    fn fixture() -> (Binding, RequestContext, HostIdentity) {
        let identity = HostIdentity {
            host_id: Uuid::new_v4(),
            tenant_id: Uuid::new_v4(),
            principal_id: Uuid::new_v4(),
            role: PrincipalRole::Verifier,
            allowed_source_roots: vec![],
            allowed_setup_roots: vec![],
        };
        let workspace = Workspace {
            id: Uuid::new_v4(),
            key: "own-preference".into(),
        };
        let context = RequestContext {
            auth: HostAuth {
                host_id: identity.host_id,
                credential: "a".repeat(64),
            },
            native_session_id: Uuid::new_v4().to_string(),
            workspace_key: workspace.key.clone(),
        };
        let binding = Binding {
            session: Some(Session {
                id: Uuid::new_v4(),
                workspace_id: workspace.id,
                host_id: identity.host_id,
                native_session_id: context.native_session_id.clone(),
                revoked: false,
            }),
            workspace,
            principal: identity.principal_id,
            member: true,
            preference: SessionAdvisoryPreference {
                preference: AdvisoryRequestPreference::UseWorkspace,
                revision: 0,
            },
            accesses: 0,
        };
        (binding, context, identity)
    }

    #[tokio::test]
    async fn verifier_own_binding_allows_preference_get_and_set_adapter_calls() {
        let (mut binding, context, identity) = fixture();
        assert_eq!(
            binding
                .access(&context, &identity, None)
                .await
                .unwrap()
                .revision,
            0
        );
        let request = SetSessionAdvisoryPreference {
            expected_revision: 0,
            preference: AdvisoryRequestPreference::Skip,
        };
        let saved = binding
            .access(&context, &identity, Some(&request))
            .await
            .unwrap();
        assert_eq!(saved.preference, AdvisoryRequestPreference::Skip);
        assert_eq!(saved.revision, 1);
        assert_eq!(binding.accesses, 2);
    }

    #[tokio::test]
    async fn preference_binding_rejects_foreign_session_actor_workspace_and_revocation_before_access()
     {
        for case in 0..8 {
            let (mut binding, mut context, identity) = fixture();
            let expected = match case {
                0 => {
                    context.native_session_id = Uuid::new_v4().to_string();
                    Error::Forbidden
                }
                1 => {
                    binding.session.as_mut().unwrap().host_id = Uuid::new_v4();
                    Error::Forbidden
                }
                2 => {
                    binding.principal = Uuid::new_v4();
                    Error::Forbidden
                }
                3 => {
                    context.workspace_key = "foreign-workspace".into();
                    Error::SessionWorkspaceMismatch
                }
                4 => {
                    binding.workspace.id = Uuid::new_v4();
                    Error::SessionWorkspaceMismatch
                }
                5 => {
                    binding.session.as_mut().unwrap().revoked = true;
                    Error::SessionRevoked
                }
                6 => {
                    binding.member = false;
                    Error::Forbidden
                }
                _ => {
                    binding.session = None;
                    Error::WorkspaceNotOpen
                }
            };
            let request = SetSessionAdvisoryPreference {
                expected_revision: 0,
                preference: AdvisoryRequestPreference::Skip,
            };
            assert_eq!(
                binding.access(&context, &identity, None).await,
                Err(expected.clone())
            );
            assert_eq!(
                binding.access(&context, &identity, Some(&request)).await,
                Err(expected)
            );
            assert_eq!(binding.accesses, 0);
            assert_eq!(binding.preference.revision, 0);
        }
    }
}
