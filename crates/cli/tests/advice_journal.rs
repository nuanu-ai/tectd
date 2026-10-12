//! Real application + non-owner PostgreSQL contracts of the Jev advice setting,
//! the journal and the generic advise path, with a scripted provider.
#[path = "advice_journal/calls.rs"]
mod calls;
#[path = "advice_journal/replay_and_audit.rs"]
mod replay_and_audit;
#[path = "advice_journal/support.rs"]
mod support;

use serde_json::json;
use support::{fixture, questions, reply};
use tect_application::{AdviceAsk, AdviceRequest};
use tect_domain::*;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setting_is_revision_checked_and_enabling_needs_a_stated_limit() {
    let f = fixture(true).await;
    let other_session = f.peer_context();
    assert_eq!(
        f.service.advice_config(&other_session).await,
        Err(Error::WorkspaceNotOpen)
    );
    let absent = f.service.advice_config(&f.context).await.unwrap();
    assert_eq!(
        absent,
        AdviceConfigStatus {
            config: None,
            calls_today: 0,
            provider_configured: true
        }
    );

    let configure = |enabled, daily_call_limit, expected_revision| {
        let (service, context) = (f.service.clone(), f.context.clone());
        async move {
            service
                .configure_advice(
                    &context,
                    &ConfigureAdvice {
                        enabled,
                        daily_call_limit,
                        expected_revision,
                    },
                )
                .await
                .map(|status| status.config.unwrap())
        }
    };
    assert_eq!(configure(true, None, 0).await, Err(Error::InvalidArguments));
    assert_eq!(f.count("advice_config").await, 0);

    // Disabling a workspace that never had a setting records it without a limit.
    let first = configure(false, None, 0).await.unwrap();
    assert_eq!(
        (first.enabled, first.daily_call_limit, first.revision),
        (false, None, 1)
    );
    assert_eq!(first.updated_by_session, f.session);
    assert!(first.updated_at.ends_with('Z') && first.updated_at.contains('T'));

    assert_eq!(configure(true, Some(3), 0).await, Err(Error::StaleRevision));
    assert_eq!(
        Error::StaleRevision.refusal().unwrap().code,
        RefusalCode::StaleRevision
    );
    let enabled = configure(true, Some(3), 1).await.unwrap();
    assert_eq!(
        (enabled.enabled, enabled.daily_call_limit, enabled.revision),
        (true, Some(3), 2)
    );
    // Disabling keeps the stored limit; a stale writer changes nothing.
    let disabled = configure(false, None, 2).await.unwrap();
    assert_eq!(
        (
            disabled.enabled,
            disabled.daily_call_limit,
            disabled.revision
        ),
        (false, Some(3), 3)
    );
    assert_eq!(configure(true, Some(9), 2).await, Err(Error::StaleRevision));
    assert_eq!(configure(true, Some(9), 4).await, Err(Error::StaleRevision));
    let read = f.service.advice_config(&f.context).await.unwrap();
    assert_eq!(read.config, Some(disabled));
    assert_eq!(f.count("advice_config").await, 1);

    // A second native session of the same workspace may change the setting.
    f.service.open_workspace(&other_session).await.unwrap();
    let by_peer = f
        .service
        .configure_advice(
            &other_session,
            &ConfigureAdvice {
                enabled: true,
                daily_call_limit: Some(7),
                expected_revision: 3,
            },
        )
        .await
        .unwrap()
        .config
        .unwrap();
    assert_eq!((by_peer.revision, by_peer.daily_call_limit), (4, Some(7)));
    assert_ne!(by_peer.updated_by_session, f.session);

    // The database itself refuses an enabled setting without a limit.
    let refused = sqlx::query(
        "UPDATE advice_config SET daily_call_limit=NULL WHERE tenant_id=$1 AND enabled",
    )
    .bind(f.tenant)
    .execute(&f.pool)
    .await;
    assert!(refused.is_err());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn concurrent_first_settings_have_one_winner_and_tenants_are_isolated() {
    let f = fixture(true).await;
    let mut tasks = Vec::new();
    for limit in 1..=8 {
        let (service, context) = (f.service.clone(), f.context.clone());
        tasks.push(tokio::spawn(async move {
            service
                .configure_advice(
                    &context,
                    &ConfigureAdvice {
                        enabled: true,
                        daily_call_limit: Some(limit),
                        expected_revision: 0,
                    },
                )
                .await
        }));
    }
    let mut winners = 0;
    for task in tasks {
        match task.await.unwrap() {
            Ok(status) => {
                winners += 1;
                assert_eq!(status.config.unwrap().revision, 1);
            }
            Err(error) => assert_eq!(error, Error::StaleRevision),
        }
    }
    assert_eq!(winners, 1);

    // Another tenant with the same workspace key sees no setting and no journal.
    let stranger = fixture(true).await;
    let mut context = stranger.context.clone();
    context.workspace_key = f.context.workspace_key.clone();
    context.native_session_id = Uuid::new_v4().to_string();
    stranger.service.open_workspace(&context).await.unwrap();
    assert_eq!(
        stranger
            .service
            .advice_config(&context)
            .await
            .unwrap()
            .config,
        None
    );
    f.provider.push_reply(reply("small", 0.7, 0.2, 0.1));
    assert!(
        f.advise("sizing", &AdviceWorkRefs::default())
            .await
            .is_some()
    );
    let page = stranger
        .service
        .advice_audit(&context, &support::audit(json!({"limit":10,"bodies":true})))
        .await
        .unwrap();
    assert!(page.entries.is_empty());
    assert_eq!(page.counters, AdviceCounters::default());

    // The runtime role sees no row without a tenant and may not delete or rewrite one.
    let runtime = sqlx::PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM advice_journal")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(visible, 0);
    for forbidden in [
        "DELETE FROM advice_journal",
        "UPDATE advice_journal SET point='rewritten'",
        "DELETE FROM advice_config",
    ] {
        assert!(sqlx::query(forbidden).execute(&runtime).await.is_err());
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn disabled_jev_returns_no_field_and_writes_nothing() {
    let f = fixture(true).await;
    let work = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    // No setting at all: exactly the state of a workspace on main.
    assert_eq!(f.advise("sizing", &work).await, None);
    assert_eq!(f.count("advice_config").await, 0);
    assert_eq!(f.count("advice_journal").await, 0);

    f.configure(false, None, 0).await;
    let before = f.versions().await;
    assert_eq!(f.advise("sizing", &work).await, None);
    assert_eq!(
        f.service
            .stored_advice(&f.context, support::ROUTE, work.request_id.unwrap())
            .await,
        Ok(None)
    );
    assert_eq!(f.versions().await, before);
    assert_eq!(f.count("advice_journal").await, 0);
    assert_eq!(f.provider.calls(), 0);

    // Invalid point input is the caller's error, never a provider outcome.
    let questions = questions();
    let request = AdviceRequest {
        point: "Sizing Point",
        route: support::ROUTE,
        work: &work,
        ask: AdviceAsk::Questions {
            questions: &questions,
            shaping: &support::Verbatim,
        },
        reply: &support::Room(usize::MAX),
    };
    assert_eq!(
        f.service.advise(&f.context, &request).await,
        Err(Error::InvalidArguments)
    );
}
