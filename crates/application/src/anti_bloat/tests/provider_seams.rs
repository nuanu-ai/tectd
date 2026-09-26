use super::*;

struct WireProvider {
    fail_prepare: bool,
    parses: AtomicUsize,
}

#[async_trait]
impl AntiBloatRankingProvider for WireProvider {
    fn adapter_identity(&self) -> &'static str {
        "test-wire-v1"
    }
    fn prepare(&self, material: &crate::AntiBloatRankingMaterial<'_>) -> Result<Vec<u8>> {
        if self.fail_prepare {
            return Err(Error::TransportUnavailable);
        }
        assert!(!material.eligible_ids.is_empty());
        Ok(b"exact provider wire".to_vec())
    }
    fn parse_sealed(
        &self,
        _: &AntiBloatSendPermit,
        observation: &AntiBloatProviderObservation,
    ) -> Result<crate::AntiBloatRankingOutcome> {
        self.parses.fetch_add(1, Ordering::SeqCst);
        let payload = observation
            .raw
            .strip_prefix(b"untouched transport:")
            .ok_or(Error::InputConflict)?;
        serde_json::from_slice(payload)
            .map(crate::AntiBloatRankingOutcome::Ranked)
            .map_err(|_| Error::InputConflict)
    }
    async fn rank(
        &self,
        started: &crate::AntiBloatStartedDispatchPermit,
    ) -> Result<AntiBloatProviderObservation> {
        let permit = started.claim()?;
        assert_eq!(permit.request.bytes, b"exact provider wire");
        Err(Error::Forbidden) // This test never performs transport.
    }
}

#[tokio::test]
async fn provider_preparation_and_parse_are_fenced_by_durable_authorization() {
    let mut generic = app(true, false);
    let saved = prepare(
        &mut generic,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let mut app = AntiBloatApplication {
        store: generic.store,
        provider: WireProvider {
            fail_prepare: true,
            parses: AtomicUsize::new(0),
        },
    };
    assert_eq!(
        app.prepare_send(saved.review_id).await,
        Err(Error::TransportUnavailable)
    );
    assert_eq!(app.store.sends, 0);
    app.provider.fail_prepare = false;
    let permit = app
        .prepare_send(saved.review_id)
        .await
        .unwrap()
        .permit
        .unwrap();
    assert_eq!(app.store.prepared.as_ref(), Some(&permit.request));
    assert_eq!(permit.request.bytes, b"exact provider wire");
    assert_eq!(
        rank_after_committed_fence(async { Ok(()) }, &app.provider, &permit)
            .await
            .unwrap(),
        Err(Error::Forbidden)
    );
    assert_eq!(
        permit.request.sha256,
        format!("{:x}", Sha256::digest(b"exact provider wire"))
    );
    assert_eq!(
        permit.request.material_sha256,
        crate::anti_bloat_material_sha256(&saved).unwrap()
    );
    let ids = saved
        .review
        .findings
        .iter()
        .filter(|f| f.rankable)
        .map(|f| f.id.clone())
        .collect::<Vec<_>>();
    let mut raw = b"untouched transport:".to_vec();
    raw.extend(serde_json::to_vec(&ids).unwrap());
    assert_eq!(
        app.finalize_response(&permit, &raw).await,
        Err(Error::InputConflict)
    );
    let mut observation = AntiBloatProviderObservation {
        response_complete: None,
        original_transport_context: None,
        http_status: None,
        raw: raw.clone(),
        input_tokens: Some(101),
        output_tokens: Some(1),
        elapsed_monotonic_ms: Some(1),
    };
    app.seal_response(&permit, &observation).await.unwrap();
    assert_eq!(
        app.finalize_response(&permit, &raw).await,
        Err(Error::InputConflict)
    );
    assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
    assert!(
        app.store
            .consume_budget(&permit, &observation)
            .await
            .unwrap()
    );
    assert_eq!(
        app.finalize_response(&permit, &raw).await,
        Err(Error::InputConflict)
    );
    assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
    // Independent successful budget decision exercises parsing after authorization.
    app.store.consumed = None;
    observation.input_tokens = Some(1);
    assert!(
        !app.store
            .consume_budget(&permit, &observation)
            .await
            .unwrap()
    );
    let mut forged = permit.clone();
    forged.request.adapter_identity = "different-v1".into();
    assert_eq!(
        app.finalize_response(&forged, &raw).await,
        Err(Error::InputConflict)
    );
    assert_eq!(app.provider.parses.load(Ordering::SeqCst), 0);
    assert_eq!(
        app.finalize_response(&permit, &raw).await.unwrap(),
        AntiBloatAttemptState::Ranked(ids)
    );
    assert_eq!(app.store.raw_response, Some(raw.clone()));
    assert_eq!(
        app.finalize_response(&permit, &raw).await,
        Err(Error::InputConflict)
    );
    assert!(
        app.prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .is_none()
    );
    assert_eq!(app.provider.parses.load(Ordering::SeqCst), 1);
}
