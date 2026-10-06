use super::*;
use tect_application::AdvisoryProviderReceiptUsage;
use tect_domain::{AdvisoryDispatchState, ComparativeDisposition, ScopeAdviceChoice};

#[test]
fn sealed_selected_and_abstain_use_validated_nonempty_context_without_mutation() {
    let context = helpers::native_context();
    for abstain in [false, true] {
        let provider = helpers::synthetic_provider();
        let prepared = provider.prepare_context(&context).unwrap();
        assert!(provider.prepared_matches_context(&context, &prepared));
        let saved = helpers::synthetic_saved(
            &prepared,
            helpers::raw_response(
                serde_json::to_vec(&helpers::native_response(context.request(), abstain)).unwrap(),
            ),
        );
        let before = saved.observation.clone();
        let answers = super::super::sealed::parse(&provider, &prepared, &saved).unwrap();
        assert_eq!(answers.answers.len(), context.request().alternatives.len());
        if abstain {
            assert_eq!(
                answers.comparative_disposition,
                Some(ComparativeDisposition::Abstain)
            );
            assert!(
                answers
                    .answers
                    .iter()
                    .all(|a| a.choice == ScopeAdviceChoice::NonPreferred)
            );
        } else {
            let mut ids = context
                .request()
                .alternatives
                .iter()
                .map(|a| a.id.clone())
                .collect::<Vec<_>>();
            ids.sort();
            assert_eq!(
                answers.comparative_disposition,
                Some(ComparativeDisposition::Selected(ids[0].clone()))
            );
            assert_eq!(
                answers
                    .answers
                    .iter()
                    .filter(|a| a.choice == ScopeAdviceChoice::Preferred)
                    .count(),
                1
            );
        }
        assert_eq!(saved.observation, before);
    }
}

#[test]
fn sealed_v2_none_historical_body_has_new_snapshot_and_no_new_send_fallback() {
    let provider = helpers::synthetic_provider();
    let context = helpers::native_context();
    let body =
        wire::serialize_request_v2("jev-1.13.0", context.request(), context.emitted()).unwrap();
    let prepared = PreparedScopeAdviceAttempt::new(
        context.request().clone(),
        body.clone(),
        "fixture".into(),
        "jev-1.13.0".into(),
        provider.config.endpoint.as_str().into(),
        LEGACY_WIRE_FORMAT.into(),
    )
    .unwrap();
    assert_eq!(prepared.body(), body);
    assert!(provider.prepared_matches_context(&context, &prepared));
    assert!(provider.matches_legacy_prepared(context.request(), context.emitted(), &prepared));
    assert_ne!(provider.prepare_context(&context).unwrap(), prepared);
    let response = serde_json::to_vec(&helpers::legacy_response(context.request())).unwrap();
    let saved = helpers::synthetic_saved(&prepared, helpers::raw_response(response.clone()));
    assert_eq!(saved.configuration_snapshot.as_object().unwrap().len(), 9);
    let expected = wire::parse_response_v2(&response, "jev-1.13.0", context.request())
        .unwrap()
        .answers;
    let answers = super::super::sealed::parse(&provider, &prepared, &saved).unwrap();
    assert_eq!(answers, expected);
    assert_eq!(answers.comparative_disposition, None);
    assert!(
        serde_json::to_value(&answers)
            .unwrap()
            .get("comparative_disposition")
            .is_none()
    );
    assert_eq!(
        serde_json::to_vec(&answers).unwrap(),
        serde_json::to_vec(&expected).unwrap()
    );
    let usage = super::super::sealed::usage(&provider, &saved).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(11), Some(5))
    );
}

