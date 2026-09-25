use super::*;

#[test]
fn composition_uses_exact_stored_revision_and_keeps_gaps_visible() {
    let stored = stored_revision();
    let output = compose_current_revision(stored.clone(), 2).unwrap();
    assert_eq!(output.task_id, stored.task_id.to_string());
    assert_eq!(output.task_revision, "2");
    assert_eq!(output.mandatory_cards[0].id, "EM02-SCOPE@0.1");
    assert!(!output.is_resolved());
}

#[tokio::test]
async fn complete_stored_demo_remains_pending_independent_verification() {
    let mut stored = stored_revision();
    stored.input = EngineeringMatrixInput {
        mode: known(EngineeringMode::Demo),
        envelope: OperatingEnvelope {
            scale: known("one synthetic request".into()),
            operational_facts: OperationalFacts::Reported {
                entries: vec![OperatingFact {
                    name: "environment".into(),
                    fact: known("synthetic".into()),
                }],
            },
        },
        criticality: known("no protected guarantee".into()),
        intent: known(EngineeringIntent::Other("demo".into())),
        urgency: known("ordinary".into()),
        promised_behavior: known("real demo".into()),
        promised_proof: known("demo check".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: FactProvenance("owner report".into()),
        },
        actual_exposure: known(false),
        demand_commitment: known(CommitmentEvidence::NoCommitment),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(false),
    };
    let output = compose_current_revision(stored.clone(), 2).unwrap();
    assert!(output.unresolved_evidence.is_empty());
    assert_eq!(
        output.source_verification_status,
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
    );
    assert!(!output.is_resolved());
    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "verified-source-needed".into(),
        version: 1,
        task_id: stored.task_id.to_string(),
        task_revision: stored.revision.to_string(),
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
    stored.input_digest =
        canonical_matrix_input_digest(&serde_json::to_value(&stored.input).unwrap()).unwrap();
    stored.choice_set_digest = Some(choice.canonical_digest(&stored.input).unwrap());
    stored.choice_set = Some(choice);
    let request = advisory_request(stored.task_id);
    let config = advisory_config(WorkspaceAdvisoryMode::Optional);
    let mut receipt = matrix_advisory_opportunity_input(
        &stored,
        &request,
        &config,
        Uuid::new_v4(),
        Uuid::new_v4(),
    )
    .unwrap();
    assert_eq!(
        receipt.primary_reason,
        AdvisoryReason::MatrixSourceUnverified
    );
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
    let actor_id = receipt.authorized_actor_id;
    let prepared = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
        &mut receipt,
        None,
        config.workspace_id,
        actor_id,
        &provider,
        &TestBudget(true),
    )
    .await
    .unwrap();
    assert!(matches!(
        prepared,
        crate::matrix_advisory_capture::PreparedMatrixOpportunity::NoCall
    ));
    assert_eq!(receipt.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        receipt.primary_reason,
        AdvisoryReason::MatrixSourceUnverified
    );
}

#[test]
fn composition_rejects_stale_or_invalid_expected_revision() {
    let stored = stored_revision();
    assert_eq!(
        compose_current_revision(stored.clone(), 1),
        Err(Error::StaleRevision)
    );
    assert_eq!(
        compose_current_revision(stored, 0),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn canonical_digest_is_independent_of_json_object_key_order() {
    let left = serde_json::json!({"mode": {"known": {"value": "production", "provenance": "source"}}, "envelope": {"scale": "one"}});
    let right = serde_json::from_str::<serde_json::Value>(
        r#"{"envelope":{"scale":"one"},"mode":{"known":{"provenance":"source","value":"production"}}}"#,
    )
    .unwrap();
    assert_eq!(
        canonical_matrix_input_digest(&left).unwrap(),
        canonical_matrix_input_digest(&right).unwrap()
    );
}

#[test]
fn choice_set_must_bind_to_exact_revision_and_input() {
    let stored = stored_revision();
    let mut request = RecordMatrixTask {
        task_id: stored.task_id,
        revision: 2,
        expected_current_revision: 1,
        request_id: stored.request_id,
        input: stored.input,
        choice_set: None,
    };
    assert_eq!(validate_request(&request), Ok(()));
    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "choice-1".into(),
        version: 1,
        task_id: request.task_id.to_string(),
        task_revision: "2".into(),
        decision_question: "Which approach?".into(),
        candidates: vec![EngineeringCandidate {
            candidate_id: "a".into(),
            title: "A".into(),
            approach: "Use A".into(),
            assumption_fact_ids: vec!["criticality".into()],
        }],
    };
    for count in 0..=1 {
        let mut choice = choice.clone();
        choice.candidates.truncate(count);
        request.choice_set = Some(choice);
        assert_eq!(validate_request(&request), Ok(()));
    }
    let mut choice = choice.clone();
    choice.task_revision = "1".into();
    request.choice_set = Some(choice.clone());
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
    let mut choice = choice.clone();
    choice.task_revision = "2".into();
    choice.task_id = Uuid::new_v4().to_string();
    request.choice_set = Some(choice.clone());
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
    choice.task_id = request.task_id.to_string();
    choice.candidates[0].assumption_fact_ids = vec!["unknown.fact".into()];
    request.choice_set = Some(choice);
    assert_eq!(validate_request(&request), Err(Error::InvalidArguments));
}
