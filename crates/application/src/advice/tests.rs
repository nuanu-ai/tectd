//! The journalling decision against an in-memory journal. The whole `advise` path,
//! with its two transactions, is covered against PostgreSQL in the CLI test suite.
mod doubles;

use super::*;
use doubles::{HUGE, Journal, Point, Room, SMALL};
use serde_json::json;

fn enabled(limit: i64) -> Option<AdviceConfig> {
    Some(AdviceConfig {
        enabled: true,
        daily_call_limit: Some(limit),
        revision: 1,
        updated_at: "2026-10-12T00:00:00.000000Z".into(),
        updated_by_session: Uuid::new_v4(),
    })
}

fn questions() -> AdviceQuestionSet {
    serde_json::from_value(
        json!({"state":{},"questions":{"size":{"instructions":"Choose.",
        "criteria":{"small":"Small.","ABSTAIN":"Cannot tell."}}}}),
    )
    .unwrap()
}

async fn opened(journal: &mut Journal, configured: bool, work: &AdviceWorkRefs) -> Opening {
    opened_for(journal, configured, work, &SMALL).await
}

async fn opened_for(
    journal: &mut Journal,
    configured: bool,
    work: &AdviceWorkRefs,
    point: &Point,
) -> Opening {
    let questions = questions();
    let request = AdviceRequest {
        point: "sizing",
        route: "scope.candidates.save",
        work,
        questions: &questions,
        shaping: point,
        reply: &Room(1_000),
    };
    open(
        journal,
        Uuid::new_v4(),
        Uuid::new_v4(),
        configured,
        &request,
    )
    .await
    .unwrap()
}

fn reason(opening: &Opening) -> Option<AdviceUnavailableReason> {
    match opening {
        Opening::Settled(advice) => advice.reason(),
        _ => None,
    }
}

#[tokio::test]
async fn disabled_or_absent_setting_writes_nothing() {
    let work = AdviceWorkRefs::default();
    let mut absent = Journal::default();
    assert!(matches!(
        opened(&mut absent, true, &work).await,
        Opening::Disabled
    ));
    let mut disabled = Journal {
        config: enabled(5).map(|config| AdviceConfig {
            enabled: false,
            ..config
        }),
        ..Journal::default()
    };
    assert!(matches!(
        opened(&mut disabled, true, &work).await,
        Opening::Disabled
    ));
    assert!(absent.rows.is_empty() && disabled.rows.is_empty());
    assert!(absent.locked && disabled.locked);
}

#[tokio::test]
async fn unconfigured_provider_and_reached_limit_are_journalled_without_a_call() {
    let work = AdviceWorkRefs::default();
    let mut journal = Journal {
        config: enabled(2),
        calls_today: 2,
        ..Journal::default()
    };
    let unconfigured = opened(&mut journal, false, &work).await;
    assert_eq!(
        reason(&unconfigured),
        Some(AdviceUnavailableReason::ProviderUnconfigured)
    );
    let limited = opened(&mut journal, true, &work).await;
    assert_eq!(
        reason(&limited),
        Some(AdviceUnavailableReason::DailyLimitReached)
    );
    let Opening::Settled(AdviceField::Unavailable { message, .. }) = limited else {
        panic!("expected an unavailable reply field")
    };
    assert_eq!(
        message,
        "Jev advice is not available: the workspace daily call limit of 2 is reached"
    );
    assert_eq!(journal.rows.len(), 2);
    assert!(journal.rows.iter().all(|(_, row)| row.not_called.is_some()));
}

#[tokio::test]
async fn a_field_the_reply_cannot_carry_is_journalled_without_a_call() {
    let work = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    let mut journal = Journal {
        config: enabled(2),
        ..Journal::default()
    };
    let refused = opened_for(&mut journal, true, &work, &HUGE).await;
    assert_eq!(
        reason(&refused),
        Some(AdviceUnavailableReason::AdviceTooLarge)
    );
    let Opening::Settled(AdviceField::Unavailable { message, .. }) = refused else {
        panic!("expected an unavailable reply field")
    };
    assert_eq!(
        message,
        "Jev advice is not available: advice for this draft (40 candidates, 3 included items) \
         would not fit the command reply"
    );
    assert_eq!(journal.rows.len(), 1);
    assert_eq!(
        journal.rows[0]
            .1
            .not_called
            .as_ref()
            .map(AdviceField::reason),
        Some(Some(AdviceUnavailableReason::AdviceTooLarge))
    );
    // The replay is answered from the journal before the reply is measured again.
    let replay = opened_for(&mut journal, true, &work, &SMALL).await;
    assert_eq!(
        reason(&replay),
        Some(AdviceUnavailableReason::AdviceTooLarge)
    );
    assert_eq!(journal.rows.len(), 1);

    // Fixed order: no provider comes before the fit, the fit before the daily limit.
    let other = AdviceWorkRefs::default();
    journal.calls_today = 2;
    assert_eq!(
        reason(&opened_for(&mut journal, false, &other, &HUGE).await),
        Some(AdviceUnavailableReason::ProviderUnconfigured)
    );
    assert_eq!(
        reason(&opened_for(&mut journal, true, &other, &HUGE).await),
        Some(AdviceUnavailableReason::AdviceTooLarge)
    );
    assert_eq!(
        reason(&opened_for(&mut journal, true, &other, &SMALL).await),
        Some(AdviceUnavailableReason::DailyLimitReached)
    );
    assert!(journal.rows.iter().all(|(_, row)| row.not_called.is_some()));

    // A disabled Jev is decided before any of it and writes nothing.
    let mut disabled = Journal::default();
    assert!(matches!(
        opened_for(&mut disabled, true, &other, &HUGE).await,
        Opening::Disabled
    ));
    assert!(disabled.rows.is_empty());
}

