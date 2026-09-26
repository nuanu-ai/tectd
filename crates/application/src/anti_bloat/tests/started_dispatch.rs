use super::*;
use crate::AntiBloatStartedDispatchPermit;

#[tokio::test]
async fn started_capability_binds_frozen_metadata_and_claims_once_even_after_provider_error() {
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
    let started = AntiBloatStartedDispatchPermit::after_committed_fence(&permit).unwrap();
    assert_eq!(started.reservation(), &permit);
    let mut copied_metadata = permit.clone();
    copied_metadata.request.bytes.push(b' ');
    assert_ne!(started.reservation(), &copied_metadata);
    assert!(AntiBloatStartedDispatchPermit::after_committed_fence(&copied_metadata).is_err());
    assert!(app.provider.rank(&started).await.is_ok());
    assert_eq!(app.provider.rank(&started).await, Err(Error::InputConflict));
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 1);
    assert!(
        app.prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .is_none()
    );
    assert_eq!(app.store.sends, 1);
    let failed = AntiBloatStartedDispatchPermit::after_committed_fence(&permit).unwrap();
    let provider = CommitObservingProvider {
        committed: Arc::new(AtomicBool::new(true)),
        calls: AtomicUsize::new(0),
        fail: true,
    };
    assert_eq!(
        provider.rank(&failed).await,
        Err(Error::TransportUnavailable)
    );
    assert_eq!(provider.rank(&failed).await, Err(Error::InputConflict));
    assert_eq!(provider.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn concurrent_claims_have_one_winner() {
    let permit = AntiBloatSendPermit {
        review_id: Uuid::new_v4(),
        request: AntiBloatPreparedRequest {
            bytes: b"{}".to_vec(),
            sha256: format!("{:x}", Sha256::digest(b"{}")),
            material_sha256: "a".repeat(64),
            adapter_identity: "generic-json-v1".into(),
        },
    };
    let started = Arc::new(AntiBloatStartedDispatchPermit::after_committed_fence(&permit).unwrap());
    let barrier = Arc::new(std::sync::Barrier::new(8));
    let handles = (0..8)
        .map(|_| {
            let started = started.clone();
            let barrier = barrier.clone();
            std::thread::spawn(move || {
                barrier.wait();
                started.claim().is_ok()
            })
        })
        .collect::<Vec<_>>();
    assert_eq!(
        handles
            .into_iter()
            .map(|handle| usize::from(handle.join().unwrap()))
            .sum::<usize>(),
        1
    );
}
