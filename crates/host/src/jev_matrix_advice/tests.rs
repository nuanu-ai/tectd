use super::*;
use serde_json::{Value, json};
use sha2::Digest;
use tect_domain::{
    EngineeringCandidate, FactProvenance, MATRIX_CHOICE_SET_SCHEMA, MatrixFact,
    MatrixSourceVerificationStatus, OperatingEnvelope, OperationalFacts,
    OwnerReportedEngineeringMatrixFacts, compose_owner_reported_engineering_matrix,
};
use uuid::Uuid;

const MODEL: &str = "jev-1.13.0";

fn fixture() -> (MatrixTaskRevision, EngineeringMatrixComposition) {
    let task_id = Uuid::from_u128(11);
    let input = EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: MatrixFact::Known {
                value: "owner reported workload".into(),
                provenance: FactProvenance("owner source #3".into()),
            },
            operational_facts: OperationalFacts::Absent,
        },
        criticality: MatrixFact::Unknown {
            provenance: FactProvenance("owner source #3".into()),
        },
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: MatrixFact::Absent,
        actual_exposure: MatrixFact::Absent,
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: MatrixFact::Absent,
    };
    let choice_set = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: "set-17".into(),
        version: 2,
        task_id: task_id.to_string(),
        task_revision: "3".into(),
        decision_question: "Which approach meets the recorded facts?".into(),
        candidates: ["a", "b", "c"]
            .into_iter()
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: format!("Approach {id}"),
                approach: format!("Implement approach {id}"),
                assumption_fact_ids: vec!["envelope.scale".into()],
            })
            .collect(),
    };
    let input_digest =
        canonical_matrix_input_digest(&serde_json::to_value(&input).unwrap()).unwrap();
    let choice_set_digest = choice_set.canonical_digest(&input).unwrap();
    let composition = compose_owner_reported_engineering_matrix(
        &OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            task_id.to_string(),
            "3".into(),
            input.clone(),
        )
        .unwrap(),
    );
    let revision = MatrixTaskRevision {
        task_id,
        revision: 3,
        request_id: Uuid::from_u128(12),
        input,
        input_digest,
        choice_set: Some(choice_set),
        choice_set_digest: Some(choice_set_digest),
        recorded_by_principal_id: Uuid::from_u128(13),
        recorded_by_session_id: Uuid::from_u128(14),
    };
    (revision, composition)
}

fn prepared() -> PreparedMatrixRankingRequest {
    let (revision, composition) = fixture();
    prepare_request(MODEL, &revision, &composition, 32_768).unwrap()
}

fn response(prepared: &PreparedMatrixRankingRequest, ranking: Value) -> Value {
    json!({
        "contract": MATRIX_EVALUATION_CONTRACT_VERSION,
        "model": MODEL,
        "binding": prepared.binding,
        "ranking": ranking,
        "usage": {"input_tokens": 8, "output_tokens": 4}
    })
}

fn parsed(
    value: &Value,
    prepared: &PreparedMatrixRankingRequest,
) -> Result<ParsedMatrixRankingResponse> {
    parse_response(&serde_json::to_vec(value).unwrap(), MODEL, prepared, 16_384)
}

#[test]
fn request_carries_complete_unredacted_accepted_material_and_pending_status() {
    let (revision, composition) = fixture();
    let prepared = prepare_request(MODEL, &revision, &composition, 32_768).unwrap();
    let body: Value = serde_json::from_slice(&prepared.body).unwrap();
    assert_eq!(body["model"], MODEL);
    assert_eq!(
        body["state"]["contract"],
        MATRIX_EVALUATION_CONTRACT_VERSION
    );
    assert_eq!(
        body["state"]["binding"],
        serde_json::to_value(&prepared.binding).unwrap()
    );
    assert_eq!(
        body["state"]["input"],
        serde_json::to_value(&revision.input).unwrap()
    );
    assert_eq!(
        body["state"]["choice_set"],
        serde_json::to_value(&revision.choice_set).unwrap()
    );
    assert_eq!(
        body["state"]["composition"],
        serde_json::to_value(&composition).unwrap()
    );
    assert_eq!(
        body["state"]["input"]["envelope"]["scale"]["provenance"],
        "owner source #3"
    );
    assert_eq!(
        body["state"]["composition"]["source_verification_status"],
        "owner_reported_pending_independent_verification"
    );
    assert_eq!(
        body["questions"]["ranking"]["candidate_ids"],
        json!(["a", "b", "c"])
    );
    assert_eq!(prepared.binding.task_id, revision.task_id.to_string());
    assert_eq!(prepared.binding.task_revision, "3");
    assert_eq!(prepared.binding.choice_set_id, "set-17");
    assert_eq!(prepared.binding.choice_set_version, 2);
    assert_eq!(prepared.binding.input_digest, revision.input_digest);
    assert_eq!(
        prepared.binding.choice_set_digest,
        revision.choice_set_digest.unwrap()
    );
    assert_eq!(prepared.binding.evaluation_digest.len(), 64);
    assert_eq!(
        composition.source_verification_status,
        MatrixSourceVerificationStatus::OwnerReportedPendingIndependentVerification
    );
}

