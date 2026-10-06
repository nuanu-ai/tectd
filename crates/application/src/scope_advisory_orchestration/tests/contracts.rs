use super::*;

#[test]
fn dispatch_stale_errors_map_only_to_their_audited_terminal_reasons() {
    assert_eq!(
        prepared_scope_stale_reason(&Error::StaleRevision),
        Some(AdvisoryReason::ConfigurationChanged)
    );
    assert_eq!(
        prepared_scope_stale_reason(&Error::StaleContext),
        Some(AdvisoryReason::DeterministicInputInvalid)
    );
    assert_eq!(prepared_scope_stale_reason(&Error::InputConflict), None);
    assert_eq!(prepared_scope_stale_reason(&Error::NotFound), None);

    let candidate_set_id = Uuid::from_u128(5);
    let manifest = authored_manifest(authored_source(candidate_set_id, 7));
    let config = no_call_config(WorkspaceAdvisoryMode::Optional);
    let request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id,
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    let prepared = opportunity_for_authored_manifest(&request, &config, &manifest);
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::Prepared,
            primary_reason: AdvisoryReason::DispatchAuthorized,
            ..prepared.clone()
        }),
        Err(Error::InputConflict)
    );
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::Invalidated,
            primary_reason: AdvisoryReason::ConfigurationChanged,
            ..prepared.clone()
        })
        .unwrap()
        .state,
        AdvisoryOpportunityState::Invalidated
    );
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::NoCall,
            primary_reason: AdvisoryReason::DeterministicInputInvalid,
            ..prepared.clone()
        })
        .unwrap()
        .primary_reason,
        AdvisoryReason::DeterministicInputInvalid
    );
    assert_eq!(
        validate_terminalized_pre_dispatch_opportunity(AdvisoryOpportunity {
            state: AdvisoryOpportunityState::NoCall,
            primary_reason: AdvisoryReason::DeterministicInputInvalid,
            provider_called: true,
            ..prepared
        }),
        Err(Error::InputConflict)
    );
}

#[test]
fn authored_no_call_replay_conflicts_on_changed_request_payload() {
    let config = no_call_config(WorkspaceAdvisoryMode::Optional);
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(6),
        candidate_set_id: Uuid::from_u128(5),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_set()),
    };
    let reason = AdvisoryReason::CapabilityUnavailable;
    let opportunity = AdvisoryOpportunity {
        id: Uuid::from_u128(40),
        workspace_id: config.workspace_id,
        session_id: Uuid::from_u128(41),
        authorized_actor_id: Uuid::from_u128(42),
        capability: AdvisoryCapability::ScopeDecomposition,
        decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
        decision_point_version: 1,
        workflow_occurrence_key: request.request_id.to_string(),
        target_kind: "scope_candidate_set".into(),
        target_id: Some(request.candidate_set_id),
        work_revision: Some(7),
        matrix_task_revision: None,
        matrix_choice_set_digest: None,
        matrix_verification_digest: None,
        source_ref: None,
        session_preference: request.session_preference,
        request_preference: request.request_preference,
        config_revision: config.revision,
        material_digest: no_call_digest(&request, config.revision, reason).unwrap(),
        state: AdvisoryOpportunityState::NoCall,
        primary_reason: reason,
        provider_called: false,
    };
    assert!(
        validate_authored_no_call_replay(
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
        )
        .is_ok()
    );
    request.authored_scope_set.as_mut().unwrap().alternatives[0]
        .draft
        .empty_disposition
        .as_mut()
        .unwrap()
        .reason
        .push('!');
    assert_eq!(
        validate_authored_no_call_replay(
            Some(&opportunity),
            &request,
            &config,
            Uuid::from_u128(42),
            Uuid::from_u128(41),
        ),
        Err(Error::InputConflict)
    );
}

#[test]
fn authored_contract_rejects_missing_baseline_duplicate_keys_and_unsorted_coverage() {
    let mut authored = authored_set();
    authored.validate().unwrap();
    authored.baseline_key = "missing".into();
    assert_eq!(authored.validate(), Err(Error::InvalidArguments));
    authored.baseline_key = "baseline".into();
    authored.alternatives.push(authored.alternatives[0].clone());
    assert_eq!(authored.validate(), Err(Error::InvalidArguments));
    authored.alternatives.pop();
    authored.alternatives[0].covered_source_ref_ids.reverse();
    assert_eq!(authored.validate(), Err(Error::InvalidArguments));
}

#[test]
fn authored_request_digest_changes_no_call_material_and_rejects_unknown_fields() {
    let mut request = RunScopeAdvisory {
        request_id: Uuid::from_u128(1),
        candidate_set_id: Uuid::from_u128(3),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::Skip,
        authored_scope_set: None,
    };
    let absent = no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap();
    request.authored_scope_set = Some(authored_set());
    let first = no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap();
    assert_ne!(absent, first);
    assert_eq!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap()
    );
    request.authored_scope_set.as_mut().unwrap().alternatives[0]
        .draft
        .empty_disposition
        .as_mut()
        .unwrap()
        .reason
        .push('!');
    assert_ne!(
        first,
        no_call_digest(&request, 3, AdvisoryReason::RequestSkip).unwrap()
    );

    let mut value = serde_json::to_value(authored_set()).unwrap();
    value["unexpected"] = serde_json::json!(true);
    assert!(serde_json::from_value::<AuthoredScopeSet>(value).is_err());
}

#[tokio::test]
async fn production_defaults_fail_closed_without_supplier_budget_or_provider() {
    assert!(crate::ScopeManifestSupplier::identity(&UnavailableScopeManifestSupplier).is_none());
    let budget = DenyScopeBudget;
    assert_eq!(
        budget
            .evaluate(
                &ScopeBudgetRequest {
                    workspace_id: Uuid::from_u128(1),
                    actor_id: Uuid::from_u128(2),
                    candidate_set_id: Uuid::from_u128(3),
                    config_revision: 0,
                    manifest_digest: "a".repeat(64),
                    body_length: 1,
                    body_sha256: "b".repeat(64),
                },
                &super::budget::syntactic_policy(1024)
            )
            .await
            .unwrap(),
        None
    );
    assert!(crate::ScopeAdviceProvider::identity(&DisabledScopeAdviceProvider).is_none());
    let source = ORCHESTRATION_SOURCE;
    let budget = source.find(".scope_budget").unwrap();
    let no_call = budget
        + source[budget..]
            .find("AdvisoryReason::BudgetPolicyInvalid")
            .unwrap();
    let provider = source.find("prepare_scope_advice_attempt(").unwrap();
    assert!(provider < budget && budget < no_call);
}
