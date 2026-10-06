use super::*;

#[test]
fn authority_binding_requires_candidate_set_identity() {
    let request = crate::ScopeAuthorityRequest {
        tenant_id: Uuid::from_u128(9),
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id: Uuid::from_u128(5),
    };
    let observation = crate::ScopeAuthorityObservation {
        workspace_id: request.workspace_id,
        actor_id: request.actor_id,
        session_id: request.session_id,
        candidate_set_id: request.candidate_set_id,
        source: source(request.candidate_set_id),
        obligations: Vec::new(),
    };
    assert!(validate_observation(&request, &observation).is_ok());
    let mut wrong_observation = observation.clone();
    wrong_observation.candidate_set_id = Uuid::from_u128(4);
    assert_eq!(
        validate_observation(&request, &wrong_observation),
        Err(tect_domain::Error::InputConflict)
    );
    let mut wrong_candidate = observation;
    wrong_candidate.source.candidate_set_id = Uuid::from_u128(4);
    assert_eq!(
        validate_observation(&request, &wrong_candidate),
        Err(tect_domain::Error::InputConflict)
    );
}

#[tokio::test]
async fn unauthorized_observer_error_has_no_registered_decision_point() {
    let request = crate::ScopeAuthorityRequest {
        tenant_id: Uuid::from_u128(9),
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id: Uuid::from_u128(5),
    };
    assert_eq!(
        UnauthorizedObserver.observe(&request).await,
        Err(tect_domain::Error::Unauthorized)
    );
    let orchestration = ORCHESTRATION_SOURCE;
    assert!(orchestration.contains("scope_authority.observe(&authority_request).await?"));
}

#[test]
fn authorized_invalid_observation_routes_to_durable_invalid_capture_before_policy() {
    let request = crate::ScopeAuthorityRequest {
        tenant_id: Uuid::from_u128(9),
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        session_id: Uuid::from_u128(3),
        candidate_set_id: Uuid::from_u128(5),
    };
    let invalid = ScopeAuthorizedInvalidObservation {
        workspace_id: request.workspace_id,
        actor_id: request.actor_id,
        session_id: request.session_id,
        candidate_set_id: request.candidate_set_id,
    };
    assert!(validate_invalid_observation(&request, &invalid).is_ok());
    let orchestration = ORCHESTRATION_SOURCE;
    let invalid_arm = orchestration.find("AuthorizedInvalid(value)").unwrap();
    let capture = orchestration[invalid_arm..]
        .find("capture_invalid_scope_input")
        .unwrap()
        + invalid_arm;
    let policy = orchestration
        .find("let preliminary = assess_advisory_policy")
        .unwrap();
    assert!(invalid_arm < capture && capture < policy);
    let missing_supplier = orchestration
        .find("scope_manifest_supplier.identity().is_none()")
        .unwrap();
    assert!(missing_supplier < policy);
    assert!(orchestration[missing_supplier..policy].contains("capture_invalid_scope_input"));
    let supplied_manifest = orchestration
        .find("let manifest = match supply_scope_manifest(")
        .unwrap();
    assert!(supplied_manifest < policy);
    assert!(orchestration[supplied_manifest..policy].contains("capture_invalid_scope_input"));
    let capture_source = include_str!("../capture.rs");
    assert!(capture_source.contains("deterministic_input_valid: false"));
    assert!(capture_source.contains("tx.commit().await?"));
}

#[test]
fn replay_precedes_policy_and_provider_and_success_is_rechecked_atomically() {
    let source = ORCHESTRATION_SOURCE;
    let replay = source.find("advisory_opportunity_by_request").unwrap();
    let policy = source.find(".scope_budget").unwrap();
    let provider = source.find(".observe_prepared(").unwrap();
    let sealed = source.find(".seal_committed_advisory_observation").unwrap();
    let reobserved = source[sealed..].find("scope_authority.observe").unwrap() + sealed;
    let finalized = source.find(".finalize_guarded_scope_advice").unwrap();
    assert!(replay < policy && policy < provider);
    assert!(sealed < reobserved && reobserved < finalized);
    assert!(source.contains("invalidate_scope_advisory"));
    assert_eq!(source.matches("capture_invalid_scope_input").count(), 4);
}

