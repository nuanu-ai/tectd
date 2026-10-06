use super::*;
use std::cell::Cell;

#[test]
fn public_read_exposes_only_current_typed_advice() {
    let task_id = Uuid::new_v4();
    let opportunity: AdvisoryOpportunity = serde_json::from_value(json!({
        "id":Uuid::new_v4(),"workspace_id":Uuid::new_v4(),"session_id":Uuid::new_v4(),
        "authorized_actor_id":Uuid::new_v4(),"capability":"engineering_profile",
        "decision_point":"engineering.profile.before_selection","decision_point_version":1,
        "workflow_occurrence_key":"key","target_kind":"matrix_task","target_id":task_id,
        "work_revision":2,"matrix_task_revision":2,"matrix_choice_set_digest":"b".repeat(64),
        "matrix_verification_digest":"d".repeat(64),"source_ref":null,
        "session_preference":"use_workspace","request_preference":"use_workspace",
        "config_revision":1,"material_digest":"c".repeat(64),"state":"advised",
        "primary_reason":"provider_response","provider_called":true
    }))
    .unwrap();
    let base = tect_application::CurrentMatrixAdvice {
        advice_id: Uuid::new_v4(),
        dispatch_id: Uuid::new_v4(),
        task_revision: 2,
        input_digest: "a".repeat(64),
        choice_set_id: "choice".into(),
        choice_set_version: 1,
        choice_set_digest: "b".repeat(64),
        evaluation_digest: "c".repeat(64),
        verification_digest: "d".repeat(64),
        provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
            id: "provider".into(),
        },
        model_configuration: tect_domain::AdvisoryModelConfiguration {
            model: "model".into(),
        },
        response_payload_sha256: "e".repeat(64),
        advice_digest: "f".repeat(64),
        outcome: GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "b".into()],
        },
        trial_evidence: None,
    };
    let ranked = read(EngineeringAdvisoryRead {
        opportunity: opportunity.clone(),
        current_advice: Some(base.clone()),
    });
    assert_eq!(
        ranked["current_advice"]["outcome"]["ranked_choice_ids"],
        json!(["a", "b"])
    );
    assert!(ranked["current_advice"].get("trial_uncertainty").is_none());
    assert!(!ranked.to_string().contains("raw_response_payload"));
    let score = |id: &str, level: usize, confidence: f64| {
        let mut probabilities = [0.0; 10];
        probabilities[level] = 1.0;
        let distribution = tect_domain::NativeMatrixScoreDistribution::new(probabilities).unwrap();
        let bounds = distribution.feasible_expected_score();
        tect_domain::MatrixTrialCandidateEvidence {
            candidate_id: id.into(),
            declared_score: level as f64,
            score_confidence: confidence,
            probabilities,
            displayed_mean: distribution.displayed_mean(),
            feasible_minimum: bounds.minimum,
            feasible_maximum: bounds.maximum,
        }
    };
    let mut trial = base.clone();
    trial.trial_evidence = Some(tect_domain::MatrixTrialRankingEvidence {
        policy_id: tect_domain::MATRIX_TRIAL_POLICY_ID.into(),
        policy_version: tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION.into(),
        policy_digest: tect_domain::matrix_trial_policy_digest(),
        choice_selected_candidate_id: "a".into(),
        choice_confidence: 0.8,
        choice_selected_answer_probability: 0.8,
        scores: vec![score("a", 8, 0.9), score("b", 4, 0.5)],
        low_loser_confidence: true,
    });
    let owner = read(EngineeringAdvisoryRead {
        opportunity: opportunity.clone(),
        current_advice: Some(trial.clone()),
    });
    let verifier = read(EngineeringAdvisoryRead {
        opportunity: opportunity.clone(),
        current_advice: Some(trial),
    });
    assert_eq!(
        owner["current_advice"]["advice_digest"],
        verifier["current_advice"]["advice_digest"]
    );
    assert_eq!(
        owner["current_advice"]["trial_uncertainty"],
        verifier["current_advice"]["trial_uncertainty"]
    );
    assert_eq!(
        owner["current_advice"]["trial_uncertainty"]["schema"],
        "tect.matrix-trial-uncertainty/1"
    );
    assert_eq!(
        owner["current_advice"]["trial_uncertainty"]["scores"][1]["score_confidence"],
        0.5
    );
    assert_eq!(
        owner["current_advice"]["trial_uncertainty"]["low_loser_confidence"],
        true
    );
    assert_eq!(
        owner["current_advice"]["trial_uncertainty"]["digest_linkage"]["response_payload_sha256"],
        "e".repeat(64)
    );
    assert!(!owner.to_string().contains("raw_response_payload"));
    let mut rejected = base.clone();
    rejected.outcome = GuardedMatrixAdviceOutcome::Rejected {
        reason: "not current".into(),
    };
    assert!(
        read(EngineeringAdvisoryRead {
            opportunity: opportunity.clone(),
            current_advice: Some(rejected),
        })
        .get("current_advice")
        .is_none()
    );
    let mut abstained = base;
    abstained.outcome = GuardedMatrixAdviceOutcome::Abstained { reason: None };
    let abstained = read(EngineeringAdvisoryRead {
        opportunity: opportunity.clone(),
        current_advice: Some(abstained),
    });
    assert_eq!(
        abstained["current_advice"]["outcome"]["status"],
        "abstained"
    );
    let mut no_call_opportunity = opportunity;
    no_call_opportunity.state = tect_domain::AdvisoryOpportunityState::NoCall;
    no_call_opportunity.primary_reason = tect_domain::AdvisoryReason::RequestSkip;
    no_call_opportunity.provider_called = false;
    let no_call = read(EngineeringAdvisoryRead {
        opportunity: no_call_opportunity,
        current_advice: None,
    });
    assert!(no_call.get("current_advice").is_none());
    assert_eq!(no_call["provider_called"], false);
}

