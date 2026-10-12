use super::*;
use crate::jev_transport::tests::fake_jev;
use serde_json::{Value, json};
use tect_domain::ADVICE_ABSTAIN;

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const KEY: &str = "local-test-key";

fn questions() -> AdviceQuestionSet {
    serde_json::from_value(json!({
        "state": {"task": "Add invitations"},
        "questions": {"size": {"instructions": "Choose the size class.",
            "criteria": {"small": "One sitting.", "large": "Several days.",
                "ABSTAIN": "Not enough to tell."}}}
    }))
    .unwrap()
}

fn answer(choice: &str, small: f64, large: f64, abstain: f64) -> Vec<u8> {
    json!({"model":"jev-2026-10","answers":{"size":{"type":"choice","choice":choice,
        "probabilities":{"small":small,"large":large,"ABSTAIN":abstain},"confidence":0.8}},
        "usage":{"input_tokens":120,"output_tokens":9}})
    .to_string()
    .into_bytes()
}

fn provider(url: &str, timeout_ms: &str) -> JevAdviceProvider {
    let config = JevConfig::from_values(Some(url), Some("jev-latest"), Some(timeout_ms), Some(KEY))
        .unwrap()
        .unwrap();
    JevAdviceProvider::new(config).unwrap()
}

#[test]
fn no_jev_variable_means_no_provider_and_a_stray_key_is_ignored() {
    assert!(
        JevConfig::from_values(None, None, None, None)
            .unwrap()
            .is_none()
    );
    assert!(
        JevConfig::from_values(None, None, None, Some(KEY))
            .unwrap()
            .is_none()
    );
}

#[test]
fn any_jev_variable_requires_all_three_and_the_key_with_no_defaults() {
    let full = (Some(ENDPOINT), Some("jev-latest"), Some("2000"), Some(KEY));
    let config = JevConfig::from_values(full.0, full.1, full.2, full.3)
        .unwrap()
        .unwrap();
    assert_eq!(config.timeout, Duration::from_millis(2000));
    assert_eq!(config.model, "jev-latest");
    for partial in [
        (full.0, None, None, full.3),
        (None, full.1, None, full.3),
        (None, None, full.2, full.3),
        (full.0, full.1, None, full.3),
        (full.0, None, full.2, full.3),
        (None, full.1, full.2, full.3),
        (full.0, full.1, full.2, None),
    ] {
        assert_eq!(
            JevConfig::from_values(partial.0, partial.1, partial.2, partial.3).err(),
            Some(Error::InvalidConfiguration)
        );
    }
    for timeout in ["", "0", "30001", "-1", "+5", "1.5", " 100", "100 ", "2s"] {
        assert_eq!(
            JevConfig::from_values(full.0, full.1, Some(timeout), full.3).err(),
            Some(Error::InvalidConfiguration),
            "{timeout:?}"
        );
    }
    for timeout in ["1", "30000"] {
        assert!(JevConfig::from_values(full.0, full.1, Some(timeout), full.3).is_ok());
    }
    for model in ["", "jev latest", "jev\n", &"m".repeat(129)] {
        assert!(JevConfig::from_values(full.0, Some(model), full.2, full.3).is_err());
    }
    for endpoint in [
        "",
        "http://example.com/v1/systemone",
        "https://api.typesafe.ai/",
    ] {
        assert!(JevConfig::from_values(Some(endpoint), full.1, full.2, full.3).is_err());
    }
    for key in ["", "  ", "line\nbreak"] {
        assert!(JevConfig::from_values(full.0, full.1, full.2, Some(key)).is_err());
    }
}

#[test]
fn debug_output_never_shows_the_key() {
    let config =
        JevConfig::from_values(Some(ENDPOINT), Some("jev-latest"), Some("2000"), Some(KEY))
            .unwrap()
            .unwrap();
    let printed = format!("{config:?}");
    assert!(printed.contains("[redacted]") && printed.contains("jev-latest"));
    assert!(!printed.contains(KEY));
}

#[tokio::test]
async fn an_accepted_answer_returns_answers_usage_and_both_raw_bodies() {
    let body = answer("small", 0.7, 0.2, 0.1);
    let server = fake_jev("200 OK", &body, Duration::ZERO);
    let provider = provider(&server.url, "2000");
    assert!(provider.configured());
    let reply = provider.ask(&questions()).await.unwrap();
    assert_eq!(reply.answers["size"].choice, "small");
    assert_eq!(reply.exchange.model.as_deref(), Some("jev-2026-10"));
    assert_eq!(reply.exchange.input_tokens, Some(120));
    assert_eq!(reply.exchange.output_tokens, Some(9));
    assert!(reply.exchange.latency_ms.is_some());
    assert_eq!(
        reply.exchange.response_body.as_deref(),
        Some(body.as_slice())
    );

    let received = server.request.join().unwrap();
    let end = received.windows(4).position(|w| w == b"\r\n\r\n").unwrap();
    assert_eq!(
        reply.exchange.request_body.as_deref(),
        Some(&received[end + 4..])
    );
    let sent: Value = serde_json::from_slice(&received[end + 4..]).unwrap();
    assert_eq!(sent["model"], "jev-latest");
    assert_eq!(sent["state"], json!({"task":"Add invitations"}));
    assert_eq!(sent["questions"]["size"]["type"], "choice");
    assert!(!String::from_utf8_lossy(&received[end + 4..]).contains(KEY));

    let server = fake_jev(
        "200 OK",
        &answer(ADVICE_ABSTAIN, 0.1, 0.2, 0.7),
        Duration::ZERO,
    );
    let reply = provider_reply(&server.url).await.unwrap();
    assert_eq!(reply.answers["size"].choice, ADVICE_ABSTAIN);
    server.request.join().unwrap();
}

