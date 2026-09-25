use super::*;
use sha2::{Digest, Sha256};
use std::time::Duration;
use tect_domain::{
    PIPELINE_RECOMMENDATION_SCHEMA, PipelineDefinitionSnapshot, PipelineDeliveryMode,
    PipelineInstructionSnapshot, PipelineKind, PipelinePhaseDefinition, PipelinePhaseRetryPolicy,
    PipelineRecommendationOption, PipelineVerificationPlan,
};
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::TcpListener;
use uuid::Uuid;

async fn http_fixture(
    status: u16,
    body: Vec<u8>,
    delay: Duration,
) -> (
    Url,
    tokio::sync::oneshot::Receiver<Vec<u8>>,
    tokio::task::JoinHandle<()>,
) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let (sender, receiver) = tokio::sync::oneshot::channel();
    let task = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let head_end = loop {
            let count = socket.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..count]);
            if let Some(index) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..head_end]);
        let length = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse::<usize>().unwrap())
            })
            .unwrap_or(0);
        while request.len() < head_end + length {
            let count = socket.read(&mut buffer).await.unwrap();
            request.extend_from_slice(&buffer[..count]);
        }
        let _ = sender.send(request);
        tokio::time::sleep(delay).await;
        let header = format!(
            "HTTP/1.1 {status} OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            body.len()
        );
        let _ = socket.write_all(header.as_bytes()).await;
        let _ = socket.write_all(&body).await;
    });
    (endpoint, receiver, task)
}

fn http_provider(endpoint: Url, timeout: Duration, cap: usize) -> JevPipelineProvider {
    JevPipelineProvider::new(
        JevPipelineConfig {
            identity: PipelineProviderIdentity {
                provider: "fixture".into(),
                model: "jev-1.13.0".into(),
                destination: endpoint.as_str().into(),
                wire_version: WIRE_VERSION.into(),
            },
            endpoint,
            timeout,
            maximum_request_bytes: MAX_REQUEST_BYTES,
            maximum_response_bytes: cap,
        },
        "fixture-secret".into(),
    )
    .unwrap()
}

#[tokio::test]
async fn http_exact_frozen_body_raw_bytes_and_usage() {
    let native = prepared(2);
    let raw = serde_json::to_vec(&response(&native)).unwrap();
    let (endpoint, captured, server) = http_fixture(200, raw.clone(), Duration::ZERO).await;
    let provider = http_provider(endpoint, Duration::from_secs(2), MAX_RESPONSE_BYTES);
    let observed = provider.send_once(native.body.clone()).await.unwrap();
    let request = captured.await.unwrap();
    server.await.unwrap();
    let split = request
        .windows(4)
        .position(|part| part == b"\r\n\r\n")
        .unwrap()
        + 4;
    let headers = String::from_utf8_lossy(&request[..split]);
    assert!(headers.contains("Bearer fixture-secret"));
    assert_eq!(&request[split..], native.body);
    assert_eq!(observed.raw_response, raw);
    assert_eq!(
        (observed.input_tokens, observed.output_tokens),
        (Some(20), Some(30))
    );
}

#[tokio::test]
async fn http_unknown_usage_stays_unknown_and_errors_are_not_retried() {
    let body = br#"{"usage":{"input_tokens":0},"invalid":true}"#.to_vec();
    let (endpoint, captured, server) = http_fixture(200, body.clone(), Duration::ZERO).await;
    let observed = http_provider(endpoint, Duration::from_secs(1), MAX_RESPONSE_BYTES)
        .send_once(vec![1])
        .await
        .unwrap();
    captured.await.unwrap();
    server.await.unwrap();
    assert_eq!(observed.raw_response, body);
    assert_eq!(observed.input_tokens, Some(0));
    assert_eq!(observed.output_tokens, None);

    for (status, response, delay, timeout, cap) in [
        (
            503,
            b"{}".to_vec(),
            Duration::ZERO,
            Duration::from_secs(1),
            32,
        ),
        (
            200,
            vec![b'x'; 33],
            Duration::ZERO,
            Duration::from_secs(1),
            32,
        ),
        (
            200,
            b"{}".to_vec(),
            Duration::from_millis(100),
            Duration::from_millis(20),
            32,
        ),
    ] {
        let (endpoint, captured, server) = http_fixture(status, response, delay).await;
        let result = http_provider(endpoint, timeout, cap)
            .send_once(vec![1])
            .await;
        captured.await.unwrap();
        server.await.unwrap();
        assert!(matches!(
            result,
            Err(Error::TransportUnavailable | Error::RequestTooLarge)
        ));
    }
}

