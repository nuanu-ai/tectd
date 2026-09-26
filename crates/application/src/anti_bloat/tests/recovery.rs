use super::*;

// FakeStore operations represent committed stages; faults occur before writes.
async fn resume<S: AntiBloatRankingProvider>(
    app: &mut AntiBloatApplication<FakeStore, S>,
    id: Uuid,
) -> Result<AntiBloatAttemptState> {
    let state = app.store.saved.as_ref().unwrap().state.clone();
    if state != AntiBloatAttemptState::Sending {
        return Ok(state);
    }
    let Some(saved) =
        crate::anti_bloat::recovery::load_saved_response(&mut app.store, &app.provider, id).await?
    else {
        return Ok(AntiBloatAttemptState::Sending);
    };
    let (observation, invalid) = observe_sealed_usage(
        &mut app.store,
        &app.provider,
        &saved.permit,
        &saved.observation,
    )
    .await?;
    let exhausted = app
        .store
        .consume_budget(&saved.permit, &observation)
        .await?;
    if invalid
        || observation
            .http_status
            .is_some_and(|n| !(200..=299).contains(&n))
    {
        app.store
            .seal_terminal(&saved.permit, AntiBloatAttemptState::InvalidResponse)
            .await?;
        return Ok(AntiBloatAttemptState::InvalidResponse);
    }
    if exhausted {
        app.store.mark_send_unknown(id).await?;
        return Ok(AntiBloatAttemptState::SendUnknown);
    }
    app.finalize_response(&saved.permit, &observation.raw).await
}

async fn started() -> (
    AntiBloatApplication<FakeStore, FakeProvider>,
    Uuid,
    AntiBloatSendPermit,
    AntiBloatProviderObservation,
) {
    let mut app = app(true, false);
    let saved = prepare(
        &mut app,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let permit = app
        .prepare_send(saved.review_id)
        .await
        .unwrap()
        .permit
        .unwrap();
    let response = rank_after_committed_fence(async { Ok(()) }, &app.provider, &permit)
        .await
        .unwrap()
        .unwrap();
    (app, saved.review_id, permit, response)
}

#[tokio::test]
async fn failures_after_seal_resume_identical_observation_without_second_dispatch_or_charge() {
    for stage in 0..3 {
        let (mut app, id, permit, response) = started().await;
        app.seal_response(&permit, &response).await.unwrap();
        match stage {
            0 => app.store.fail_recovery_read = true,
            1 => app.store.fail_consume = true,
            _ => app.store.fail_finish = true,
        }
        assert_eq!(resume(&mut app, id).await, Err(Error::StorageUnavailable));
        assert!(matches!(
            resume(&mut app, id).await.unwrap(),
            AntiBloatAttemptState::Ranked(_)
        ));
        let charged = app.store.consumed.clone();
        assert!(matches!(
            resume(&mut app, id).await.unwrap(),
            AntiBloatAttemptState::Ranked(_)
        ));
        assert_eq!(app.store.consumed, charged);
        assert_eq!(app.store.sealed_observation, Some(response.clone()));
        assert_eq!(
            app.store.response_sha256,
            Some(format!("{:x}", Sha256::digest(&response.raw)))
        );
        assert_eq!(app.store.sends, 1);
        assert_eq!(app.provider.calls.load(Ordering::SeqCst), 1);
        assert_eq!(app.store.consumption_count, 1);
    }
}

#[tokio::test]
async fn unsealed_send_is_unresolved_and_material_or_identity_changes_cannot_resume() {
    let (mut app, id, permit, response) = started().await;
    assert_eq!(
        resume(&mut app, id).await.unwrap(),
        AntiBloatAttemptState::Sending
    );
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 1);
    assert!(app.store.consumed.is_none());
    app.seal_response(&permit, &response).await.unwrap();
    app.store.prepared.as_mut().unwrap().adapter_identity = "different/1".into();
    assert_eq!(resume(&mut app, id).await, Err(Error::InputConflict));
    app.store.prepared = Some(permit.request.clone());
    app.store.saved.as_mut().unwrap().workspace_id = Uuid::new_v4();
    assert_eq!(resume(&mut app, id).await, Err(Error::InputConflict));
    assert_eq!(app.store.sends, 1);
    assert!(app.store.consumed.is_none());
}

struct ParseObservingProvider {
    parses: AtomicUsize,
    different_config: bool,
}
#[async_trait]
impl AntiBloatRankingProvider for ParseObservingProvider {
    fn prepare(&self, material: &crate::AntiBloatRankingMaterial<'_>) -> Result<Vec<u8>> {
        let mut bytes=serde_json::to_vec(&serde_json::json!({"review":&material.saved.review,"eligible_ids":material.eligible_ids})).unwrap();
        if self.different_config {
            bytes.push(b' ');
        }
        Ok(bytes)
    }
    fn parse_sealed(
        &self,
        _: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<crate::AntiBloatRankingOutcome> {
        self.parses.fetch_add(1, Ordering::SeqCst);
        Ok(crate::AntiBloatRankingOutcome::Ranked(
            serde_json::from_slice(&observation.raw).unwrap(),
        ))
    }
    async fn rank(
        &self,
        _: &crate::AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        panic!("recovery cannot send")
    }
}

#[tokio::test]
async fn http_error_body_accounts_usage_but_cannot_parse_choice_and_changed_config_fails_closed() {
    let (mut generic, id, permit, mut response) = started().await;
    response.http_status = Some(500); // The body is still a complete valid ranking.
    generic.seal_response(&permit, &response).await.unwrap();
    let mut app = AntiBloatApplication {
        store: generic.store,
        provider: ParseObservingProvider {
            parses: AtomicUsize::new(0),
            different_config: true,
        },
    };
    assert_eq!(resume(&mut app, id).await, Err(Error::InputConflict));
    assert!(app.store.consumed.is_none());
    app.provider.different_config = false;
    assert_eq!(
        resume(&mut app, id).await.unwrap(),
        AntiBloatAttemptState::InvalidResponse
    );
    assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
    assert_eq!(app.store.seals, 0);
    assert_eq!(app.store.consumption_count, 1);
    assert_eq!(app.store.sealed_observation, Some(response));
}

#[tokio::test]
async fn old_seal_without_original_elapsed_cannot_invent_usage_or_advice() {
    let (mut app, id, permit, mut response) = started().await;
    response.elapsed_monotonic_ms = None;
    app.seal_response(&permit, &response).await.unwrap();
    assert_eq!(
        resume(&mut app, id).await.unwrap(),
        AntiBloatAttemptState::SendUnknown
    );
    assert_eq!(app.store.seals, 0);
    assert_eq!(
        app.store.consumed.as_ref().unwrap().elapsed_monotonic_ms,
        None
    );
}
