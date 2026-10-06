use super::*;
use crate::{AntiBloatRankingOutcome, AntiBloatUsage};

struct LifecycleProvider {
    native: bool,
    usage_error: Option<Error>,
    outcome: AntiBloatRankingOutcome,
    usages: AtomicUsize,
    parses: AtomicUsize,
}
#[async_trait]
impl AntiBloatRankingProvider for LifecycleProvider {
    fn required_profile(&self) -> Option<&str> {
        self.native.then_some("native-test")
    }
    fn usage_sealed(
        &self,
        _: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<AntiBloatUsage> {
        self.usages.fetch_add(1, Ordering::SeqCst);
        assert_eq!(observation.raw, b"true HTTP raw body");
        if let Some(error) = &self.usage_error {
            return Err(error.clone());
        }
        Ok(AntiBloatUsage {
            input_tokens: Some(2),
            output_tokens: Some(3),
        })
    }
    fn parse_sealed(
        &self,
        _: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<AntiBloatRankingOutcome> {
        self.parses.fetch_add(1, Ordering::SeqCst);
        assert_eq!(observation.raw, b"true HTTP raw body");
        Ok(self.outcome.clone())
    }
    async fn rank(
        &self,
        started: &crate::AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        started.claim()?;
        Err(Error::Forbidden)
    }
}

#[tokio::test]
async fn usage_requires_seal_and_preserves_transport_raw_elapsed_and_terminal_replay() {
    for outcome in [
        AntiBloatRankingOutcome::Abstained,
        AntiBloatRankingOutcome::InvalidResponse,
    ] {
        let mut generic = app(true, false);
        let saved = prepare(
            &mut generic,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await;
        let mut app = AntiBloatApplication {
            store: generic.store,
            provider: LifecycleProvider {
                native: false,
                usage_error: None,
                outcome: outcome.clone(),
                usages: AtomicUsize::new(0),
                parses: AtomicUsize::new(0),
            },
        };
        let permit = app
            .prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .unwrap();
        let transport = AntiBloatProviderObservation {
            response_complete: None,
            original_transport_context: None,
            http_status: None,
            raw: b"true HTTP raw body".to_vec(),
            input_tokens: None,
            output_tokens: None,
            elapsed_monotonic_ms: Some(7),
        };
        assert_eq!(
            observe_sealed_usage(&mut app.store, &app.provider, &permit, &transport).await,
            Err(Error::InputConflict)
        );
        assert_eq!(app.provider.usages.load(Ordering::SeqCst), 0);
        app.seal_response(&permit, &transport).await.unwrap();
        let (usage, invalid) =
            observe_sealed_usage(&mut app.store, &app.provider, &permit, &transport)
                .await
                .unwrap();
        assert!(!invalid);
        assert_eq!(usage.raw, transport.raw);
        assert_eq!(usage.elapsed_monotonic_ms, Some(7));
        assert_eq!(
            app.finalize_response(&permit, &usage.raw).await,
            Err(Error::InputConflict)
        );
        assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
        assert!(!app.store.consume_budget(&permit, &usage).await.unwrap());
        let expected = if outcome == AntiBloatRankingOutcome::Abstained {
            AntiBloatAttemptState::ProviderAbstained
        } else {
            AntiBloatAttemptState::InvalidResponse
        };
        assert_eq!(
            app.finalize_response(&permit, &usage.raw).await.unwrap(),
            expected
        );
        assert!(
            app.prepare_send(saved.review_id)
                .await
                .unwrap()
                .permit
                .is_none()
        );
        assert_eq!(
            app.finalize_response(&permit, &usage.raw).await,
            Err(Error::InputConflict)
        );
        assert_eq!(app.provider.parses.load(Ordering::SeqCst), 1);
        assert_eq!(app.store.sends, 1);
        assert_eq!(app.store.raw_response, Some(transport.raw));
    }
}

#[tokio::test]
async fn invalid_usage_is_unknown_consumption_and_invalid_terminal_without_ranking() {
    for error in [Error::InputConflict, Error::InvalidArguments] {
        let mut generic = app(true, false);
        let saved = prepare(
            &mut generic,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await;
        let mut app = AntiBloatApplication {
            store: generic.store,
            provider: LifecycleProvider {
                native: false,
                usage_error: Some(error),
                outcome: AntiBloatRankingOutcome::Abstained,
                usages: AtomicUsize::new(0),
                parses: AtomicUsize::new(0),
            },
        };
        let permit = app
            .prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .unwrap();
        let transport = AntiBloatProviderObservation {
            response_complete: None,
            original_transport_context: None,
            http_status: Some(200),
            raw: b"true HTTP raw body".to_vec(),
            input_tokens: None,
            output_tokens: None,
            elapsed_monotonic_ms: Some(7),
        };
        app.seal_response(&permit, &transport).await.unwrap();
        let (observation, invalid) =
            observe_sealed_usage(&mut app.store, &app.provider, &permit, &transport)
                .await
                .unwrap();
        assert!(invalid);
        assert_eq!(observation.input_tokens, None);
        assert_eq!(observation.output_tokens, None);
        assert!(app.store.consumed.is_none());
        assert_eq!(
            app.finalize_response(&permit, &observation.raw).await,
            Err(Error::InputConflict)
        );
        assert_eq!(
            super::recovery::resume(&mut app, saved.review_id)
                .await
                .unwrap(),
            AntiBloatAttemptState::InvalidResponse
        );
        assert!(
            app.prepare_send(saved.review_id)
                .await
                .unwrap()
                .permit
                .is_none()
        );
        assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
        assert_eq!(app.store.seals, 0);
        assert_eq!(app.store.raw_response, Some(transport.raw.clone()));
        assert_eq!(app.store.sealed_observation, Some(transport));
        assert_eq!(app.store.consumed, Some(observation));
        assert_eq!(app.store.consumption_count, 1);
        assert_eq!(
            super::recovery::resume(&mut app, saved.review_id)
                .await
                .unwrap(),
            AntiBloatAttemptState::InvalidResponse
        );
        assert_eq!(app.store.consumption_count, 1);
    }
}

#[tokio::test]
async fn incomplete_and_historical_native_seals_skip_usage_and_advice_with_one_unknown_charge() {
    for (native, complete) in [(false, Some(false)), (true, Some(false)), (true, None)] {
        let mut generic = app(true, false);
        let saved = prepare(
            &mut generic,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await;
        generic.store.selected_profile = native.then(|| "native-test".into());
        let mut app = AntiBloatApplication {
            store: generic.store,
            provider: LifecycleProvider {
                native,
                usage_error: None,
                outcome: AntiBloatRankingOutcome::Abstained,
                usages: AtomicUsize::new(0),
                parses: AtomicUsize::new(0),
            },
        };
        let permit = app
            .prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .unwrap();
        let raw = b"true HTTP raw body".to_vec();
        let context = crate::AdvisoryProviderTransportContext {
            send_certainty: tect_domain::AdvisorySendCertainty::Sent,
            outcome: tect_domain::AdvisoryDispatchOutcome::ProviderFailure,
            raw_response_ref: Some(format!("sha256:{:x}", Sha256::digest(&raw))),
            provider_failure_code: Some("response-body-read".into()),
        };
        let transport = AntiBloatProviderObservation {
            raw,
            response_complete: complete,
            original_transport_context: complete.map(|_| context),
            http_status: Some(200),
            input_tokens: Some(2),
            output_tokens: Some(3),
            elapsed_monotonic_ms: Some(7),
        };
        app.seal_response(&permit, &transport).await.unwrap();
        let (observed, invalid) =
            observe_sealed_usage(&mut app.store, &app.provider, &permit, &transport)
                .await
                .unwrap();
        assert!(invalid);
        assert_eq!(observed.input_tokens, None);
        assert_eq!(observed.output_tokens, None);
        assert_eq!(observed.response_complete, complete);
        assert_eq!(
            observed.original_transport_context,
            transport.original_transport_context
        );
        assert_eq!(
            super::recovery::resume(&mut app, saved.review_id)
                .await
                .unwrap(),
            AntiBloatAttemptState::InvalidResponse
        );
        assert_eq!(
            super::recovery::resume(&mut app, saved.review_id)
                .await
                .unwrap(),
            AntiBloatAttemptState::InvalidResponse
        );
        assert_eq!(app.provider.usages.load(Ordering::SeqCst), 0);
        assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
        assert_eq!(app.store.consumption_count, 1);
        assert_eq!(app.store.consumed, Some(observed));
        assert_eq!(app.store.sealed_observation, Some(transport));
    }
}

#[tokio::test]
async fn complete_http500_consumes_known_usage_before_rejecting_advice() {
    let mut generic = app(true, false);
    let saved = prepare(
        &mut generic,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let mut app = AntiBloatApplication {
        store: generic.store,
        provider: LifecycleProvider {
            native: false,
            usage_error: None,
            outcome: AntiBloatRankingOutcome::Abstained,
            usages: AtomicUsize::new(0),
            parses: AtomicUsize::new(0),
        },
    };
    let permit = app
        .prepare_send(saved.review_id)
        .await
        .unwrap()
        .permit
        .unwrap();
    let transport = AntiBloatProviderObservation {
        raw: b"true HTTP raw body".to_vec(),
        response_complete: Some(true),
        original_transport_context: None,
        http_status: Some(500),
        input_tokens: None,
        output_tokens: None,
        elapsed_monotonic_ms: Some(7),
    };
    app.seal_response(&permit, &transport).await.unwrap();
    assert_eq!(
        super::recovery::resume(&mut app, saved.review_id)
            .await
            .unwrap(),
        AntiBloatAttemptState::InvalidResponse
    );
    assert_eq!(app.store.consumed.as_ref().unwrap().input_tokens, Some(2));
    assert_eq!(app.store.consumed.as_ref().unwrap().output_tokens, Some(3));
    assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
}