#[test]
fn authored_lookup_replay_and_failure_paths_precede_external_attempts() {
    let source = ORCHESTRATION_SOURCE;
    let digest = source.find(".map(authored_request_digest)").unwrap();
    let request_lookup = source
        .find("read.scope_advisory_manifest_by_request_key")
        .unwrap();
    let replay = source.find("replay_authored_scope_advisory(").unwrap();
    let observer = source
        .find("self.scope_authority.observe(&authority_request)")
        .unwrap();
    let supplier = source.find("supply_scope_manifest(").unwrap();
    assert!(digest < request_lookup && request_lookup < replay);
    let provider = source.find(".observe_prepared(").unwrap();
    assert!(replay < observer && observer < supplier && replay < provider);
    let early_no_call = source.find("if let Some((reason, revision))").unwrap();
    let active_input_gate = source
        .find("if request.authored_scope_set.is_none()")
        .unwrap();
    assert!(early_no_call < active_input_gate && active_input_gate < observer);
    assert!(source[active_input_gate..observer].contains("return Err(Error::InputPending)"));

    let authored_persist = source
        .find(".prepare_authored_scope_advisory_manifest(")
        .unwrap();
    let persisted_commit = source[authored_persist..]
        .find("prepare.commit().await?")
        .unwrap()
        + authored_persist;
    let reobserved = source[persisted_commit..]
        .find("self.scope_authority.observe(&authority_request)")
        .unwrap()
        + persisted_commit;
    let no_call_transition = source[reobserved..]
        .find("finalize_prepared_scope_advisory_without_dispatch")
        .unwrap()
        + reobserved;
    let provider = source.find(".observe_prepared(").unwrap();
    assert!(authored_persist < persisted_commit);
    assert!(persisted_commit < reobserved);
    assert!(reobserved < no_call_transition && no_call_transition < provider);
}

#[test]
fn authorize_staleness_and_cancelled_start_terminalize_before_provider_attempt() {
    let source = ORCHESTRATION_SOURCE;
    let authorize = source
        .find(".authorize_advisory_dispatch(&lifecycle")
        .unwrap();
    let stale_mapping = source[authorize..]
        .find("prepared_scope_stale_reason(&error)")
        .unwrap()
        + authorize;
    let rollback = source[stale_mapping..].find("drop(authorize)").unwrap() + stale_mapping;
    let close = source[rollback..]
        .find(".finalize_prepared_scope_stale(")
        .unwrap()
        + rollback;
    let start = source
        .find(".start_signed_scope_dispatch(&lifecycle")
        .unwrap();
    let cancelled = source[start..]
        .find("started.dispatch.state == AdvisoryDispatchState::Cancelled")
        .unwrap()
        + start;
    let terminal_load = source[cancelled..]
        .find("advisory_opportunity_for_dispatch(workspace.id, opportunity.id)")
        .unwrap()
        + cancelled;
    let provider = source.find(".observe_prepared(").unwrap();
    assert!(authorize < stale_mapping && stale_mapping < rollback && rollback < close);
    assert!(close < start && start < cancelled && cancelled < terminal_load);
    assert!(terminal_load < provider);

    let helper = include_str!("../helpers.rs");
    assert!(helper.contains("Error::StaleRevision => Some(AdvisoryReason::ConfigurationChanged)"));
    assert!(
        helper.contains("Error::StaleContext => Some(AdvisoryReason::DeterministicInputInvalid)")
    );
    assert!(helper.contains("if !expected || opportunity.provider_called"));
    assert!(source.contains("started.dispatch.opportunity_id == opportunity.id"));
    assert!(source.contains("started.dispatch.outcome.is_none()"));
    assert!(source.contains("started.dispatch.raw_response_ref.is_none()"));
}

#[test]
fn authored_supplier_failure_is_captured_as_no_call_before_budget_or_provider() {
    let source = ORCHESTRATION_SOURCE;
    let supplied = source
        .find("let manifest = match supply_scope_manifest(")
        .unwrap();
    let failure = source[supplied..].find("Err(_) =>").unwrap() + supplied;
    let capture = source[failure..]
        .find("capture_invalid_scope_input")
        .unwrap()
        + failure;
    let budget = source.find(".scope_budget").unwrap();
    let provider = source.find(".observe_prepared(").unwrap();
    assert!(supplied < failure && failure < capture);
    assert!(capture < budget && budget < provider);
}

#[test]
fn authored_request_entry_and_explicit_adapter_constructor_are_public_without_effects() {
    let root = include_str!("../../lib.rs");
    let service = include_str!("../../service.rs");
    let orchestration = ORCHESTRATION_SOURCE;
    assert!(root.contains("ScopeAdviceProviderRequest"));
    assert!(!root.contains("pub use scope_advisory_runtime::*;"));
    assert!(service.contains("pub fn new_with_scope_advisory_adapters("));
    assert!(orchestration.contains("pub async fn run_scope_advisory"));
    assert!(root.contains("RunScopeAdvisory, ScopeAdvisoryOutcome"));
    assert!(!orchestration.contains("pub async fn decide_scope_advisory"));
    assert!(!orchestration.contains("pub async fn preserve_scope_advisory"));
}
