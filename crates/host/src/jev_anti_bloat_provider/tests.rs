use super::*;
use serde_json::{Value, json};
use tect_application::{AntiBloatPreparedRequest, anti_bloat_material_sha256};

fn config() -> JevAntiBloatConfig {
    JevAntiBloatConfig {
        profile: "local-test".into(),
        endpoint: Url::parse("http://127.0.0.1:1/v1/systemone").unwrap(),
        model: "jev-1.13.0".into(),
        timeout: Duration::from_millis(100),
        maximum_request_bytes: 100000,
        maximum_response_bytes: 16000,
    }
}
fn provider(config: JevAntiBloatConfig) -> JevAntiBloatProvider {
    JevAntiBloatProvider::new(config, "local-test".into()).unwrap()
}
fn permit(provider: &JevAntiBloatProvider) -> AntiBloatSendPermit {
    let saved = crate::jev_anti_bloat_choice::tests::saved();
    let eligible_ids = saved
        .review
        .findings
        .iter()
        .map(|f| f.id.clone())
        .collect::<Vec<_>>();
    let bytes = provider
        .prepare(&AntiBloatRankingMaterial {
            saved: &saved,
            eligible_ids: &eligible_ids,
        })
        .unwrap();
    AntiBloatSendPermit {
        review_id: saved.review_id,
        request: AntiBloatPreparedRequest {
            sha256: format!("{:x}", Sha256::digest(&bytes)),
            bytes,
            material_sha256: anti_bloat_material_sha256(&saved).unwrap(),
            adapter_identity: provider.adapter_identity().into(),
        },
    }
}
fn response() -> Value {
    json!({"model":"jev-1.13.0","answers":{"anti_bloat_order_v1":{"type":"choice","choice":"R0","probabilities":{"R0":0.3,"R1":0.2,"R2":0.15,"R3":0.1,"R4":0.09,"R5":0.07,"R6":0.05,"ABSTAIN":0.04}}},"usage":{"input_tokens":20,"output_tokens":30}})
}
fn observation(value: Value, status: Option<u16>) -> AntiBloatProviderObservation {
    AntiBloatProviderObservation {
        raw: serde_json::to_vec(&value).unwrap(),
        http_status: status,
        input_tokens: None,
        output_tokens: None,
        elapsed_monotonic_ms: Some(5),
    }
}

#[test]
fn validated_config_available_and_bound_to_endpoint_profile_model_wire() {
    let p = provider(config());
    assert!(p.available());
    let permit = permit(&p);
    let body: Value = serde_json::from_slice(&permit.request.bytes).unwrap();
    assert_eq!(
        body["state"]["provider_binding_digest"],
        p.provider_binding_digest
    );
    assert!(p.restore(&permit).is_ok());
    for field in 0..3 {
        let mut c = config();
        match field {
            0 => c.endpoint = Url::parse("http://127.0.0.1:2/v1/systemone").unwrap(),
            1 => c.profile = "changed".into(),
            _ => c.model = "jev-1.14.0".into(),
        };
        assert!(provider(c).restore(&permit).is_err());
    }
    let same = provider(config());
    assert!(same.restore(&permit).is_ok());
    let changed_credential =
        JevAntiBloatProvider::new(config(), "rotated-local-test".into()).unwrap();
    assert!(changed_credential.restore(&permit).is_ok());
    let mut invalid = config();
    invalid.model.clear();
    assert!(JevAntiBloatProvider::new(invalid, "local-test".into()).is_err());
    let mut invalid = config();
    invalid.profile.clear();
    assert!(JevAntiBloatProvider::new(invalid, "local-test".into()).is_err());
    assert!(JevAntiBloatProvider::new(config(), String::new()).is_err());
}

#[test]
fn postseal_usage_and_ranking_non_success_never_advice() {
    let p = provider(config());
    let permit = permit(&p);
    let observation = observation(response(), Some(200));
    assert_eq!(
        p.usage_sealed(&permit, &observation).unwrap(),
        AntiBloatUsage {
            input_tokens: Some(20),
            output_tokens: Some(30)
        }
    );
    assert_eq!(
        p.parse_sealed(&permit, &observation).unwrap(),
        AntiBloatRankingOutcome::Ranked((0..7).map(|i| format!("id-{i}")).collect())
    );
    for status in [None, Some(500), Some(302)] {
        let mut observation = observation.clone();
        observation.http_status = status;
        assert_eq!(
            p.parse_sealed(&permit, &observation).unwrap(),
            AntiBloatRankingOutcome::InvalidResponse
        );
    }
    let mut malformed = observation.clone();
    malformed.raw = b"malformed {".to_vec();
    assert_eq!(
        p.parse_sealed(&permit, &malformed).unwrap(),
        AntiBloatRankingOutcome::InvalidResponse
    );
    assert!(p.usage_sealed(&permit, &malformed).is_err());
    let mut missing = response();
    missing.as_object_mut().unwrap().remove("usage");
    assert_eq!(
        p.usage_sealed(&permit, &super::tests::observation(missing, Some(200)))
            .unwrap(),
        AntiBloatUsage {
            input_tokens: None,
            output_tokens: None
        }
    );
    let mut abstain = response();
    abstain["answers"]["anti_bloat_order_v1"]["choice"] = json!("ABSTAIN");
    abstain["answers"]["anti_bloat_order_v1"]["probabilities"] =
        json!({"R0":0.04,"R1":0.05,"R2":0.07,"R3":0.09,"R4":0.1,"R5":0.15,"R6":0.2,"ABSTAIN":0.3});
    assert_eq!(
        p.parse_sealed(&permit, &super::tests::observation(abstain, Some(200)))
            .unwrap(),
        AntiBloatRankingOutcome::Abstained
    );
}
