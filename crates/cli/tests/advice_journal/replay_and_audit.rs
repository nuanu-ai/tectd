//! Replays under one idempotency request id, and the journal as the audit reads it.
use crate::support::{self, Fixture, advised, failed, fixture, reply};
use serde_json::json;
use tect_domain::*;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn a_replayed_request_id_returns_the_stored_field_without_a_call_or_a_row() {
    let f = fixture(true).await;
    f.configure(true, Some(5), 0).await;
    let request_id = Uuid::new_v4();
    let work = AdviceWorkRefs {
        request_id: Some(request_id),
        ..AdviceWorkRefs::default()
    };
    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    let first = f.advise("sizing", &work).await.unwrap();
    for _ in 0..2 {
        assert_eq!(f.advise("sizing", &work).await, Some(first.clone()));
    }
    assert_eq!(f.provider.calls(), 1);
    assert_eq!(f.count("advice_journal").await, 1);
    let stored = |route: &'static str, request_id| {
        let (service, context) = (f.service.clone(), f.context.clone());
        async move {
            service
                .stored_advice(&context, route, request_id)
                .await
                .unwrap()
        }
    };
    assert_eq!(
        stored(support::ROUTE, request_id).await,
        Some(first.clone())
    );
    assert_eq!(stored("slice.candidates.save", request_id).await, None);
    assert_eq!(stored(support::ROUTE, Uuid::new_v4()).await, None);
    assert_eq!(
        f.service
            .stored_advice(&f.context, support::ROUTE, Uuid::nil())
            .await,
        Err(Error::InvalidArguments)
    );

    // Another point of the same command is its own decision; the read is newest first.
    f.provider.push_reply(reply(ADVICE_ABSTAIN, 0.1, 0.2, 0.7));
    assert_eq!(
        f.advise("pipeline", &work).await,
        Some(AdviceField::Abstained)
    );
    assert_eq!(f.provider.calls(), 2);
    assert_eq!(f.count("advice_journal").await, 2);
    assert_eq!(f.advise("sizing", &work).await, Some(first));
    assert_eq!(
        stored(support::ROUTE, request_id).await,
        Some(AdviceField::Abstained)
    );

    // A call that was sent and never completed is not sent again on replay.
    let lost = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advice_journal (tenant_id, workspace_id, session_id, point, route, \
             request_id, called, send_certainty) \
         SELECT tenant_id, workspace_id, id, 'sizing', $2, $3, true, 'sent_unknown' \
         FROM agent_sessions WHERE id=$1",
    )
    .bind(f.session)
    .bind(support::ROUTE)
    .bind(lost)
    .execute(&f.pool)
    .await
    .unwrap();
    let lost_work = AdviceWorkRefs {
        request_id: Some(lost),
        ..AdviceWorkRefs::default()
    };
    let replay = f.advise("sizing", &lost_work).await.unwrap();
    assert_eq!(
        replay.reason(),
        Some(AdviceUnavailableReason::TransportError)
    );
    assert_eq!(stored(support::ROUTE, lost).await, None);
    assert_eq!(f.provider.calls(), 2);
    assert_eq!(f.count("advice_journal").await, 3);

    // Disabled again: no field for the replay either.
    f.configure(false, None, 1).await;
    assert_eq!(f.advise("sizing", &work).await, None);
    assert_eq!(stored(support::ROUTE, request_id).await, None);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn journal_pages_filter_by_point_outcome_and_time_and_count_calls_and_skips() {
    let f: Fixture = fixture(true).await;
    f.configure(true, Some(4), 0).await;
    let work = AdviceWorkRefs::default();
    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    f.provider.push_reply(reply(ADVICE_ABSTAIN, 0.1, 0.2, 0.7));
    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    f.provider.push_failure(failed(
        AdviceUnavailableReason::Timeout,
        AdviceSendCertainty::SentUnknown,
        "Jev did not answer within 2000 ms",
        true,
    ));
    for point in ["sizing", "pipeline", "sizing", "sizing", "sizing"] {
        f.advise(point, &work).await.unwrap();
    }
    let audit = |query: serde_json::Value| {
        let (service, context) = (f.service.clone(), f.context.clone());
        async move { service.advice_audit(&context, &support::audit(query)).await }
    };

    let counters = audit(json!({"limit":0,"bodies":true})).await.unwrap();
    assert!(counters.entries.is_empty() && counters.next_after.is_none());
    assert_eq!(
        json!(counters.counters),
        json!({"called":4,"not_called":1,"advised":2,"abstained":1,"unavailable":2,
            "pending":0,"reasons":{"daily_limit_reached":1,"timeout":1}})
    );

    // Newest first, two rows at a time, with a cursor that ends.
    let first = audit(json!({"limit":2,"bodies":true})).await.unwrap();
    assert_eq!(first.counters, counters.counters);
    assert_eq!(
        first
            .entries
            .iter()
            .map(|entry| entry.reason)
            .collect::<Vec<_>>(),
        [
            Some(AdviceUnavailableReason::DailyLimitReached),
            Some(AdviceUnavailableReason::Timeout)
        ]
    );
    assert_eq!(first.next_after, Some(first.entries[1].id));
    let second = audit(json!({"limit":2,"bodies":true,"after":first.next_after}))
        .await
        .unwrap();
    let third = audit(json!({"limit":2,"bodies":true,"after":second.next_after}))
        .await
        .unwrap();
    assert_eq!((second.entries.len(), third.entries.len()), (2, 1));
    assert_eq!(third.next_after, None);
    let mut all: Vec<_> = [&first, &second, &third]
        .iter()
        .flat_map(|page| page.entries.iter())
        .collect();
    assert!(
        all.windows(2)
            .all(|pair| pair[0].created_at >= pair[1].created_at)
    );
    all.dedup_by_key(|entry| entry.id);
    assert_eq!(all.len(), 5);
    let oldest = all[4];
    assert_eq!(oldest.advice, Some(advised("small", 0.7, 0.2, 0.1)));
    assert_eq!(
        oldest.request_body.as_deref(),
        Some("{\"model\":\"jev-latest\"}")
    );
    assert_eq!(oldest.session_id, f.session);
    // Without bodies the same rows come back with no body loaded; their sizes remain.
    let lean = audit(json!({"limit":10,"bodies":false})).await.unwrap();
    assert_eq!(lean.entries.len(), 5);
    assert!(
        lean.entries
            .iter()
            .all(|entry| entry.request_body.is_none() && entry.response_body.is_none())
    );
    assert_eq!(lean.entries[4].id, oldest.id);
    assert_eq!(lean.entries[4].request_bytes, Some(22));
    assert_eq!(lean.entries[4].advice, oldest.advice);
    assert_eq!(lean.counters, counters.counters);
    assert_eq!(
        audit(json!({"limit":2,"bodies":true,"after":Uuid::new_v4()})).await,
        Err(Error::InvalidArguments)
    );

    let pipeline = audit(json!({"limit":10,"bodies":true,"point":"pipeline"}))
        .await
        .unwrap();
    assert_eq!(pipeline.entries.len(), 1);
    assert_eq!(pipeline.entries[0].outcome, Some(AdviceOutcome::Abstained));
    assert_eq!(
        (pipeline.counters.called, pipeline.counters.abstained),
        (1, 1)
    );
    assert_eq!(pipeline.counters.not_called, 0);

    // The outcome filter narrows the rows; the counters still describe the window.
    let unavailable = audit(json!({"limit":10,"bodies":true,"outcome":"unavailable"}))
        .await
        .unwrap();
    assert_eq!(unavailable.entries.len(), 2);
    assert_eq!(unavailable.counters, counters.counters);

    let pivot = all[2].created_at.clone();
    let since = audit(json!({"limit":10,"bodies":true,"since":pivot}))
        .await
        .unwrap();
    let until = audit(json!({"limit":10,"bodies":true,"until":pivot}))
        .await
        .unwrap();
    assert_eq!((since.entries.len(), until.entries.len()), (3, 2));
    assert_eq!((since.counters.called, until.counters.called), (2, 2));
    assert_eq!(since.counters.not_called, 1);

    // Another workspace of the same tenant has its own journal.
    let mut elsewhere = f.peer_context();
    elsewhere.workspace_key = format!("advice-elsewhere-{}", Uuid::new_v4().simple());
    f.service.open_workspace(&elsewhere).await.unwrap();
    let empty = f
        .service
        .advice_audit(
            &elsewhere,
            &support::audit(json!({"limit":10,"bodies":true})),
        )
        .await
        .unwrap();
    assert!(empty.entries.is_empty());
    assert_eq!(empty.counters, AdviceCounters::default());
    assert_eq!(
        f.service
            .advice_audit(
                &elsewhere,
                &support::audit(json!({"limit":2,"bodies":true,"after":all[0].id}))
            )
            .await,
        Err(Error::InvalidArguments)
    );
}
