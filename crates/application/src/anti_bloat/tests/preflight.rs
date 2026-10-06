use super::*;

#[tokio::test]
async fn configured_profile_must_match_before_budget_and_recovery() {
    for selected in [None, Some("different"), Some("native")] {
        let mut app = app(true, false);
        let saved = prepare(
            &mut app,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await;
        app.provider.required_profile = Some("native");
        app.store.selected_profile = selected.map(str::to_owned);
        if selected != Some("native") {
            app.store.policy = None;
        }
        let prepared = app.prepare_send(saved.review_id).await.unwrap();
        if selected == Some("native") {
            let permit = prepared.permit.unwrap();
            let observation = rank_after_committed_fence(async { Ok(()) }, &app.provider, &permit)
                .await
                .unwrap()
                .unwrap();
            seal_anti_bloat_response(&mut app.store, &permit, &observation)
                .await
                .unwrap();
            app.store.selected_profile = Some("changed".into());
            assert_eq!(
                super::super::recovery::load_saved_response(
                    &mut app.store,
                    &app.provider,
                    saved.review_id
                )
                .await,
                Err(Error::InputConflict)
            );
            assert_eq!(app.provider.calls.load(Ordering::SeqCst), 1);
        } else {
            assert_eq!(
                prepared.state,
                AntiBloatAttemptState::NoCall(AntiBloatNoCall::PreflightInvalidConfiguration)
            );
            assert!(prepared.permit.is_none());
            assert_eq!(app.store.sends, 0);
            assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
            assert_eq!(app.store.consumption_count, 0);
        }
    }
}

struct FailedPreparation {
    error: Error,
    calls: AtomicUsize,
}
#[async_trait]
impl AntiBloatRankingProvider for FailedPreparation {
    fn prepare(&self, _: &crate::AntiBloatRankingMaterial<'_>) -> Result<Vec<u8>> {
        Err(self.error.clone())
    }
    async fn rank(
        &self,
        _: &crate::AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        panic!("preflight must never dispatch")
    }
}

#[tokio::test]
async fn provider_unconfigured_is_durable_no_call_despite_positive_signed_budget() {
    let mut generic = app(true, false);
    let saved = prepare(
        &mut generic,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    assert!(generic.store.policy.is_some());
    let mut app = AntiBloatApplication {
        store: generic.store,
        provider: crate::DisabledAntiBloatRankingProvider,
    };
    let first = app.prepare_send(saved.review_id).await.unwrap();
    assert_eq!(
        first.state,
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::ProviderUnconfigured)
    );
    assert!(first.permit.is_none());
    assert_eq!(app.store.saved.as_ref().unwrap().state, first.state);
    assert_eq!(app.prepare_send(saved.review_id).await.unwrap(), first);
    assert_eq!(app.store.sends, 0);
    assert!(app.store.prepared.is_none());
    assert!(app.store.consumed.is_none());
}

#[tokio::test]
async fn pure_preflight_failures_are_durable_but_authority_storage_errors_propagate() {
    for (error, expected) in [
        (
            Error::InvalidConfiguration,
            Some(AntiBloatNoCall::PreflightInvalidConfiguration),
        ),
        (
            Error::InvalidArguments,
            Some(AntiBloatNoCall::PreflightInvalidArguments),
        ),
        (
            Error::InputConflict,
            Some(AntiBloatNoCall::PreflightInputConflict),
        ),
        (
            Error::RequestTooLarge,
            Some(AntiBloatNoCall::PreflightRequestTooLarge),
        ),
        (Error::Forbidden, None),
        (Error::StorageUnavailable, None),
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
            provider: FailedPreparation {
                error: error.clone(),
                calls: AtomicUsize::new(0),
            },
        };
        let outcome = app.prepare_send(saved.review_id).await;
        if let Some(reason) = expected {
            let first = outcome.unwrap();
            assert_eq!(first.state, AntiBloatAttemptState::NoCall(reason));
            assert!(first.permit.is_none());
            assert_eq!(app.store.saved.as_ref().unwrap().state, first.state);
            assert_eq!(app.prepare_send(saved.review_id).await.unwrap(), first);
        } else {
            assert_eq!(outcome, Err(error));
            assert_eq!(
                app.store.saved.as_ref().unwrap().state,
                AntiBloatAttemptState::Prepared
            );
        }
        assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
        assert_eq!(app.store.sends, 0);
        assert!(app.store.prepared.is_none());
        assert!(app.store.consumed.is_none());
    }
}
