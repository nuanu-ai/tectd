use super::*;

#[tokio::test]
async fn exact_agent_delta_preserves_plan_before_caller() {
    let mut app = app(true, false);
    let saved = prepare(
        &mut app,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let extra = Uuid::from_u128(70);
    let finding = saved
        .review
        .findings
        .iter()
        .find(|item| item.candidate_id == extra)
        .unwrap();
    let before = &saved
        .input
        .manifest
        .eligible(&saved.input.selected_id)
        .unwrap()
        .material;
    let authored = AntiBloatAuthoredDelta {
        review_id: saved.review_id,
        finding_id: finding.id.clone(),
        disposition: AntiBloatDisposition::Narrow,
        delta: CandidateDeltaBatch {
            candidate_set_id: Uuid::from_u128(1),
            expected_revision: 3,
            idempotency_key: "exact-removal".into(),
            operations: vec![CandidateDeltaOperation::CandidateRemove {
                candidate_id: extra,
                expected_revision: 1,
            }],
        },
    };
    let mut keep = authored.clone();
    keep.disposition = AntiBloatDisposition::Keep;
    assert!(matches!(
        app.disposition_and_apply(&keep).await,
        Err(Error::InputConflict)
    ));
    let mut mismatch = authored.clone();
    mismatch
        .delta
        .operations
        .push(CandidateDeltaOperation::CandidateRemove {
            candidate_id: Uuid::from_u128(52),
            expected_revision: 1,
        });
    assert!(matches!(
        app.disposition_and_apply(&mismatch).await,
        Err(Error::InputConflict)
    ));
    assert_eq!(app.store.applies, 0);
    let receipt = app.disposition_and_apply(&authored).await.unwrap();
    assert_eq!(receipt.to_revision, 4);
    assert_eq!(app.store.applies, 1);
    let after = app.store.after.as_ref().unwrap();
    assert_eq!(after.candidates.len(), before.candidates.len() - 1);
    assert_eq!(after.candidates[0], before.candidates[0]);
    app.store.input.as_mut().unwrap().dependency_digest =
        "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb".into();
    assert!(matches!(
        app.disposition_and_apply(&authored).await,
        Err(Error::InputConflict)
    ));
    assert_eq!(app.store.applies, 1);
}