fn manifest(count: usize) -> PipelineRecommendationManifest {
    let options = PipelineKind::CURRENT_SLICE_RUN_KINDS[..count]
        .iter()
        .map(|kind| {
            let plan = PipelineVerificationPlan::from_definition(&definition(*kind)).unwrap();
            PipelineRecommendationOption {
                id: PipelineRecommendationOption::pair_id(*kind, &plan.id),
                kind: *kind,
                definition_version: "1".into(),
                definition_digest: "definition-digest".into(),
                completion_contract: "completion".into(),
                forbidden_claims: vec!["unverified success".into()],
                verification_plan: plan,
            }
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
        matrix_input_digest: "d".repeat(64),
        selected_candidate_digest: "e".repeat(64),
        compatibility_policy_digest: "f".repeat(64),
        mandatory_card_ids: vec!["card-1".into()],
        deterministic_kind: PipelineKind::CURRENT_SLICE_RUN_KINDS[0],
        deterministic_option_id: None,
        catalogue_revision: "4".into(),
        catalogue_digest: "c".repeat(64),
        options,
        excluded: PipelineKind::CURRENT_SLICE_RUN_KINDS[count..]
            .iter()
            .map(|kind| tect_domain::PipelineExcludedKind {
                kind: *kind,
                reason: tect_domain::PipelineExclusionReason::MissingRule,
            })
            .collect(),
        evidence_refs: vec![],
        digest: String::new(),
    };
    manifest.deterministic_option_id = Some(manifest.options[0].id.clone());
    manifest.digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&manifest).unwrap())
    );
    manifest.validate_digest().unwrap();
    manifest
}

fn definition(kind: PipelineKind) -> PipelineDefinitionSnapshot {
    let instruction = PipelineInstructionSnapshot {
        id: "instruction".into(),
        version: "1".into(),
        digest: "instruction-digest".into(),
        body: "instruction".into(),
        origin_refs: vec!["source".into()],
    };
    PipelineDefinitionSnapshot {
        kind,
        version: "1".into(),
        digest: "definition-digest".into(),
        overview: instruction.clone(),
        default_mode: PipelineDeliveryMode::Phasewise,
        allowed_modes: vec![PipelineDeliveryMode::Phasewise],
        phases: vec![PipelinePhaseDefinition {
            id: "proof".into(),
            ordinal: 1,
            title: "Proof".into(),
            required: true,
            disposition_required: false,
            instructions: vec![instruction],
            skills: vec![],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            required_fields: vec!["proof".into()],
            allowed_verdicts: vec![],
            required_dispositions: vec![],
            allowed_dispositions: vec![],
            output_constraints: vec![],
            verdict_routes: vec![],
            followup_contracts: vec![],
            allowed_backward_to: vec![],
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "proof".into(),
        }],
        completion_contract: "completion".into(),
        escalation_contract: "escalation".into(),
        forbidden_claims: vec!["unverified success".into()],
    }
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
    for (index, option) in manifest.options.iter().enumerate() {
        assert_eq!(prepared.eligible_ids[index], option.id);
        assert_eq!(
            request["state"]["manifest"]["options"][index]["kind"],
            json!(option.kind)
        );
        assert_eq!(
            request["state"]["manifest"]["options"][index]["verification_plan"]["id"],
            json!(option.verification_plan.id)
        );
        assert_eq!(
            request["state"]["manifest"]["options"][index]["verification_plan"]["digest"],
            json!(option.verification_plan.digest)
        );
        assert_eq!(
            request["state"]["manifest"]["options"][index]["verification_plan"]["source_definition_version"],
            json!(option.verification_plan.source_definition_version)
        );
        assert_eq!(
            request["state"]["manifest"]["options"][index]["verification_plan"]["obligations"],
            json!(option.verification_plan.obligations)
        );
    }
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
