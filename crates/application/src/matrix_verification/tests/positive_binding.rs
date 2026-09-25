use super::*;

#[tokio::test]
async fn positive_binding_requires_revalidated_exact_record() {
    use tect_domain::{
        AdvisoryModelConfiguration, AdvisoryProviderProfileRef, EngineeringCandidate,
        EngineeringChoiceSet, MATRIX_CHOICE_SET_SCHEMA,
    };
    let mut revision = revision();
    let choice_set = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "set-1".into(),
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
    revision.choice_set_digest = Some(choice_set.canonical_digest(&revision.input).unwrap());
    revision.choice_set = Some(choice_set);
    let workspace = Uuid::new_v4();
    let mut store = FakeStore::default();
    let record = verify_locked_revision(
        &mut store,
        &FakeValidator { trusted: true },
        MatrixVerificationActor {
            workspace_id: workspace,
            verifier_principal_id: Uuid::new_v4(),
            verifier_session_id: Uuid::new_v4(),
        },
        &revision,
        &request(&revision),
        &|| Ok(100),
    )
    .await
    .unwrap();
    let (composition, verification) =
        crate::matrix_tasks::compose_current_revision_with_validated_verification(
            Some(&mut store),
            &FakeValidator { trusted: true },
            workspace,
            revision.clone(),
            revision.revision,
            100,
        )
        .await
        .unwrap();
    let verification = verification.unwrap();
    let provider = crate::MatrixProviderRequest::new_verified(
        revision.clone(),
        composition.clone(),
        &verification,
        AdvisoryProviderProfileRef {
            id: "provider".into(),
        },
        AdvisoryModelConfiguration {
            model: "model".into(),
        },
    )
    .unwrap();
    assert_eq!(
        provider.binding().verification_digest.as_deref(),
        Some(record.digest.as_str())
    );
    assert_ne!(
        provider.binding().evaluation_digest,
        tect_domain::matrix_evaluation_digest(
            &revision.input,
            &composition,
            revision.choice_set.as_ref().unwrap()
        )
        .unwrap()
        .unwrap()
    );
    let config = tect_domain::WorkspaceAdvisoryConfig {
        workspace_id: workspace,
        revision: 1,
        mode: tect_domain::WorkspaceAdvisoryMode::Optional,
        materialized: true,
        provider_profile_ref: Some(provider.provider_profile_ref().clone()),
        model_configuration: Some(provider.model_configuration().clone()),
    };
    let advice_request = crate::RequestEngineeringAdvisory {
        task_id: revision.task_id,
        expected_task_revision: revision.revision,
        request_key: "verified-capture".into(),
        session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
    };
    let mut opportunity = crate::matrix_tasks::matrix_advisory_opportunity_input(
        &revision,
        &advice_request,
        &config,
        Uuid::new_v4(),
        Uuid::new_v4(),
    )
    .unwrap();
    opportunity.primary_reason = tect_domain::AdvisoryReason::CapabilityUnavailable;
    let fake_provider = FakeAdviceProvider(MatrixProviderIdentity {
        provider_profile_ref: provider.provider_profile_ref().clone(),
        model_configuration: provider.model_configuration().clone(),
        destination: "fake".into(),
        wire_version: "fake/1".into(),
    });
    let saved = crate::StoredMatrixDispatch {
        dispatch: tect_domain::AdvisoryDispatch {
            id: Uuid::new_v4(),
            opportunity_id: Uuid::new_v4(),
            predecessor_dispatch_id: None,
            attempt_number: 1,
            provider: "provider".into(),
            model: "model".into(),
            configuration_digest: "a".repeat(64),
            material_digest: provider.binding().evaluation_digest.clone(),
            payload_digest: "b".repeat(64),
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: tect_domain::AdvisoryDispatchState::Sealed,
            send_certainty: tect_domain::AdvisorySendCertainty::Sent,
            outcome: Some(tect_domain::AdvisoryDispatchOutcome::ProviderResponse),
            retry_basis: tect_domain::AdvisoryRetryBasis::Initial,
            raw_response_ref: None,
        },
        binding: provider.binding().clone(),
        provider_profile_ref: provider.provider_profile_ref().clone(),
        model_configuration: provider.model_configuration().clone(),
        configuration_snapshot: serde_json::json!({}),
        destination: "fake".into(),
        wire_version: "fake/1".into(),
        request_payload: b"verified-body".to_vec(),
        request_payload_sha256: "b".repeat(64),
        response_payload: Some(b"opaque".to_vec()),
        response_payload_sha256: Some("c".repeat(64)),
    };
    assert_eq!(
        fake_provider.parse_sealed_response(&provider, &saved),
        Err(Error::TransportUnavailable)
    );
    let policy_id = Uuid::new_v4();
    let ceilings = tect_domain::AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 100,
        output_tokens: 100,
        request_utf8_bytes: 1000,
        elapsed_monotonic_ms: 1000,
        retry_dispatches: 1,
    };
    let policy = tect_domain::AdvisoryBudgetPolicy::new(
        policy_id,
        1,
        tect_domain::AdvisoryBudgetPolicy::digest_for(policy_id, 1, 0, 1000, ceilings),
        0,
        1000,
        ceilings,
        Uuid::new_v4(),
        "a".repeat(128),
    )
    .unwrap();
    let mut without_policy = opportunity.clone();
    let denied = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
        &mut without_policy,
        Some(&provider),
        workspace,
        Uuid::new_v4(),
        &fake_provider,
        &FakeBudget,
        None,
    )
    .await
    .unwrap();
    assert!(matches!(
        denied,
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
    ));
    assert_eq!(
        without_policy.primary_reason,
        tect_domain::AdvisoryReason::BudgetPolicyInvalid
    );
    let prepared = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
        &mut opportunity,
        Some(&provider),
        workspace,
        Uuid::new_v4(),
        &fake_provider,
        &FakeBudget,
        Some(&policy),
    )
    .await
    .unwrap();
    assert!(matches!(
        prepared,
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::Authorized { .. }
    ));
    assert_eq!(
        opportunity.matrix_verification_digest.as_deref(),
        Some(record.digest.as_str())
    );
    assert_eq!(
        opportunity.material_digest,
        provider.binding().evaluation_digest
    );
    let (revoked, token) =
        crate::matrix_tasks::compose_current_revision_with_validated_verification(
            Some(&mut store),
            &FakeValidator { trusted: false },
            workspace,
            revision.clone(),
            revision.revision,
            100,
        )
        .await
        .unwrap();
    assert!(token.is_none());
    assert!(
        crate::MatrixProviderRequest::new_verified(
            revision.clone(),
            revoked,
            &verification,
            AdvisoryProviderProfileRef {
                id: "provider".into()
            },
            AdvisoryModelConfiguration {
                model: "model".into()
            },
        )
        .is_err()
    );
    let mut advanced = revision;
    advanced.revision += 1;
    assert!(
        crate::MatrixProviderRequest::new_verified(
            advanced,
            composition,
            &verification,
            AdvisoryProviderProfileRef {
                id: "provider".into()
            },
            AdvisoryModelConfiguration {
                model: "model".into()
            },
        )
        .is_err()
    );
}
