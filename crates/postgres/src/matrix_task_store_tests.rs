use super::*;
use tect_domain::{EngineeringCandidate, MatrixFact, OperatingEnvelope, OperationalFacts};

fn input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: MatrixFact::Absent,
            operational_facts: OperationalFacts::Absent,
        },
        criticality: MatrixFact::Absent,
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: MatrixFact::Absent,
        actual_exposure: MatrixFact::Absent,
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: MatrixFact::Absent,
    }
}

fn choice(task_id: Uuid, revision: i64) -> EngineeringChoiceSet {
    EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "choice-1".into(),
        version: 1,
        task_id: task_id.to_string(),
        task_revision: revision.to_string(),
        decision_question: "Which approach?".into(),
        candidates: vec![EngineeringCandidate {
            candidate_id: "a".into(),
            title: "A".into(),
            approach: "Use A".into(),
            assumption_fact_ids: vec!["criticality".into()],
        }],
    }
}

#[test]
fn retry_matches_exact_record_only() {
    let input = input();
    let canonical = serde_json::to_value(&input).unwrap();
    let digest = canonical_matrix_input_digest(&canonical).unwrap();
    let task_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    let request = RecordMatrixTask {
        task_id,
        revision: 2,
        expected_current_revision: 1,
        request_id,
        input: input.clone(),
        choice_set: None,
    };
    let prior = MatrixTaskRevision {
        task_id,
        revision: 2,
        request_id,
        input,
        input_digest: digest.clone(),
        choice_set: None,
        choice_set_digest: None,
        recorded_by_principal_id: Uuid::new_v4(),
        recorded_by_session_id: Uuid::new_v4(),
    };
    assert!(same_request(&prior, &request, &canonical, &digest).unwrap());
    assert!(!same_request(&prior, &request, &canonical, &"0".repeat(64)).unwrap());
    let mut changed_canonical = canonical.clone();
    changed_canonical["mode"] = serde_json::json!({"state": "unknown", "provenance": "other"});
    assert!(!same_request(&prior, &request, &changed_canonical, &digest).unwrap());
    let mut changed_task = request.clone();
    changed_task.task_id = Uuid::new_v4();
    assert!(!same_request(&prior, &changed_task, &canonical, &digest).unwrap());
    let mut changed_revision = request;
    changed_revision.revision += 1;
    assert!(!same_request(&prior, &changed_revision, &canonical, &digest).unwrap());
}

#[test]
fn read_rejects_digest_mismatch() {
    let canonical = serde_json::to_value(input()).unwrap();
    let digest = canonical_matrix_input_digest(&canonical).unwrap();
    assert!(decode_input(canonical.clone(), &digest).is_ok());
    assert_eq!(
        decode_input(canonical, &"0".repeat(64)),
        Err(Error::InternalInvariant)
    );
}

#[test]
fn choice_read_checks_digest_schema_and_binding() {
    let input = input();
    let task_id = Uuid::new_v4();
    let choice = choice(task_id, 2);
    let json = serde_json::to_value(&choice).unwrap();
    let digest = choice.canonical_digest(&input).unwrap();
    assert_eq!(
        decode_choice_set(
            Some(MATRIX_CHOICE_SET_SCHEMA.into()),
            Some(json.clone()),
            Some(&digest),
            task_id,
            2,
            &input
        ),
        Ok(Some(choice.clone()))
    );
    assert_eq!(
        decode_choice_set(None, None, None, task_id, 2, &input),
        Ok(None)
    );
    assert_eq!(
        decode_choice_set(
            Some(MATRIX_CHOICE_SET_SCHEMA.into()),
            Some(json.clone()),
            Some(&"0".repeat(64)),
            task_id,
            2,
            &input
        ),
        Err(Error::InternalInvariant)
    );
    assert_eq!(
        decode_choice_set(
            Some("wrong".into()),
            Some(json.clone()),
            Some(&digest),
            task_id,
            2,
            &input
        ),
        Err(Error::InternalInvariant)
    );
    assert_eq!(
        decode_choice_set(
            Some(MATRIX_CHOICE_SET_SCHEMA.into()),
            Some(json),
            Some(&digest),
            task_id,
            3,
            &input
        ),
        Err(Error::InternalInvariant)
    );
}

#[test]
fn retry_compares_choice_set_even_when_facts_match() {
    let input = input();
    let canonical = serde_json::to_value(&input).unwrap();
    let digest = canonical_matrix_input_digest(&canonical).unwrap();
    let task_id = Uuid::new_v4();
    let choice_set = choice(task_id, 1);
    let prior = MatrixTaskRevision {
        task_id,
        revision: 1,
        request_id: Uuid::new_v4(),
        input: input.clone(),
        input_digest: digest.clone(),
        choice_set: Some(choice_set.clone()),
        choice_set_digest: Some(choice_set.canonical_digest(&input).unwrap()),
        recorded_by_principal_id: Uuid::new_v4(),
        recorded_by_session_id: Uuid::new_v4(),
    };
    let mut request = RecordMatrixTask {
        task_id,
        revision: 1,
        expected_current_revision: 0,
        request_id: prior.request_id,
        input,
        choice_set: Some(choice_set),
    };
    assert!(same_request(&prior, &request, &canonical, &digest).unwrap());
    request.choice_set.as_mut().unwrap().decision_question = "A different question?".into();
    assert!(!same_request(&prior, &request, &canonical, &digest).unwrap());
    request.choice_set = None;
    assert!(!same_request(&prior, &request, &canonical, &digest).unwrap());
}