#[test]
fn complete_usage_is_independent_known_zero_and_ambiguous_unknown() {
    for state in [
        AdvisoryDispatchState::Sending,
        AdvisoryDispatchState::Sealed,
    ] {
        let (provider, _, mut saved) = helpers::saved_fixture();
        saved.dispatch.state = state;
        for (bytes, expected) in [
            (
                br#"{"usage":{"input_tokens":11,"output_tokens":5}}"#.as_slice(),
                (Some(11), Some(5)),
            ),
            (
                br#"{"usage":{"input_tokens":0,"output_tokens":0}}"#.as_slice(),
                (Some(0), Some(0)),
            ),
            (
                br#"{"usage":{"input_tokens":-1,"output_tokens":-1}}"#.as_slice(),
                (None, None),
            ),
            (
                br#"{"usage":{"input_tokens":1.5,"output_tokens":2.5}}"#.as_slice(),
                (None, None),
            ),
            (br#"{"usage":{}}"#.as_slice(), (None, None)),
            (br#"{}"#.as_slice(), (None, None)),
            (
                br#"{"usage":{"input_tokens":11}}"#.as_slice(),
                (Some(11), None),
            ),
            (
                br#"{"usage":{"input_tokens":11,"input_tokens":0,"output_tokens":5}}"#.as_slice(),
                (None, None),
            ),
            (
                br#"{"usage":{"input_tokens":11},"usage":{"output_tokens":5}}"#.as_slice(),
                (None, None),
            ),
            (b"{".as_slice(), (None, None)),
        ] {
            saved.observation.as_mut().unwrap().response_payload = Some(bytes.to_vec());
            let before = saved.observation.clone();
            let usage = super::super::sealed::usage(&provider, &saved).unwrap();
            assert_eq!((usage.input_tokens, usage.output_tokens), expected);
            assert_eq!(saved.observation, before);
        }
        saved.observation.as_mut().unwrap().response_payload =
            Some(br#"{"usage":{"input_tokens":11,"output_tokens":5}}"#.to_vec());
        saved.observation.as_mut().unwrap().response_complete = false;
        assert_eq!(
            super::super::sealed::usage(&provider, &saved).unwrap(),
            AdvisoryProviderReceiptUsage::default()
        );
        saved.observation.as_mut().unwrap().response_complete = true;
        saved.observation.as_mut().unwrap().response_payload = None;
        assert_eq!(
            super::super::sealed::usage(&provider, &saved).unwrap(),
            AdvisoryProviderReceiptUsage::default()
        );
        saved.observation = None;
        assert_eq!(
            super::super::sealed::usage(&provider, &saved).unwrap(),
            AdvisoryProviderReceiptUsage::default()
        );
    }
}

#[test]
fn invalid_answers_keep_known_usage_and_original_raw_context_immutable() {
    let (provider, prepared, mut saved) = helpers::saved_fixture();
    let mut response = helpers::native_response(prepared.request(), false);
    response["answers"]["choice_v3"]["choice"] = json!("UNKNOWN");
    saved.observation.as_mut().unwrap().response_payload =
        Some(serde_json::to_vec(&response).unwrap());
    let before = saved.observation.clone();
    let usage = super::super::sealed::usage(&provider, &saved).unwrap();
    assert_eq!(
        (usage.input_tokens, usage.output_tokens),
        (Some(11), Some(5))
    );
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());
    assert_eq!(saved.observation, before);
    let context = saved
        .observation
        .as_ref()
        .unwrap()
        .original_transport_context
        .as_ref()
        .unwrap();
    assert_eq!(context.outcome, AdvisoryDispatchOutcome::ProviderResponse);
    assert_eq!(context.provider_failure_code, None);
}

#[test]
fn partial_status_identity_snapshot_and_emitted_tampering_never_parse() {
    let (provider, prepared, mut saved) = helpers::saved_fixture();
    saved.observation.as_mut().unwrap().response_complete = false;
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());
    saved.observation.as_mut().unwrap().response_complete = true;
    saved.observation.as_mut().unwrap().http_status = Some(500);
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());
    saved.observation.as_mut().unwrap().http_status = Some(200);
    saved.dispatch.model = "other".into();
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());
    assert!(super::super::sealed::usage(&provider, &saved).is_err());
    saved.dispatch.model = "jev-1.13.0".into();
    saved
        .configuration_snapshot
        .as_object_mut()
        .unwrap()
        .remove("budget_policy");
    saved.dispatch.configuration_digest = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&saved.configuration_snapshot).unwrap())
    );
    assert!(super::super::sealed::parse(&provider, &prepared, &saved).is_err());

    let context = helpers::native_context();
    for version in [WIRE_FORMAT, LEGACY_WIRE_FORMAT] {
        let canonical = if version == WIRE_FORMAT {
            wire::serialize_request("jev-1.13.0", context.request(), context.emitted())
        } else {
            wire::serialize_request_v2("jev-1.13.0", context.request(), context.emitted())
        }
        .unwrap();
        let mut body: Value = serde_json::from_slice(&canonical).unwrap();
        body["state"]["emitted"][0]["material_digest"] = json!("0".repeat(64));
        let bad = PreparedScopeAdviceAttempt::new(
            context.request().clone(),
            serde_json::to_vec(&body).unwrap(),
            "fixture".into(),
            "jev-1.13.0".into(),
            provider.config.endpoint.as_str().into(),
            version.into(),
        )
        .unwrap();
        let saved = helpers::synthetic_saved(
            &bad,
            helpers::raw_response(
                serde_json::to_vec(&helpers::native_response(context.request(), false)).unwrap(),
            ),
        );
        assert!(!provider.prepared_matches_context(&context, &bad));
        assert!(super::super::sealed::parse(&provider, &bad, &saved).is_err());
    }
}
