/// Storage representation only; the application contract stays plain typed Rust.
#[derive(serde::Serialize, serde::Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderTransportContextJson {
    send_certainty: AdvisorySendCertainty,
    outcome: AdvisoryDispatchOutcome,
    raw_response_ref: Option<String>,
    provider_failure_code: Option<String>,
}

impl From<&AdvisoryProviderTransportContext> for ProviderTransportContextJson {
    fn from(value: &AdvisoryProviderTransportContext) -> Self {
        Self {
            send_certainty: value.send_certainty,
            outcome: value.outcome,
            raw_response_ref: value.raw_response_ref.clone(),
            provider_failure_code: value.provider_failure_code.clone(),
        }
    }
}

impl From<ProviderTransportContextJson> for AdvisoryProviderTransportContext {
    fn from(value: ProviderTransportContextJson) -> Self {
        Self {
            send_certainty: value.send_certainty,
            outcome: value.outcome,
            raw_response_ref: value.raw_response_ref,
            provider_failure_code: value.provider_failure_code,
        }
    }
}

pub(crate) fn encode_transport_context(value: &AdvisoryProviderTransportContext) -> Result<serde_json::Value> {
    serde_json::to_value(ProviderTransportContextJson::from(value)).map_err(storage_error)
}

pub(crate) fn decode_transport_context(value: serde_json::Value) -> Result<AdvisoryProviderTransportContext> {
    serde_json::from_value::<ProviderTransportContextJson>(value)
        .map(Into::into)
        .map_err(storage_error)
}

#[cfg(test)]
mod provider_transport_context_tests {
    use super::*;

    #[test]
    fn typed_storage_roundtrip_preserves_opaque_transport_metadata() {
        let original = AdvisoryProviderTransportContext {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            raw_response_ref: Some("original-not-a-parsed-protocol".to_owned()),
            provider_failure_code: Some("opaque-future-provider-code".to_owned()),
        };
        let json = encode_transport_context(&original).unwrap();
        assert_eq!(decode_transport_context(json.clone()).unwrap(), original);
        let mut extra = json;
        extra["answers"] = serde_json::json!([]);
        assert!(decode_transport_context(extra).is_err());
    }

    #[test]
    fn optional_transport_metadata_preserves_historical_unknown() {
        let original: Option<AdvisoryProviderTransportContext> = None;
        let stored = original.as_ref().map(encode_transport_context).transpose().unwrap();
        assert_eq!(stored, None);
        assert_eq!(stored.map(decode_transport_context).transpose().unwrap(), None);
    }

    #[test]
    fn complete_and_partial_receipt_contexts_roundtrip_without_inference() {
        let raw = Some(b"retained original bytes".to_vec());
        for outcome in [AdvisoryDispatchOutcome::ProviderResponse, AdvisoryDispatchOutcome::ProviderFailure] {
            let original = AdvisoryProviderTransportContext {
                send_certainty: AdvisorySendCertainty::Sent,
                outcome,
                raw_response_ref: Some("sha256:retained-original".to_owned()),
                provider_failure_code: (outcome == AdvisoryDispatchOutcome::ProviderFailure)
                    .then(|| "response-oversize".to_owned()),
            };
            original.validate_for(&raw).unwrap();
            assert_eq!(decode_transport_context(encode_transport_context(&original).unwrap()).unwrap(), original);
        }
    }
}
