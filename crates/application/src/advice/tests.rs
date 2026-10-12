//! The journalling decision against an in-memory journal. The whole `advise` path,
//! with its two transactions, is covered against PostgreSQL in the CLI test suite.
use super::*;
use async_trait::async_trait;
use serde_json::json;

#[derive(Default)]
struct Journal {
    config: Option<AdviceConfig>,
    calls_today: i64,
    rows: Vec<(Uuid, NewAdviceEntry)>,
    locked: bool,
}

#[async_trait]
impl AdviceStore for Journal {
    async fn advice_config(&mut self, _: Uuid, for_update: bool) -> Result<Option<AdviceConfig>> {
        self.locked = for_update;
        Ok(self.config.clone())
    }
    async fn configure_advice(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: &ConfigureAdvice,
    ) -> Result<AdviceConfig> {
        unreachable!()
    }
    async fn advice_calls_today(&mut self, _: Uuid) -> Result<i64> {
        Ok(self.calls_today)
    }
    async fn insert_advice_entry(
        &mut self,
        _: Uuid,
        _: Uuid,
        entry: &NewAdviceEntry,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.rows.push((id, entry.clone()));
        Ok(id)
    }
    async fn complete_advice_entry(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: &AdviceCompletion,
    ) -> Result<()> {
        unreachable!()
    }
    async fn advice_entry_for_request(
        &mut self,
        workspace_id: Uuid,
        route: &str,
        point: Option<&str>,
        request_id: Uuid,
    ) -> Result<Option<AdviceJournalEntry>> {
        Ok(self
            .rows
            .iter()
            .rev()
            .find(|(_, row)| {
                row.route == route
                    && point.is_none_or(|point| row.point == point)
                    && row.work.request_id == Some(request_id)
            })
            .map(|(id, row)| AdviceJournalEntry {
                id: *id,
                created_at: "2026-10-12T00:00:00.000000Z".into(),
                workspace_id,
                session_id: Uuid::new_v4(),
                point: row.point.clone(),
                route: row.route.clone(),
                work: row.work.clone(),
                called: row.not_called.is_none(),
                outcome: row.not_called.as_ref().map(AdviceField::outcome),
                reason: row.not_called.as_ref().and_then(AdviceField::reason),
                send_certainty: if row.not_called.is_none() {
                    AdviceSendCertainty::SentUnknown
                } else {
                    AdviceSendCertainty::NotSent
                },
                model: None,
                request_bytes: None,
                response_bytes: None,
                input_tokens: None,
                output_tokens: None,
                latency_ms: None,
                request_body: None,
                response_body: None,
                advice: row.not_called.clone(),
            }))
    }
    async fn advice_entries(
        &mut self,
        _: Uuid,
        _: &AdviceAuditQuery,
        _: u32,
    ) -> Result<Vec<AdviceJournalEntry>> {
        unreachable!()
    }
    async fn advice_counters(&mut self, _: Uuid, _: &AdviceAuditQuery) -> Result<AdviceCounters> {
        unreachable!()
    }
}

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
    let questions = questions();
    let request = AdviceRequest {
        point: "sizing",
        route: "scope.candidates.save",
        work,
        questions: &questions,
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
