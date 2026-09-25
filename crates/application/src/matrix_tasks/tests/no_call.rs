use super::*;

#[tokio::test]
async fn matrix_no_call_precedence_and_digest_bind_stored_material() {
    let mut revision = stored_revision();
    let mut request = advisory_request(revision.task_id);
    let session = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let mut config = advisory_config(WorkspaceAdvisoryMode::Disabled);
    let disabled =
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor).unwrap();
    assert_eq!(disabled.primary_reason, AdvisoryReason::WorkspaceDisabled);
    config.mode = WorkspaceAdvisoryMode::Optional;
    request.session_preference = AdvisoryRequestPreference::Skip;
    assert_eq!(
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
            .unwrap()
            .primary_reason,
        AdvisoryReason::SessionSkip
    );
    request.session_preference = AdvisoryRequestPreference::UseWorkspace;
    request.request_preference = AdvisoryRequestPreference::Skip;
    assert_eq!(
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
            .unwrap()
            .primary_reason,
        AdvisoryReason::RequestSkip
    );
    request.request_preference = AdvisoryRequestPreference::UseWorkspace;
    let absent =
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor).unwrap();
    assert_eq!(
        absent.primary_reason,
        AdvisoryReason::ChoiceSetNotApplicable
    );
    assert_eq!(absent.matrix_task_revision, Some(2));
    assert_eq!(absent.matrix_choice_set_digest, None);

    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "choice-1".into(),
        version: 1,
        task_id: revision.task_id.to_string(),
        task_revision: "2".into(),
        decision_question: "Which approach?".into(),
        candidates: ["a", "b"]
            .into_iter()
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: id.into(),
                approach: id.into(),
                assumption_fact_ids: vec![],
            })
            .collect(),
    };
    revision.choice_set_digest = Some(choice.canonical_digest(&revision.input).unwrap());
    revision.choice_set = Some(choice);
    let eligible =
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor).unwrap();
    assert_eq!(
        eligible.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
    );
    assert_eq!(eligible.state, AdvisoryOpportunityState::NoCall);
    assert_ne!(eligible.material_digest, absent.material_digest);
    let provider = TestProvider(MatrixProviderIdentity {
        provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
            id: "test-profile".into(),
        },
        model_configuration: tect_domain::AdvisoryModelConfiguration {
            model: "test-model".into(),
        },
        destination: "test-destination".into(),
        wire_version: "test-wire/1".into(),
    });
    let mut all_absent = eligible.clone();
    let result = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
        &mut all_absent,
        None,
        config.workspace_id,
        actor,
        &provider,
        &TestBudget(true),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        result,
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
    ));
    assert_eq!(
        all_absent.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
    );
    revision.input_digest = "changed-input".into();
    assert_ne!(
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
            .unwrap()
            .material_digest,
        eligible.material_digest
    );
    config.revision += 1;
    assert_ne!(
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor)
            .unwrap()
            .material_digest,
        eligible.material_digest
    );
}