#[test]
fn verified_wire_binds_record_digest_and_v2_contract() {
    let (revision, mut composition) = fixture();
    composition.source_verification_status =
        MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported;
    let prepared = prepare_request_inner(
        MODEL,
        &revision,
        &composition,
        Some((&"d".repeat(64), &"e".repeat(64))),
        32_768,
    )
    .unwrap();
    let body: Value = serde_json::from_slice(&prepared.body).unwrap();
    assert_eq!(
        prepared.contract,
        MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION
    );
    assert_eq!(
        body["state"]["contract"],
        MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION
    );
    assert_eq!(
        body["state"]["binding"]["verification_digest"],
        "d".repeat(64)
    );
    assert_eq!(
        body["state"]["binding"]["evaluation_digest"],
        "e".repeat(64)
    );
    let changed_record = prepare_request_inner(
        MODEL,
        &revision,
        &composition,
        Some((&"f".repeat(64), &"e".repeat(64))),
        32_768,
    )
    .unwrap();
    assert_ne!(
        sha2::Sha256::digest(&prepared.body),
        sha2::Sha256::digest(&changed_record.body)
    );
    let ranked = json!({
        "contract": MATRIX_VERIFIED_EVALUATION_CONTRACT_VERSION,
        "model": MODEL,
        "binding": prepared.binding,
        "ranking": {"status":"abstained", "ranked_candidate_ids":[], "recommended_candidate_id":null},
        "usage": null
    });
    assert!(parsed(&ranked, &prepared).is_ok());
    let mut old_contract = ranked;
    old_contract["contract"] = json!(MATRIX_EVALUATION_CONTRACT_VERSION);
    assert_eq!(
        parsed(&old_contract, &prepared),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn preparation_rejects_stale_or_unaccepted_material_and_bounds() {
    let (revision, composition) = fixture();
    assert_eq!(
        prepare_request(MODEL, &revision, &composition, 10),
        Err(Error::RequestTooLarge)
    );
    let mut bad = revision.clone();
    bad.input_digest = "different".into();
    assert_eq!(
        prepare_request(MODEL, &bad, &composition, 32_768),
        Err(Error::InvalidArguments)
    );
    let mut bad = revision.clone();
    bad.choice_set_digest = None;
    assert_eq!(
        prepare_request(MODEL, &bad, &composition, 32_768),
        Err(Error::InvalidArguments)
    );
    let mut stale = composition.clone();
    stale.task_revision = "2".into();
    assert_eq!(
        prepare_request(MODEL, &revision, &stale, 32_768),
        Err(Error::StaleRevision)
    );
    let mut incomplete = revision;
    incomplete
        .choice_set
        .as_mut()
        .unwrap()
        .candidates
        .truncate(1);
    incomplete.choice_set_digest = Some(
        incomplete
            .choice_set
            .as_ref()
            .unwrap()
            .canonical_digest(&incomplete.input)
            .unwrap(),
    );
    assert_eq!(
        prepare_request(MODEL, &incomplete, &composition, 32_768),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn strict_ranked_and_abstained_responses() {
    let prepared = prepared();
    let ranked = response(
        &prepared,
        json!({
            "status": "ranked", "ranked_candidate_ids": ["b", "a", "c"],
            "recommended_candidate_id": "b"
        }),
    );
    let result = parsed(&ranked, &prepared).unwrap();
    assert_eq!(result.input_tokens, Some(8));
    assert_eq!(result.output_tokens, Some(4));
    assert_eq!(
        result.ranking,
        MatrixRanking::Ranked {
            ranked_candidate_ids: vec!["b".into(), "a".into(), "c".into()],
            recommended_candidate_id: "b".into(),
        }
    );
    let abstained = response(
        &prepared,
        json!({
            "status": "abstained", "ranked_candidate_ids": [],
            "recommended_candidate_id": null
        }),
    );
    assert!(matches!(
        parsed(&abstained, &prepared).unwrap().ranking,
        MatrixRanking::Abstained { .. }
    ));
}

#[test]
fn response_rejects_bad_rankings_binding_model_contract_and_fields() {
    let prepared = prepared();
    let good = response(
        &prepared,
        json!({
            "status": "ranked", "ranked_candidate_ids": ["a", "b", "c"],
            "recommended_candidate_id": "a"
        }),
    );
    let mut cases = Vec::new();
    for ids in [
        json!(["a", "b"]),
        json!(["a", "a", "c"]),
        json!(["a", "b", "unknown"]),
    ] {
        let mut value = good.clone();
        value["ranking"]["ranked_candidate_ids"] = ids;
        cases.push(value);
    }
    let mut value = good.clone();
    value["ranking"]["recommended_candidate_id"] = json!("b");
    cases.push(value);
    let mut value = good.clone();
    value["ranking"]["extra"] = json!(true);
    cases.push(value);
    for key in [
        "task_id",
        "task_revision",
        "input_digest",
        "choice_set_id",
        "choice_set_version",
        "choice_set_digest",
        "evaluation_digest",
    ] {
        let mut value = good.clone();
        value["binding"][key] = json!("wrong");
        cases.push(value);
    }
    for key in ["model", "contract"] {
        let mut value = good.clone();
        value[key] = json!("wrong");
        cases.push(value);
    }
    let mut value = good.clone();
    value["unexpected"] = json!(0);
    cases.push(value);
    let mut value = good.clone();
    value.as_object_mut().unwrap().remove("usage");
    cases.push(value);
    let mut value = good.clone();
    value["usage"]["input_tokens"] = json!(-1);
    cases.push(value);
    let mut value = good.clone();
    value["ranking"] = json!({"status":"abstained", "ranked_candidate_ids": ["a"],
        "recommended_candidate_id": null});
    cases.push(value);
    for value in cases {
        assert!(parsed(&value, &prepared).is_err(), "accepted {value}");
    }
    let bytes = serde_json::to_vec(&good).unwrap();
    assert_eq!(
        parse_response(&bytes, MODEL, &prepared, bytes.len() - 1),
        Err(Error::RequestTooLarge)
    );
    let duplicate = format!(
        r#"{{"contract":"{MATRIX_EVALUATION_CONTRACT_VERSION}","contract":"{MATRIX_EVALUATION_CONTRACT_VERSION}"}}"#
    );
    assert!(parse_response(duplicate.as_bytes(), MODEL, &prepared, 16_384).is_err());
}
