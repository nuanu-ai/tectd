use super::*;

struct DurablePreference {
    preference: AdvisoryRequestPreference,
    reads: usize,
    denied: bool,
}
#[async_trait]
impl SessionPreferenceReader for DurablePreference {
    async fn preference(
        &mut self,
        _: Uuid,
        _: Uuid,
    ) -> Result<tect_domain::SessionAdvisoryPreference> {
        self.reads += 1;
        if self.denied {
            return Err(Error::Forbidden);
        }
        Ok(tect_domain::SessionAdvisoryPreference {
            preference: self.preference,
            revision: 3,
        })
    }
}

#[tokio::test]
async fn new_operation_binds_durable_preference_not_request_and_fail_closed() {
    let mut request = RunScopeAdvisory {
        request_id: Uuid::new_v4(),
        candidate_set_id: Uuid::new_v4(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
    };
    let mut reader = DurablePreference {
        preference: AdvisoryRequestPreference::Skip,
        reads: 0,
        denied: false,
    };
    request.session_preference = bound_session_preference(
        &mut reader,
        Uuid::from_u128(1),
        Uuid::from_u128(2),
        Uuid::from_u128(42),
        None,
    )
    .await
    .unwrap();
    assert_eq!(
        early_no_call_reason(&no_call_config(WorkspaceAdvisoryMode::Optional), &request),
        Some(AdvisoryReason::SessionSkip)
    );
    assert_eq!(reader.reads, 1);
    let capture = scope_opportunity_input(
        &request,
        &no_call_config(WorkspaceAdvisoryMode::Optional),
        ScopeCaptureIdentity {
            actor: Uuid::from_u128(42),
            session: Uuid::from_u128(2),
            material_digest: no_call_digest(&request, 3, AdvisoryReason::SessionSkip).unwrap(),
        },
        ScopeCaptureStatus {
            state: AdvisoryOpportunityState::NoCall,
            reason: AdvisoryReason::SessionSkip,
        },
        Some(7),
    );
    assert_eq!(capture.session_preference, AdvisoryRequestPreference::Skip);
    assert_eq!(capture.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(capture.validate(), Ok(()));
    reader.denied = true;
    assert_eq!(
        bound_session_preference(
            &mut reader,
            Uuid::from_u128(1),
            Uuid::from_u128(2),
            Uuid::from_u128(42),
            None
        )
        .await,
        Err(Error::Forbidden)
    );
}

#[tokio::test]
async fn exact_key_replay_uses_captured_preference_without_durable_read() {
    let request = RunScopeAdvisory {
        request_id: Uuid::new_v4(),
        candidate_set_id: Uuid::from_u128(9),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
    };
    let manifest = authored_manifest(authored_source(request.candidate_set_id, 7));
    let mut saved = opportunity_for_authored_manifest(
        &request,
        &no_call_config(WorkspaceAdvisoryMode::Optional),
        &manifest,
    );
    let mut reader = DurablePreference {
        preference: AdvisoryRequestPreference::Skip,
        reads: 0,
        denied: true,
    };
    assert_eq!(
        bound_session_preference(
            &mut reader,
            Uuid::from_u128(1),
            saved.session_id,
            saved.authorized_actor_id,
            Some(&saved)
        )
        .await,
        Ok(AdvisoryRequestPreference::UseWorkspace)
    );
    saved.session_preference = AdvisoryRequestPreference::Skip;
    reader.preference = AdvisoryRequestPreference::UseWorkspace;
    assert_eq!(
        bound_session_preference(
            &mut reader,
            Uuid::from_u128(1),
            saved.session_id,
            saved.authorized_actor_id,
            Some(&saved)
        )
        .await,
        Ok(AdvisoryRequestPreference::Skip)
    );
    assert_eq!(
        bound_session_preference(
            &mut reader,
            saved.workspace_id,
            Uuid::new_v4(),
            saved.authorized_actor_id,
            Some(&saved)
        )
        .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        bound_session_preference(
            &mut reader,
            Uuid::new_v4(),
            saved.session_id,
            saved.authorized_actor_id,
            Some(&saved)
        )
        .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        bound_session_preference(
            &mut reader,
            saved.workspace_id,
            saved.session_id,
            Uuid::new_v4(),
            Some(&saved)
        )
        .await,
        Err(Error::Forbidden)
    );
    assert_eq!(reader.reads, 0);
}

#[test]
fn durable_binding_and_skip_commit_precede_new_authorization() {
    // Source ordering guard only, not a concurrency/runtime proof.
    let run = include_str!("../../scope_advisory_orchestration.rs");
    let bind = run
        .find("bound_request.session_preference = bound_session_preference")
        .unwrap();
    assert!(bind < run.find(".scope_receipt_for_replay(").unwrap());
    assert!(bind < run.find("early_no_call_target(").unwrap());
    let dispatch = include_str!("../dispatch.rs");
    let skip = dispatch
        .find("reason: AdvisoryReason::SessionSkip")
        .unwrap();
    let authorize = dispatch.find(".authorize_advisory_dispatch(").unwrap();
    assert!(skip < authorize);
    assert!(dispatch[skip..authorize].contains("authorize.commit().await?"));
    assert!(dispatch[skip..authorize].contains("return Ok(ScopeAdvisoryOutcome"));
    let recovery = include_str!("../recovery.rs");
    assert!(recovery.contains("tx.lock_native_session(identity.host_id"));
    let shared = include_str!("../../advisory.rs");
    assert!(shared.contains("tx.lock_native_session(identity.host_id"));
}

#[test]
fn prepared_session_skip_terminal_requires_no_call_and_no_provider_call() {
    let request = RunScopeAdvisory {
        request_id: Uuid::new_v4(),
        candidate_set_id: Uuid::from_u128(9),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: None,
    };
    let manifest = authored_manifest(authored_source(request.candidate_set_id, 7));
    let mut opportunity = opportunity_for_authored_manifest(
        &request,
        &no_call_config(WorkspaceAdvisoryMode::Optional),
        &manifest,
    );
    opportunity.state = AdvisoryOpportunityState::NoCall;
    opportunity.primary_reason = AdvisoryReason::SessionSkip;
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(opportunity.clone()),
        Ok(opportunity.clone())
    );
    opportunity.provider_called = true;
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(opportunity.clone()),
        Err(Error::InputConflict)
    );
    opportunity.provider_called = false;
    opportunity.primary_reason = AdvisoryReason::RequestSkip;
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(opportunity.clone()),
        Err(Error::InputConflict)
    );
    opportunity.state = AdvisoryOpportunityState::Prepared;
    opportunity.primary_reason = AdvisoryReason::SessionSkip;
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(opportunity),
        Err(Error::InputConflict)
    );
}
