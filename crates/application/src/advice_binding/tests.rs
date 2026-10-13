//! The review gate against an in-memory store. The whole path, inside the review's
//! own transaction, is covered against PostgreSQL in the CLI test suite.
use super::*;
use crate::advice::tests::doubles::Journal;
use serde_json::json;

fn setting(enabled: bool) -> Option<AdviceConfig> {
    Some(AdviceConfig {
        enabled,
        daily_call_limit: Some(5),
        revision: 1,
        updated_at: "2026-10-13T00:00:00.000000Z".into(),
        updated_by_session: Uuid::new_v4(),
    })
}

/// One kept answer that asks for the first candidate, at revision 1, to be split.
fn store(config: Option<AdviceConfig>) -> Journal {
    Journal {
        config,
        open: vec![OpenAdvice {
            id: Uuid::from_u128(10),
            candidate_id: Uuid::from_u128(1),
            candidate_revision: 1,
            point: BindingPoint::Sizing,
            answer: json!({"advice":"split","extra_items":[]}),
        }],
        ..Journal::default()
    }
}

fn reviewed(ready: bool, departures: &[AdviceDeparture]) -> Reviewed<'_> {
    Reviewed {
        level: SizingLevel::ScopeToSlices,
        candidate_set_id: Uuid::from_u128(5),
        draft: Some(vec![(Uuid::from_u128(1), 1), (Uuid::from_u128(2), 1)]),
        ready,
        departures,
    }
}

fn departing() -> Vec<AdviceDeparture> {
    vec![AdviceDeparture {
        candidate_id: Uuid::from_u128(1),
        point: BindingPoint::Sizing,
        reason: "It is one outcome".into(),
    }]
}

#[tokio::test]
async fn a_disabled_jev_is_not_looked_at_and_nothing_is_written() {
    let stated = departing();
    for config in [None, setting(false)] {
        for review in [reviewed(true, &[]), reviewed(true, &stated)] {
            let mut journal = store(config.clone());
            assert_eq!(hold(&mut journal, Uuid::new_v4(), &review).await, Ok(()));
            // The setting was read without a lock, and nothing else was touched.
            assert!(!journal.locked);
            assert!(journal.open_reads.is_empty() && journal.departed.is_empty());
        }
    }
}

#[tokio::test]
async fn a_ready_review_is_refused_while_advice_is_open_and_records_nothing() {
    let mut journal = store(setting(true));
    let error = hold(&mut journal, Uuid::new_v4(), &reviewed(true, &[]))
        .await
        .unwrap_err();
    assert_eq!(error.code(), "ADVICE_NOT_FOLLOWED");
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.rule.as_deref(), Some("SLICE-REVIEW-ADVICE-OPEN"));
    assert!(refusal.actual.unwrap().contains("sizing: split"));
    // The open answers of this candidate set, at this level, were what it read.
    assert_eq!(
        journal.open_reads,
        [(SizingLevel::ScopeToSlices, Uuid::from_u128(5))]
    );
    assert!(journal.departed.is_empty());
}

#[tokio::test]
async fn a_passing_review_records_the_departures_it_states() {
    let stated = departing();
    // Ready with a departure for the open item, and a review that does not say ready.
    for ready in [true, false] {
        let mut journal = store(setting(true));
        let review = reviewed(ready, &stated);
        assert_eq!(hold(&mut journal, Uuid::new_v4(), &review).await, Ok(()));
        assert_eq!(
            journal.departed,
            [(Uuid::from_u128(10), "It is one outcome".to_owned())]
        );
    }
    // Not ready and nothing stated: it passes with the advice still open.
    let mut journal = store(setting(true));
    let review = reviewed(false, &[]);
    assert_eq!(hold(&mut journal, Uuid::new_v4(), &review).await, Ok(()));
    assert!(journal.departed.is_empty());
    // Nothing open: a ready review passes.
    let mut journal = Journal {
        config: setting(true),
        ..Journal::default()
    };
    let review = reviewed(true, &stated);
    assert_eq!(hold(&mut journal, Uuid::new_v4(), &review).await, Ok(()));
    assert!(journal.departed.is_empty());
}
