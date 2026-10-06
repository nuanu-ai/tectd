use super::*;

#[tokio::test]
async fn early_no_call_uses_only_target_port_and_requires_target_access() {
    let workspace = Uuid::from_u128(1);
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(2),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
    };
    let mut target = FakeEarlyCandidateRevision {
        calls: 0,
        revision: Some(7),
    };
    assert_eq!(
        early_no_call_target(
            &mut target,
            workspace,
            &no_call_config(WorkspaceAdvisoryMode::Optional),
            &request,
        )
        .await,
        Ok(None)
    );
    assert_eq!(target.calls, 0);
    request.request_preference = AdvisoryRequestPreference::Skip;
    assert_eq!(
        early_no_call_target(
            &mut target,
            workspace,
            &no_call_config(WorkspaceAdvisoryMode::Optional),
            &request,
        )
        .await,
        Ok(Some((AdvisoryReason::RequestSkip, 7)))
    );
    assert_eq!(target.calls, 1);
    target.revision = None;
    assert_eq!(
        early_no_call_target(
            &mut target,
            workspace,
            &no_call_config(WorkspaceAdvisoryMode::Disabled),
            &request,
        )
        .await,
        Err(tect_domain::Error::NotFound)
    );
    assert_eq!(target.calls, 2);
}

#[test]
fn early_no_call_gate_preserves_disabled_session_request_precedence() {
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(2),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::Skip,
        request_preference: AdvisoryRequestPreference::Skip,
        authored_scope_set: None,
    };
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Disabled), &request),
        Some(AdvisoryReason::WorkspaceDisabled)
    );
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        Some(AdvisoryReason::SessionSkip)
    );
    request.session_preference = AdvisoryRequestPreference::UseWorkspace;
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        Some(AdvisoryReason::RequestSkip)
    );
    request.request_preference = AdvisoryRequestPreference::UseWorkspace;
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        None
    );
}

#[test]
fn early_no_call_branch_precedes_all_external_advisory_ports() {
    let source = ORCHESTRATION_SOURCE;
    let branch = source.find("if let Some((reason, revision))").unwrap();
    let return_from_branch = source[branch..]
        .find("let authority_request = ScopeAuthorityRequest")
        .unwrap()
        + branch;
    assert!(source[branch..return_from_branch].contains("capture_early_scope_no_call"));
    assert!(source[branch..return_from_branch].contains("return Ok(ScopeAdvisoryOutcome"));
    for port in [
        "scope_authority.observe(&authority_request)",
        "supply_scope_manifest(",
        ".scope_budget",
        ".observe_prepared(\n                &ScopeAdviceProviderRequest",
    ] {
        assert!(return_from_branch < source.find(port).unwrap(), "{port}");
    }
    assert!(
        source[..branch]
            .contains("early_no_call_target(&mut *read, workspace.id, &config, request)")
    );
    let capture = include_str!("../capture.rs");
    let recheck = capture
        .find("pub(super) async fn capture_early_scope_no_call")
        .unwrap();
    let end = capture[recheck..]
        .find("pub(super) async fn capture_scope_opportunity")
        .unwrap()
        + recheck;
    assert!(
        capture[recheck..end]
            .contains("lock_candidate_revision(workspace.id, request.candidate_set_id)")
    );
    assert!(capture[recheck..end].contains("tx.commit().await?"));
}

#[test]
fn no_call_material_is_deterministic_and_binds_reason_and_revision() {
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(1),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
    };
    let first = no_call_digest(&request, 3, AdvisoryReason::CapabilityUnavailable).unwrap();
    assert_eq!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::CapabilityUnavailable).unwrap()
    );
    assert_ne!(
        first,
        no_call_digest(&request, 4, AdvisoryReason::CapabilityUnavailable).unwrap()
    );
    assert_ne!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::ProviderUnconfigured).unwrap()
    );
}
