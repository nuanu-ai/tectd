use super::*;
use sha2::{Digest, Sha256};
use tect_domain::{PIPELINE_RECOMMENDATION_SCHEMA, PipelineKind, PipelineRecommendationOption};
use uuid::Uuid;

fn manifest(count: usize) -> PipelineRecommendationManifest {
    let options = PipelineKind::CURRENT_SLICE_RUN_KINDS[..count]
        .iter()
        .map(|kind| PipelineRecommendationOption {
            id: kind.as_str().into(),
            kind: *kind,
            definition_version: "1".into(),
            definition_digest: "definition-digest".into(),
            completion_contract: "completion".into(),
            forbidden_claims: vec!["unverified success".into()],
            obligations: vec![],
        })
        .collect();
    let mut manifest = PipelineRecommendationManifest {
        schema: PIPELINE_RECOMMENDATION_SCHEMA.into(),
        work_id: Uuid::parse_str("00000000-0000-0000-0000-000000000001").unwrap(),
        work_revision: 1,
        matrix_task_id: "task".into(),
        matrix_task_revision: "1".into(),
        selected_choice_id: "matrix-choice".into(),
        matrix_choice_set_digest: "a".repeat(64),
        matrix_verification_digest: "b".repeat(64),
        mandatory_card_ids: vec!["card-1".into()],
        catalogue_revision: "4".into(),
        catalogue_digest: "c".repeat(64),
        options,
        evidence_refs: vec![],
        digest: String::new(),
    };
    manifest.digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    manifest.validate_digest().unwrap();
    manifest
}

fn prepared(count: usize) -> PreparedPipelineNativeRequest {
    prepare_native_request("jev-1.13.0", &manifest(count), MAX_REQUEST_BYTES).unwrap()
}

fn score_answer(level: usize) -> Value {
    let legend = SCORE_LEVELS
        .iter()
        .enumerate()
        .map(|(index, label)| (index.to_string(), json!(label)))
        .collect::<serde_json::Map<_, _>>();
    let probabilities = (0..10)
        .map(|index| {
            (
                index.to_string(),
                json!(if index == level { 1.0 } else { 0.0 }),
            )
        })
        .collect::<serde_json::Map<_, _>>();
    json!({"type":"score", "score":level, "legend":legend, "probabilities":probabilities, "confidence":0.91})
}

fn response(prepared: &PreparedPipelineNativeRequest) -> Value {
    let mut answers = serde_json::Map::new();
    for index in 0..prepared.eligible_ids.len() {
        answers.insert(format!("score_v1_{index}"), score_answer(9 - index));
    }
    let mut probabilities = serde_json::Map::new();
    for (index, id) in prepared.eligible_ids.iter().enumerate() {
        probabilities.insert(id.clone(), json!(if index == 0 { 0.8 } else { 0.0 }));
    }
    probabilities.insert(ABSTAIN.into(), json!(0.2));
    answers.insert(
        CHOICE_ID.into(),
        json!({
            "type":"choice", "choice":prepared.eligible_ids[0],
            "probabilities":probabilities, "confidence":0.8
        }),
    );
    json!({"model":"jev-1.13.0", "answers":answers, "usage":{"input_tokens":20,"output_tokens":30}})
}

fn parse(
    value: &Value,
    prepared: &PreparedPipelineNativeRequest,
) -> Result<ParsedPipelineNativeResponse> {
    parse_native_response(
        &serde_json::to_vec(value).unwrap(),
        prepared,
        MAX_RESPONSE_BYTES,
    )
}

#[test]
fn eight_way_request_and_fake_provider_response_rank_exact_permutation() {
    let manifest = manifest(8);
    let prepared = prepared(8);
    let request: Value = serde_json::from_slice(&prepared.body).unwrap();
    assert_eq!(request["state"]["manifest_digest"], manifest.digest);
    assert_eq!(request["state"]["manifest"], json!(manifest));
    assert_eq!(request["questions"].as_object().unwrap().len(), 9);
    assert_eq!(
        request["questions"][CHOICE_ID]["criteria"]
            .as_object()
            .unwrap()
            .len(),
        9
    );
    let parsed = parse(&response(&prepared), &prepared).unwrap();
    assert_eq!(parsed.abstain_reason, None);
    assert_eq!((parsed.input_tokens, parsed.output_tokens), (20, 30));
    assert_eq!(parsed.selected_probability, 0.8);
    assert_eq!(parsed.choice_confidence, 0.8);
    assert_eq!(
        parsed.ranking,
        PipelineRecommendationRanking::Ranked {
            ranked_ids: prepared.eligible_ids.clone()
        }
    );
    parsed.ranking.validate(&manifest).unwrap();
}