async fn provider_reply(url: &str) -> std::result::Result<AdviceReply, AdviceFailure> {
    provider(url, "2000").ask(&questions()).await
}

#[tokio::test]
async fn every_failure_is_typed_with_its_send_certainty_and_what_is_known() {
    let invalid = answer("large", 0.7, 0.2, 0.1);
    let server = fake_jev("200 OK", &invalid, Duration::ZERO);
    let failure = provider_reply(&server.url).await.unwrap_err();
    assert_eq!(failure.reason, AdviceUnavailableReason::InvalidResponse);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::Sent);
    assert!(failure.detail.contains("not the most probable"));
    assert_eq!(failure.exchange.input_tokens, Some(120));
    assert_eq!(
        failure.exchange.response_body.as_deref(),
        Some(invalid.as_slice())
    );
    assert!(failure.exchange.request_body.is_some());
    server.request.join().unwrap();

    let server = fake_jev("500 Internal Server Error", b"down", Duration::ZERO);
    let failure = provider_reply(&server.url).await.unwrap_err();
    assert_eq!(failure.reason, AdviceUnavailableReason::HttpStatus);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::Sent);
    assert_eq!(failure.detail, "Jev answered with HTTP status 500");
    assert_eq!(failure.exchange.model.as_deref(), Some("jev-latest"));
    assert_eq!(
        (
            failure.exchange.input_tokens,
            failure.exchange.output_tokens
        ),
        (None, None)
    );
    assert_eq!(
        failure.exchange.response_body.as_deref(),
        Some(b"down".as_slice())
    );
    server.request.join().unwrap();

    let server = fake_jev("200 OK", b"{}", Duration::from_millis(150));
    let failure = provider(&server.url, "20")
        .ask(&questions())
        .await
        .unwrap_err();
    assert_eq!(failure.reason, AdviceUnavailableReason::Timeout);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::SentUnknown);
    assert!(failure.exchange.request_body.is_some());
    assert!(failure.exchange.response_body.is_none());
    server.request.join().unwrap();

    let mut oversize = questions();
    oversize.state = json!({"task": "x".repeat(crate::jev_transport::MAX_REQUEST_BYTES)});
    let failure = provider("http://127.0.0.1:1/v1/systemone", "2000")
        .ask(&oversize)
        .await
        .unwrap_err();
    assert_eq!(failure.reason, AdviceUnavailableReason::RequestTooLarge);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::NotSent);
    assert!(failure.exchange.request_body.is_none() && failure.exchange.latency_ms.is_none());
}

#[tokio::test]
async fn a_request_whose_smallest_answer_exceeds_the_answer_limit_is_not_sent() {
    use crate::jev_codec::smallest_answer_bytes;
    let ceiling = crate::jev_transport::MAX_RESPONSE_BYTES;
    // Many small questions: the request stays well under its own limit, while even
    // the shortest acceptable answer to all of them is over the answer limit.
    let asked = |count: usize| {
        let mut set = questions();
        let question = set.questions["size"].clone();
        for index in 0..count {
            set.questions
                .insert(format!("q{index:04}"), question.clone());
        }
        set
    };
    let each = smallest_answer_bytes(&asked(2)) - smallest_answer_bytes(&asked(1));
    let most = (ceiling - smallest_answer_bytes(&asked(0))) / each;
    let (fitting, over) = (asked(most), asked(most + 1));
    assert!(smallest_answer_bytes(&fitting) <= ceiling);
    assert!(smallest_answer_bytes(&over) > ceiling);
    let request = crate::jev_codec::encode_request("jev-latest", &over);
    assert!(request.len() <= crate::jev_transport::MAX_REQUEST_BYTES);

    // Nothing listens on this port: a request that left would be a transport error.
    let unreachable = provider("http://127.0.0.1:1/v1/systemone", "2000");
    let failure = unreachable.ask(&over).await.unwrap_err();
    assert_eq!(failure.reason, AdviceUnavailableReason::RequestTooLarge);
    assert_eq!(failure.send_certainty, AdviceSendCertainty::NotSent);
    assert_eq!(
        failure.detail,
        format!(
            "the smallest acceptable answer of {} bytes would exceed the answer limit of \
             {ceiling} bytes",
            smallest_answer_bytes(&over)
        )
    );
    assert_eq!(failure.exchange.model.as_deref(), Some("jev-latest"));
    assert!(failure.exchange.request_body.is_none() && failure.exchange.latency_ms.is_none());

    // One question fewer could be answered within the limit, so it is sent.
    let failure = unreachable.ask(&fitting).await.unwrap_err();
    assert_eq!(failure.reason, AdviceUnavailableReason::TransportError);
}
