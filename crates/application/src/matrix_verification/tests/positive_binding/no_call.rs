use super::*;

#[tokio::test]
async fn verified_no_call_captures_selection_material_without_provider_or_budget() {
    use tect_domain::{
        AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference, EngineeringCandidate,
        EngineeringChoiceSet, MATRIX_CHOICE_SET_SCHEMA, WorkspaceAdvisoryConfig,
        WorkspaceAdvisoryMode,
    };
    struct NoProvider;
    #[async_trait]
    impl MatrixAdviceProvider for NoProvider {
        fn identity(&self) -> Option<MatrixProviderIdentity> {
            panic!("no-call provider identity")
        }
        fn prepare(&self, _: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
            panic!("no-call prepare")
        }
        async fn attempt_prepared(
            &self,
            _: PreparedMatrixAdviceAttempt,
            _: MatrixStartedDispatchPermit,
        ) -> Result<MatrixProviderResponse> {
            panic!("no-call send")
        }
    }
    struct NoBudget;
    #[async_trait]
    impl MatrixBudgetPolicy for NoBudget {
        async fn authorize(
            &self,
            _: &MatrixBudgetRequest,
            _: &tect_domain::AdvisoryBudgetPolicy,
        ) -> Result<Option<MatrixBudgetAuthorization>> {
            panic!("no-call budget")
        }
    }
    let mut revision = revision();
    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "manual-selection".into(),
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
    let workspace = Uuid::new_v4();
    let mut store = FakeStore::default();
    verify_locked_revision(
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
    let (composition, token) =
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
    let verification = crate::MatrixDispositionVerification::LegacyV1 {
        composition,
        verification: token.unwrap(),
    };
    let config = WorkspaceAdvisoryConfig {
        workspace_id: workspace,
        revision: 1,
        mode: WorkspaceAdvisoryMode::Optional,
        materialized: true,
        provider_profile_ref: None,
        model_configuration: None,
    };
    let request = crate::RequestEngineeringAdvisory {
        task_id: revision.task_id,
        expected_task_revision: revision.revision,
        request_key: "skip-current-source".into(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::Skip,
    };
    for reason in [
        AdvisoryReason::RequestSkip,
        AdvisoryReason::SessionSkip,
        AdvisoryReason::WorkspaceDisabled,
        AdvisoryReason::ProviderUnconfigured,
        AdvisoryReason::BudgetPolicyInvalid,
    ] {
        let mut input = crate::matrix_tasks::matrix_advisory_opportunity_input(
            &revision,
            &request,
            &config,
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .unwrap();
        input.primary_reason = reason;
        if reason == AdvisoryReason::SessionSkip {
            input.session_preference = AdvisoryRequestPreference::Skip;
        }
        crate::matrix_advisory_capture::bind_verified_matrix_snapshot(
            &mut input,
            &revision,
            &verification,
        )
        .unwrap();
        assert_eq!(
            input.material_digest,
            verification
                .disposition_digest(&revision.input, revision.choice_set.as_ref().unwrap())
                .unwrap()
        );
        assert_eq!(
            input.matrix_verification_digest.as_deref(),
            Some(verification.record_digest())
        );
        let before = input.clone();
        for _ in 0..2 {
            assert!(matches!(
                crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
                    &mut input,
                    None,
                    workspace,
                    Uuid::new_v4(),
                    &NoProvider,
                    &NoBudget,
                    None,
                )
                .await
                .unwrap(),
                crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
            ));
            assert_eq!(input, before);
            assert_eq!(input.state, AdvisoryOpportunityState::NoCall);
        }
        input.matrix_task_revision = Some(revision.revision + 1);
        assert!(
            crate::matrix_advisory_capture::bind_verified_matrix_snapshot(
                &mut input,
                &revision,
                &verification
            )
            .is_err()
        );
        input.matrix_task_revision = Some(revision.revision);
        input.matrix_choice_set_digest = Some("f".repeat(64));
        assert!(
            crate::matrix_advisory_capture::bind_verified_matrix_snapshot(
                &mut input,
                &revision,
                &verification
            )
            .is_err()
        );
    }
}
