//! One call per advice point: journalled before it is sent, completed after, counted
//! against the daily limit only when the request left.
use crate::support::{self, advised, failed, fixture, reply};
use serde_json::json;
use sqlx::Row;
use tect_domain::*;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn one_call_is_journalled_before_sending_and_completed_after() {
    let f = fixture(true).await;
    f.configure(true, Some(3), 0).await;
    let work = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        program_id: Some(Uuid::new_v4()),
        candidate_set_id: Some(Uuid::new_v4()),
        scope_id: Some(Uuid::new_v4()),
        slice_id: Some(Uuid::new_v4()),
        phase: Some("K1".into()),
    };

    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    let field = f.advise("sizing", &work).await.unwrap();
    assert_eq!(field, advised("small", 0.7, 0.2, 0.1));
    assert_eq!(f.provider.calls(), 1);
    // While the provider was being asked the committed row said: sent, answer unknown.
    assert_eq!(
        f.provider.seen_in_flight(),
        vec![(true, "sent_unknown".to_owned(), true, true)]
    );

    let row = f.newest_row().await;
    assert_eq!(row.get::<Uuid, _>("session_id"), f.session);
    assert_eq!(row.get::<String, _>("point"), "sizing");
    assert_eq!(row.get::<String, _>("route"), support::ROUTE);
    assert_eq!(row.get::<Option<Uuid>, _>("request_id"), work.request_id);
    assert_eq!(row.get::<Option<Uuid>, _>("program_id"), work.program_id);
    assert_eq!(
        row.get::<Option<Uuid>, _>("candidate_set_id"),
        work.candidate_set_id
    );
    assert_eq!(row.get::<Option<Uuid>, _>("scope_id"), work.scope_id);
    assert_eq!(row.get::<Option<Uuid>, _>("slice_id"), work.slice_id);
    assert_eq!(row.get::<Option<String>, _>("phase").as_deref(), Some("K1"));
    assert!(row.get::<bool, _>("called"));
    assert_eq!(
        row.get::<Option<String>, _>("outcome").as_deref(),
        Some("advised")
    );
    assert_eq!(row.get::<Option<String>, _>("reason"), None);
    assert_eq!(row.get::<String, _>("send_certainty"), "sent");
    assert_eq!(
        row.get::<Option<String>, _>("model").as_deref(),
        Some("jev-latest")
    );
    assert_eq!(
        row.get::<Option<Vec<u8>>, _>("request_body").as_deref(),
        Some(support::REQUEST_BODY)
    );
    assert_eq!(
        row.get::<Option<Vec<u8>>, _>("response_body").as_deref(),
        Some(support::RESPONSE_BODY)
    );
    assert_eq!(
        row.get::<Option<i64>, _>("request_bytes"),
        Some(support::REQUEST_BODY.len() as i64)
    );
    assert_eq!(
        row.get::<Option<i64>, _>("response_bytes"),
        Some(support::RESPONSE_BODY.len() as i64)
    );
    assert_eq!(row.get::<Option<i64>, _>("input_tokens"), Some(120));
    // The provider did not report output tokens: unknown stays NULL, never 0.
    assert_eq!(row.get::<Option<i64>, _>("output_tokens"), None);
    assert_eq!(row.get::<Option<i64>, _>("latency_ms"), Some(42));
    assert_eq!(
        row.get::<Option<serde_json::Value>, _>("advice"),
        Some(json!(field))
    );

    f.provider.push_reply(reply(ADVICE_ABSTAIN, 0.1, 0.2, 0.7));
    assert_eq!(
        f.advise("sizing", &AdviceWorkRefs::default()).await,
        Some(AdviceField::Abstained)
    );
    let row = f.newest_row().await;
    assert_eq!(
        row.get::<Option<String>, _>("outcome").as_deref(),
        Some("abstained")
    );
    assert_eq!(row.get::<Option<Uuid>, _>("request_id"), None);
    assert_eq!(row.get::<Option<String>, _>("phase"), None);

    // A provider failure is a reply field with a reason, not an error of the command.
    f.provider.push_failure(failed(
        AdviceUnavailableReason::Timeout,
        AdviceSendCertainty::SentUnknown,
        "Jev did not answer within 2000 ms",
        true,
    ));
    assert_eq!(
        f.advise("sizing", &AdviceWorkRefs::default()).await,
        Some(AdviceField::Unavailable {
            reason: AdviceUnavailableReason::Timeout,
            message: "Jev advice is not available: Jev did not answer within 2000 ms".into(),
        })
    );
    let row = f.newest_row().await;
    assert!(row.get::<bool, _>("called"));
    assert_eq!(
        row.get::<Option<String>, _>("outcome").as_deref(),
        Some("unavailable")
    );
    assert_eq!(
        row.get::<Option<String>, _>("reason").as_deref(),
        Some("timeout")
    );
    assert_eq!(row.get::<String, _>("send_certainty"), "sent_unknown");
    assert!(row.get::<Option<Vec<u8>>, _>("request_body").is_some());
    assert_eq!(row.get::<Option<Vec<u8>>, _>("response_body"), None);
    assert_eq!(row.get::<Option<i64>, _>("response_bytes"), None);
    assert_eq!(row.get::<Option<i64>, _>("input_tokens"), None);
    assert_eq!(row.get::<Option<i64>, _>("latency_ms"), None);

    // Three requests went out today: the fourth point is journalled and not sent.
    assert_eq!(
        f.service
            .advice_config(&f.context)
            .await
            .unwrap()
            .calls_today,
        3
    );
    let limited = f
        .advise("sizing", &AdviceWorkRefs::default())
        .await
        .unwrap();
    assert_eq!(
        limited,
        AdviceField::Unavailable {
            reason: AdviceUnavailableReason::DailyLimitReached,
            message: "Jev advice is not available: the workspace daily call limit of 3 is reached"
                .into(),
        }
    );
    assert_eq!(f.provider.calls(), 3);
    let row = f.newest_row().await;
    assert!(!row.get::<bool, _>("called"));
    assert_eq!(row.get::<String, _>("send_certainty"), "not_sent");
    assert_eq!(
        row.get::<Option<String>, _>("reason").as_deref(),
        Some("daily_limit_reached")
    );
    assert_eq!(row.get::<Option<String>, _>("model"), None);
    assert_eq!(row.get::<Option<i64>, _>("request_bytes"), None);
    assert_eq!(
        row.get::<Option<serde_json::Value>, _>("advice"),
        Some(json!(limited))
    );
    assert_eq!(f.count("advice_journal").await, 4);
    assert_eq!(f.provider.seen_in_flight().len(), 3);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn only_requests_that_left_today_count_against_the_limit() {
    let f = fixture(true).await;
    f.configure(true, Some(2), 0).await;
    // A call from the last second of yesterday, UTC, is outside today's count.
    sqlx::query(
        "INSERT INTO advice_journal (tenant_id, workspace_id, session_id, created_at, point, \
             route, called, outcome, send_certainty, advice) \
         SELECT tenant_id, workspace_id, id, \
                pg_catalog.date_trunc('day', pg_catalog.now(), 'UTC') - interval '1 second', \
                'sizing', $2, true, 'abstained', 'sent', '{\"status\":\"abstained\"}'::jsonb \
         FROM agent_sessions WHERE id=$1",
    )
    .bind(f.session)
    .bind(support::ROUTE)
    .execute(&f.pool)
    .await
    .unwrap();
    assert_eq!(
        f.service
            .advice_config(&f.context)
            .await
            .unwrap()
            .calls_today,
        0
    );

    // A request that never left frees its reservation.
    f.provider.push_failure(failed(
        AdviceUnavailableReason::RequestTooLarge,
        AdviceSendCertainty::NotSent,
        "the request of 300000 bytes exceeds the limit of 262144 bytes",
        false,
    ));
    let field = f
        .advise("sizing", &AdviceWorkRefs::default())
        .await
        .unwrap();
    assert_eq!(
        field.reason(),
        Some(AdviceUnavailableReason::RequestTooLarge)
    );
    let row = f.newest_row().await;
    assert!(!row.get::<bool, _>("called"));
    assert_eq!(row.get::<String, _>("send_certainty"), "not_sent");
    assert_eq!(row.get::<Option<Vec<u8>>, _>("request_body"), None);
    assert_eq!(
        f.service
            .advice_config(&f.context)
            .await
            .unwrap()
            .calls_today,
        0
    );

    // Two concurrent points under a limit of two both go out; a third is refused.
    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    let work = AdviceWorkRefs::default();
    let (first, second, third) = tokio::join!(
        f.advise("sizing", &work),
        f.advise("sizing", &work),
        f.advise("sizing", &work)
    );
    let reasons: Vec<_> = [first, second, third]
        .into_iter()
        .map(|field| field.unwrap().reason())
        .collect();
    assert_eq!(reasons.iter().filter(|reason| reason.is_none()).count(), 2);
    assert_eq!(
        reasons
            .iter()
            .filter(|reason| **reason == Some(AdviceUnavailableReason::DailyLimitReached))
            .count(),
        1
    );
    assert_eq!(f.provider.calls(), 3);
    assert_eq!(
        f.service
            .advice_config(&f.context)
            .await
            .unwrap()
            .calls_today,
        2
    );
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_without_a_provider_journals_the_skip_and_asks_nobody() {
    let f = fixture(false).await;
    let status = f.configure(true, Some(5), 0).await;
    assert!(!status.provider_configured);
    let field = f
        .advise("sizing", &AdviceWorkRefs::default())
        .await
        .unwrap();
    assert_eq!(
        field,
        AdviceField::Unavailable {
            reason: AdviceUnavailableReason::ProviderUnconfigured,
            message: "Jev advice is not available: the daemon has no Jev provider configured"
                .into(),
        }
    );
    assert_eq!(f.provider.calls(), 0);
    let row = f.newest_row().await;
    assert!(!row.get::<bool, _>("called"));
    assert_eq!(row.get::<String, _>("send_certainty"), "not_sent");
    assert_eq!(
        row.get::<Option<String>, _>("reason").as_deref(),
        Some("provider_unconfigured")
    );
    assert_eq!(
        f.service
            .advice_config(&f.context)
            .await
            .unwrap()
            .calls_today,
        0
    );
}