#[test]
fn matrix_no_call_replay_binds_actor_session_and_request() {
    let revision = stored_revision();
    let request = advisory_request(revision.task_id);
    let config = advisory_config(WorkspaceAdvisoryMode::Optional);
    let session = Uuid::new_v4();
    let actor = Uuid::new_v4();
    let input =
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor).unwrap();
    let receipt = AdvisoryOpportunity {
        id: Uuid::new_v4(),
        workspace_id: config.workspace_id,
        session_id: session,
        authorized_actor_id: actor,
        capability: input.capability,
        decision_point: input.decision_point,
        decision_point_version: input.decision_point_version,
        workflow_occurrence_key: input.workflow_occurrence_key,
        target_kind: input.target_kind,
        target_id: input.target_id,
        work_revision: input.work_revision,
        matrix_task_revision: input.matrix_task_revision,
        matrix_choice_set_digest: input.matrix_choice_set_digest,
        matrix_verification_digest: input.matrix_verification_digest,
        source_ref: None,
        session_preference: input.session_preference,
        request_preference: input.request_preference,
        config_revision: input.config_revision,
        material_digest: input.material_digest,
        state: input.state,
        primary_reason: input.primary_reason,
        provider_called: false,
    };
    assert!(matrix_advisory_receipt_matches(
        &receipt,
        config.workspace_id,
        request.task_id,
        &request.request_key,
    ));
    assert!(!matrix_advisory_receipt_matches(
        &receipt,
        Uuid::new_v4(),
        request.task_id,
        &request.request_key,
    ));
    assert!(!matrix_advisory_receipt_matches(
        &receipt,
        config.workspace_id,
        Uuid::new_v4(),
        &request.request_key,
    ));
    assert!(!matrix_advisory_receipt_matches(
        &receipt,
        config.workspace_id,
        request.task_id,
        "different-key",
    ));
    let mut wrong_kind = receipt.clone();
    wrong_kind.capability = AdvisoryCapability::ScopeDecomposition;
    assert!(!matrix_advisory_receipt_matches(
        &wrong_kind,
        config.workspace_id,
        request.task_id,
        &request.request_key,
    ));
    let mut wrong_revision = receipt.clone();
    wrong_revision.matrix_task_revision = Some(3);
    assert!(!matrix_advisory_receipt_matches(
        &wrong_revision,
        config.workspace_id,
        request.task_id,
        &request.request_key,
    ));
    assert!(matrix_advisory_replay_matches(
        &receipt, &request, session, actor
    ));
    // A saved receipt is replayable independently of the current task
    // head and workspace configuration, which may have advanced.
    let mut advanced_head = revision.clone();
    advanced_head.revision += 1;
    let mut changed_config = config.clone();
    changed_config.revision += 1;
    assert_ne!(advanced_head.revision, request.expected_task_revision);
    assert_ne!(changed_config.revision, receipt.config_revision);
    assert!(matrix_advisory_replay_matches(
        &receipt, &request, session, actor
    ));
    for state in [
        AdvisoryOpportunityState::Prepared,
        AdvisoryOpportunityState::AwaitingResponse,
        AdvisoryOpportunityState::Advised,
        AdvisoryOpportunityState::Failed,
        AdvisoryOpportunityState::Invalidated,
        AdvisoryOpportunityState::Unresolved,
    ] {
        let mut progressed = receipt.clone();
        progressed.state = state;
        progressed.provider_called = true;
        assert!(matrix_advisory_replay_matches(
            &progressed,
            &request,
            session,
            actor
        ));
    }
    assert!(!matrix_advisory_replay_matches(
        &receipt,
        &request,
        session,
        Uuid::new_v4()
    ));
    assert!(!matrix_advisory_replay_matches(
        &receipt,
        &request,
        Uuid::new_v4(),
        actor
    ));
    let mut changed = request.clone();
    changed.task_id = Uuid::new_v4();
    assert!(!matrix_advisory_replay_matches(
        &receipt, &changed, session, actor
    ));
    changed = request.clone();
    changed.expected_task_revision += 1;
    assert!(!matrix_advisory_replay_matches(
        &receipt, &changed, session, actor
    ));
    changed = request.clone();
    changed.request_preference = AdvisoryRequestPreference::Skip;
    assert!(!matrix_advisory_replay_matches(
        &receipt, &changed, session, actor
    ));
}

#[test]
fn matrix_no_call_digest_matches_legacy_vectors() {
    // These fixed vectors use the pre-dispatch tuple serialized by 3288740.
    // A missing nested evaluation Option changes both historical digests.
    let mut revision = stored_revision();
    revision.task_id = Uuid::parse_str("11111111-1111-4111-8111-111111111111").unwrap();
    let request = advisory_request(revision.task_id);
    let mut config = advisory_config(WorkspaceAdvisoryMode::Optional);
    config.workspace_id = Uuid::parse_str("22222222-2222-4222-8222-222222222222").unwrap();
    let session = Uuid::parse_str("33333333-3333-4333-8333-333333333333").unwrap();
    let actor = Uuid::parse_str("44444444-4444-4444-8444-444444444444").unwrap();

    let absent =
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor).unwrap();
    assert_eq!(absent.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        absent.material_digest,
        "2f4a37aa09e7a02717aa8f5e6209ca9b70827f0af9cedbdc7a43a5ddda963804"
    );

    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "choice-1".into(),
        version: 1,
        task_id: revision.task_id.to_string(),
        task_revision: revision.revision.to_string(),
        decision_question: "Which approach?".into(),
        candidates: ["a", "b"]
            .into_iter()
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: id.into(),
                approach: id.into(),
                assumption_fact_ids: vec![],
            })
            .collect(),
    };
    revision.choice_set_digest = Some(choice.canonical_digest(&revision.input).unwrap());
    revision.choice_set = Some(choice);
    assert!(
        tect_domain::matrix_evaluation_digest(
            &revision.input,
            &compose_current_revision(revision.clone(), revision.revision).unwrap(),
            revision.choice_set.as_ref().unwrap(),
        )
        .unwrap()
        .is_some()
    );
    let eligible =
        matrix_advisory_opportunity_input(&revision, &request, &config, session, actor).unwrap();
    assert_eq!(eligible.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        eligible.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
    );
    assert_eq!(
        eligible.material_digest,
        "801ac7f646b95763fe74bcef09764f14ef8e1d9f96067c73c2390fe3f12a77dd"
    );
}