#[tokio::test]
async fn the_early_decision_reads_the_setting_and_the_journal_and_writes_nothing() {
    let request_id = Uuid::new_v4();
    let names = ("sizing", "scope.candidates.save");
    let mut journal = Journal::default();
    assert!(matches!(
        earlier(&mut journal, Uuid::new_v4(), false, names, Some(request_id)).await,
        Ok(Earlier::Disabled)
    ));
    assert!(!journal.locked);
    journal.config = enabled(3);
    assert!(matches!(
        earlier(&mut journal, Uuid::new_v4(), false, names, Some(request_id)).await,
        Ok(Earlier::Undecided(config)) if config.daily_call_limit == Some(3)
    ));
    let work = AdviceWorkRefs {
        request_id: Some(request_id),
        ..AdviceWorkRefs::default()
    };
    assert!(matches!(
        opened(&mut journal, true, &work).await,
        Opening::Reserved(_)
    ));
    // The same request id is answered from its row; another point or route is not.
    let stored = earlier(&mut journal, Uuid::new_v4(), false, names, Some(request_id)).await;
    assert!(matches!(
        stored,
        Ok(Earlier::Stored(field))
            if field.reason() == Some(AdviceUnavailableReason::TransportError)
    ));
    for other in [
        ("pipeline", "scope.candidates.save"),
        ("sizing", "slice.candidates.save"),
    ] {
        assert!(matches!(
            earlier(&mut journal, Uuid::new_v4(), false, other, Some(request_id)).await,
            Ok(Earlier::Undecided(_))
        ));
    }
    assert!(matches!(
        earlier(&mut journal, Uuid::new_v4(), false, names, None).await,
        Ok(Earlier::Undecided(_))
    ));
    assert_eq!(journal.rows.len(), 1);
}

#[tokio::test]
async fn a_call_under_the_limit_is_reserved_before_it_is_sent() {
    let mut journal = Journal {
        config: enabled(2),
        calls_today: 1,
        ..Journal::default()
    };
    let work = AdviceWorkRefs {
        program_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    let Opening::Reserved(entry_id) = opened(&mut journal, true, &work).await else {
        panic!("expected a reserved call")
    };
    let (id, row) = &journal.rows[0];
    assert_eq!(*id, entry_id);
    assert_eq!(
        (row.point.as_str(), row.route.as_str()),
        ("sizing", "scope.candidates.save")
    );
    assert_eq!(row.work, work);
    assert_eq!(row.not_called, None);
}

#[tokio::test]
async fn a_repeated_request_id_returns_the_stored_reply_and_adds_no_row() {
    let work = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    let mut journal = Journal {
        config: enabled(1),
        calls_today: 1,
        ..Journal::default()
    };
    let first = opened(&mut journal, true, &work).await;
    assert_eq!(
        reason(&first),
        Some(AdviceUnavailableReason::DailyLimitReached)
    );
    journal.calls_today = 0;
    let replay = opened(&mut journal, true, &work).await;
    assert_eq!(
        reason(&replay),
        Some(AdviceUnavailableReason::DailyLimitReached)
    );
    assert_eq!(journal.rows.len(), 1);

    // A reserved call that never completed is not sent again either.
    let pending = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    assert!(matches!(
        opened(&mut journal, true, &pending).await,
        Opening::Reserved(_)
    ));
    let replay = opened(&mut journal, true, &pending).await;
    assert_eq!(
        reason(&replay),
        Some(AdviceUnavailableReason::TransportError)
    );
    assert_eq!(journal.rows.len(), 2);

    // Another request id is a new decision.
    let other = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    assert!(matches!(
        opened(&mut journal, true, &other).await,
        Opening::Reserved(_)
    ));
    assert_eq!(journal.rows.len(), 3);
}

#[tokio::test]
async fn disabled_provider_reports_itself_unconfigured() {
    let provider = crate::DisabledAdviceProvider;
    assert!(!crate::AdviceProvider::configured(&provider));
    let failure = crate::AdviceProvider::ask(&provider, &questions())
        .await
        .unwrap_err();
    assert_eq!(
        failure.reason,
        AdviceUnavailableReason::ProviderUnconfigured
    );
    assert_eq!(failure.send_certainty, AdviceSendCertainty::NotSent);
}
