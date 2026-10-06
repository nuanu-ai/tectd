fn require_owner(role: PrincipalRole) -> Result<()> {
    if role != PrincipalRole::Owner {
        return Err(Error::Forbidden);
    }
    Ok(())
}

fn verifier_opened_state(workspace: Workspace, session: Session) -> WorkspaceState {
    let mut state = WorkspaceState::opened(workspace, session);
    state.next_action = None;
    state
}

#[cfg(test)]
mod auth_role_tests {
    use super::*;

    #[test]
    fn owner_authorization_rejects_verifier_role() {
        assert_eq!(require_owner(PrincipalRole::Owner), Ok(()));
        assert_eq!(
            require_owner(PrincipalRole::Verifier),
            Err(Error::Forbidden)
        );
    }

    #[test]
    fn verifier_opening_exposes_only_workspace_and_session() {
        let workspace = Workspace {
            id: uuid::Uuid::new_v4(),
            key: "verifier-membership".into(),
        };
        let session = Session {
            id: uuid::Uuid::new_v4(),
            workspace_id: workspace.id,
            host_id: uuid::Uuid::new_v4(),
            native_session_id: uuid::Uuid::new_v4().to_string(),
            revoked: false,
        };
        let state = verifier_opened_state(workspace.clone(), session.clone());
        assert_eq!(state.workspace, Some(workspace));
        assert_eq!(state.session, Some(session));
        assert_eq!(state.next_action, None);
        assert!(state.selected_worktrees.is_empty());
        assert!(state.programs.is_empty());
        assert!(state.candidate_sets.is_empty());
        assert!(state.native_planning.is_empty());
        assert_eq!(state.setup_context, None);
        assert_eq!(state.next_after, None);
    }
}
