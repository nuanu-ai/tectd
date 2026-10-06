use super::*;

#[tokio::test]
async fn sealed_legacy_reconstruction_uses_saved_digest_after_newer_verification() {
    use tect_domain::{
        AdvisoryModelConfiguration, AdvisoryProviderProfileRef, EngineeringCandidate,
        EngineeringChoiceSet, MATRIX_CHOICE_SET_SCHEMA,
    };
    let mut revision = revision();
    let choice_set = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "saved-choice".into(),
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
    let first = verify_locked_revision(
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
    let (composition, Some(token)) =
        crate::matrix_tasks::compose_current_revision_with_validated_verification(
            Some(&mut store),
            &FakeValidator { trusted: true },
            workspace,
            revision.clone(),
            revision.revision,
            100,
        )
        .await
        .unwrap()
    else {
        panic!("first verification must be usable")
    };
    let profile = AdvisoryProviderProfileRef {
        id: "provider".into(),
    };
    let model = AdvisoryModelConfiguration {
        model: "model".into(),
    };
    let expected = crate::MatrixProviderRequest::new_verified(
        revision.clone(),
        composition,
        &token,
        profile.clone(),
        model.clone(),
    )
    .unwrap();
    let second = verify_locked_revision(
        &mut store,
        &FakeValidator { trusted: true },
        MatrixVerificationActor {
            workspace_id: workspace,
            verifier_principal_id: Uuid::new_v4(),
            verifier_session_id: Uuid::new_v4(),
        },
        &revision,
        &request(&revision),
        &|| Ok(101),
    )
    .await
    .unwrap();
    assert_ne!(first.digest, second.digest);
    assert_eq!(store.saved.last().unwrap().digest, second.digest);
    let restored = crate::matrix_advisory_dispatch::reconstruct_historical_legacy_request(
        &revision,
        &first,
        100,
        expected.binding(),
        profile.clone(),
        model.clone(),
    )
    .unwrap()
    .unwrap();
    assert_eq!(restored.binding(), expected.binding());
    assert!(
        crate::matrix_advisory_dispatch::reconstruct_historical_legacy_request(
            &revision,
            &second,
            101,
            expected.binding(),
            profile,
            model,
        )
        .unwrap()
        .is_none()
    );
}
