use super::*;

#[tokio::test]
async fn owner_reported_matrix_never_prepares_even_with_budget() {
    let mut revision = stored_revision();
    revision.input.mode = known(EngineeringMode::Demo);
    revision.input_digest =
        canonical_matrix_input_digest(&serde_json::to_value(&revision.input).unwrap()).unwrap();
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
    let request = advisory_request(revision.task_id);
    let mut config = advisory_config(WorkspaceAdvisoryMode::Optional);
    let identity = MatrixProviderIdentity {
        provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
            id: "test-profile".into(),
        },
        model_configuration: tect_domain::AdvisoryModelConfiguration {
            model: "test-model".into(),
        },
        destination: "test-destination".into(),
        wire_version: "test-wire/1".into(),
    };
    config.provider_profile_ref = Some(identity.provider_profile_ref.clone());
    config.model_configuration = Some(identity.model_configuration.clone());
    let mut input = matrix_advisory_opportunity_input(
        &revision,
        &request,
        &config,
        Uuid::new_v4(),
        Uuid::new_v4(),
    )
    .unwrap();
    assert_eq!(
        input.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
    );
    let legacy_no_call_digest = input.material_digest.clone();
    let expected_evaluation_digest = tect_domain::matrix_evaluation_digest(
        &revision.input,
        &compose_current_revision(revision.clone(), revision.revision).unwrap(),
        revision.choice_set.as_ref().unwrap(),
    )
    .unwrap()
    .unwrap();
    let provider_request = MatrixProviderRequest::new(
        revision.clone(),
        compose_current_revision(revision.clone(), revision.revision).unwrap(),
        identity.provider_profile_ref.clone(),
        identity.model_configuration.clone(),
    )
    .unwrap();
    let mut response = MatrixProviderResponse {
        binding: provider_request.binding().clone(),
        provider_profile_ref: identity.provider_profile_ref.clone(),
        model_configuration: identity.model_configuration.clone(),
        raw_response_payload: b"opaque response".to_vec(),
        response_payload_sha256: format!("{:x}", Sha256::digest(b"opaque response")),
        ranking: tect_domain::MatrixRanking::Ranked {
            ranked_candidate_ids: vec!["a".into(), "b".into()],
            recommended_candidate_id: "a".into(),
        },
        input_tokens: None,
        output_tokens: None,
    };
    assert_eq!(response.validate_for(&provider_request), Ok(()));
    let dispatch_id = Uuid::new_v4();
    let opportunity_id = Uuid::new_v4();
    let (ranked_seal, ranked) = crate::matrix_advisory_dispatch::seal_matrix_provider_observation(
        opportunity_id,
        dispatch_id,
        &provider_request,
        Ok(response.clone()),
    );
    assert_eq!(
        ranked_seal.outcome,
        tect_domain::AdvisoryDispatchOutcome::ProviderResponse
    );
    assert_eq!(
        ranked_seal.response_payload.as_deref(),
        Some(b"opaque response".as_slice())
    );
    assert!(matches!(
        ranked.unwrap().outcome,
        crate::GuardedMatrixAdviceOutcome::Ranked { .. }
    ));
    let mut abstained = response.clone();
    abstained.ranking = tect_domain::MatrixRanking::Abstained {
        ranked_candidate_ids: vec![],
        recommended_candidate_id: None,
    };
    let (abstained_seal, abstained_record) =
        crate::matrix_advisory_dispatch::seal_matrix_provider_observation(
            opportunity_id,
            dispatch_id,
            &provider_request,
            Ok(abstained),
        );
    assert_eq!(
        abstained_seal.outcome,
        tect_domain::AdvisoryDispatchOutcome::ProviderResponse
    );
    assert!(matches!(
        abstained_record.unwrap().outcome,
        crate::GuardedMatrixAdviceOutcome::Abstained { .. }
    ));
    response.raw_response_payload.push(b'!');
    assert_eq!(
        response.validate_for(&provider_request),
        Err(Error::InvalidArguments)
    );
    let (malformed_seal, malformed_record) =
        crate::matrix_advisory_dispatch::seal_matrix_provider_observation(
            opportunity_id,
            dispatch_id,
            &provider_request,
            Ok(response),
        );
    assert_eq!(
        malformed_seal.outcome,
        tect_domain::AdvisoryDispatchOutcome::ProviderFailure
    );
    assert_eq!(
        malformed_seal.send_certainty,
        tect_domain::AdvisorySendCertainty::Sent
    );
    assert_eq!(
        malformed_seal.response_payload.as_deref(),
        Some(b"opaque response!".as_slice())
    );
    assert!(malformed_record.is_none());
    let (uncertain_seal, uncertain_record) =
        crate::matrix_advisory_dispatch::seal_matrix_provider_observation(
            opportunity_id,
            dispatch_id,
            &provider_request,
            Err(Error::TransportUnavailable),
        );
    assert_eq!(
        uncertain_seal.send_certainty,
        tect_domain::AdvisorySendCertainty::SentUnknown
    );
    assert!(uncertain_seal.response_payload.is_none());
    assert!(uncertain_record.is_none());
    let provider = TestProvider(identity);
    let actor_id = input.authorized_actor_id;
    let captured = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
        &mut input,
        None,
        config.workspace_id,
        actor_id,
        &provider,
        &TestBudget(true),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        captured,
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
    ));
    assert_eq!(
        revision.input.mode,
        MatrixFact::Known {
            value: EngineeringMode::Demo,
            provenance: FactProvenance("owner report".into()),
        }
    );
    assert_eq!(input.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        input.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
    );
    assert_eq!(input.material_digest, legacy_no_call_digest);
    assert!(!expected_evaluation_digest.is_empty());
    let mut denied = matrix_advisory_opportunity_input(
        &revision,
        &request,
        &config,
        Uuid::new_v4(),
        Uuid::new_v4(),
    )
    .unwrap();
    let actor_id = denied.authorized_actor_id;
    let denied_capture = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
        &mut denied,
        None,
        config.workspace_id,
        actor_id,
        &provider,
        &TestBudget(false),
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        denied_capture,
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
    ));
    assert_eq!(denied.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        denied.primary_reason,
        AdvisoryReason::MatrixEvidenceUnresolved
    );
    assert_eq!(denied.material_digest, legacy_no_call_digest);
}