#[test]
fn strict_matrix_advisory_arguments() {
    let id = Uuid::new_v4();
    let base = json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1"});
    assert!(matches!(
        parse("request_engineering_advisory", base.clone()),
        Ok(MatrixAdvisoryInvocation::Request(_))
    ));
    assert!(matches!(
        parse(
            "get_engineering_advisory",
            json!({"task_id":id,"request_key":"task-1"})
        ),
        Ok(MatrixAdvisoryInvocation::Get { .. })
    ));
    for invalid in [
        json!({"task_id":Uuid::nil(),"expected_task_revision":1,"request_key":"task-1"}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"a\u{0}b"}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":""}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1 "}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"é".repeat(129)}),
        json!({"task_id":id,"expected_task_revision":0,"request_key":"task-1"}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":" task-1"}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"x".repeat(257)}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","principal_id":id}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","request_preference":"force"}),
        json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","session_preference":"skip"}),
    ] {
        assert!(parse("request_engineering_advisory", invalid).is_err());
    }
    assert!(
        parse(
            "get_engineering_advisory",
            json!({"task_id":id,"request_key":"task-1","workspace_id":id})
        )
        .is_err()
    );
}

#[tokio::test]
async fn tiny_output_capacity_rejects_before_service_invocation() {
    let request = RequestEngineeringAdvisory {
        task_id: Uuid::new_v4(),
        expected_task_revision: 1,
        request_key: "matrix-1".into(),
        session_preference: AdvisoryRequestPreference::UseWorkspace,
        request_preference: AdvisoryRequestPreference::UseWorkspace,
    };
    let called = Cell::new(false);
    let result = guarded_request(&request, 1, || {
        called.set(true);
        async { Err(Error::TransportUnavailable) }
    })
    .await;
    assert!(matches!(result, Err(Error::RequestTooLarge)));
    assert!(!called.get());
}