#[test]
fn canonical_request_bytes_bind_exact_manifest_digest() {
    let first = prepared(8);
    let second = prepared(8);
    assert_eq!(first.body, second.body);
    let digest = format!("{:x}", Sha256::digest(&first.body));
    assert_eq!(
        digest,
        "7fbf59270a3249fb84ebebb54873b54597cc47b448c28c69ce5276492381a23f"
    );
    let mut changed = manifest(8);
    changed.selected_choice_id = "different-choice".into();
    changed.digest.clear();
    changed.digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&changed).unwrap())
    );
    assert_ne!(
        prepare_native_request("jev-1.13.0", &changed, MAX_REQUEST_BYTES)
            .unwrap()
            .body,
        first.body
    );
}

#[test]
fn supports_partial_catalogue_and_rejects_invalid_counts_or_binding() {
    let two = prepared(2);
    assert_eq!(
        serde_json::from_slice::<Value>(&two.body).unwrap()["questions"]
            .as_object()
            .unwrap()
            .len(),
        3
    );
    assert_eq!(
        parse(&response(&two), &two).unwrap().ranking,
        PipelineRecommendationRanking::Ranked {
            ranked_ids: two.eligible_ids.clone()
        }
    );
    assert_eq!(
        prepare_native_request("jev-1.13.0", &manifest(1), MAX_REQUEST_BYTES),
        Err(Error::InvalidArguments)
    );
    let mut wrong = manifest(8);
    wrong.digest = "a".repeat(64);
    assert_eq!(
        prepare_native_request("jev-1.13.0", &wrong, MAX_REQUEST_BYTES),
        Err(Error::InputConflict)
    );
    assert_eq!(
        prepare_native_request("jev-1.13.0", &manifest(8), 1),
        Err(Error::RequestTooLarge)
    );
    let bytes = serde_json::to_vec(&response(&two)).unwrap();
    assert_eq!(
        parse_native_response(&bytes, &two, bytes.len() - 1),
        Err(Error::RequestTooLarge)
    );
}

#[test]
fn typed_abstain_reasons_cover_policy_boundaries() {
    let p = prepared(2);
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["choice"] = json!(ABSTAIN);
    value["answers"][CHOICE_ID]["probabilities"][ABSTAIN] = json!(0.8);
    value["answers"][CHOICE_ID]["probabilities"][&p.eligible_ids[0]] = json!(0.2);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::ProviderAbstained)
    );
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["probabilities"][&p.eligible_ids[0]] = json!(0.6);
    value["answers"][CHOICE_ID]["probabilities"][ABSTAIN] = json!(0.4);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::LowChoiceProbability)
    );
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["confidence"] = json!(0.69);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::LowChoiceConfidence)
    );
    let mut value = response(&p);
    value["answers"]["score_v1_0"] = score_answer(7);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::ChoiceScoreDisagreement)
    );
    let mut value = response(&p);
    value["answers"]["score_v1_1"]["score"] = json!(8.95);
    value["answers"]["score_v1_1"]["probabilities"]["8"] = json!(0.05);
    value["answers"]["score_v1_1"]["probabilities"]["9"] = json!(0.95);
    assert_eq!(
        parse(&value, &p).unwrap().abstain_reason,
        Some(PipelineNativeAbstainReason::InsufficientScoreSeparation)
    );
}

#[test]
fn rejects_malformed_unknown_missing_duplicate_ids_and_incoherent_answers() {
    let p = prepared(2);
    assert_eq!(
        parse_native_response(b"{", &p, MAX_RESPONSE_BYTES),
        Err(Error::InvalidArguments)
    );
    let mut duplicate = p.clone();
    duplicate.eligible_ids[1] = duplicate.eligible_ids[0].clone();
    assert_eq!(
        parse(&response(&p), &duplicate),
        Err(Error::InvalidArguments)
    );
    let mut value = response(&p);
    value["answers"][CHOICE_ID]["choice"] = json!("unknown-id");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]
        .as_object_mut()
        .unwrap()
        .remove("score_v1_1");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]["score_v1_2"] = score_answer(4);
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]["score_v1_0"]["score"] = json!(8.5);
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["answers"]["score_v1_0"]["legend"]["9"] = json!("altered");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
    let mut value = response(&p);
    value["model"] = json!("unexpected-model");
    assert_eq!(parse(&value, &p), Err(Error::InvalidArguments));
}
