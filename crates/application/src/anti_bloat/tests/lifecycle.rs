use super::*;
use crate::{AntiBloatRankingOutcome, AntiBloatUsage};

struct LifecycleProvider {
    usage_error: Option<Error>,
    outcome: AntiBloatRankingOutcome,
    usages: AtomicUsize,
    parses: AtomicUsize,
}
#[async_trait]
impl AntiBloatRankingProvider for LifecycleProvider {
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
