use super::*;

#[tokio::test]
async fn no_call_states_are_durable_and_never_send() {
    for (extra, mode, preference, expected) in [
        (
            true,
            WorkspaceAdvisoryMode::Disabled,
            AdvisoryRequestPreference::UseWorkspace,
            AntiBloatNoCall::Disabled,
        ),
        (
            true,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::Skip,
            AntiBloatNoCall::Skipped,
        ),
        (
            false,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
            AntiBloatNoCall::NoEligibleFindings,
        ),
    ] {
        let mut app = app(extra, false);
        let saved = prepare(&mut app, mode, preference).await;
        assert_eq!(saved.state, AntiBloatAttemptState::NoCall(expected));
        let attempt = app.prepare_send(saved.review_id).await.unwrap();
        assert_eq!(attempt.state, saved.state);
        assert!(attempt.permit.is_none());
        assert_eq!(app.store.sends, 0);
        assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
    }
}

#[tokio::test]
async fn missing_trusted_policy_denies_before_one_use_send_fence() {
    let mut app = app(true, false);
    let saved = prepare(
        &mut app,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    app.store.policy = None;
    assert_eq!(
        app.prepare_send(saved.review_id).await,
        Err(Error::BudgetPolicyInvalid)
    );
    assert_eq!(app.store.sends, 0);
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        app.store.saved.unwrap().state,
        AntiBloatAttemptState::Prepared
    );
}

#[tokio::test]
async fn missing_or_overrun_usage_suppresses_ranked_advice_after_raw_seal() {
    for (input_tokens, output_tokens, elapsed, expected) in [
        (None, Some(1), Some(1), true),
        (Some(101), Some(1), Some(1), true),
        (Some(1), Some(1), Some(1001), true),
        (Some(100), Some(100), Some(1000), false),
    ] {
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
        let mut observation = rank_after_committed_fence(async { Ok(()) }, &app.provider, &permit)
            .await
            .unwrap()
            .unwrap();
        observation.input_tokens = input_tokens;
        observation.output_tokens = output_tokens;
        observation.elapsed_monotonic_ms = elapsed;
        app.seal_response(&permit, &observation.raw).await.unwrap();
        assert_eq!(
            app.store
                .consume_budget(&permit, &observation)
                .await
                .unwrap(),
            expected
        );
        assert_eq!(
            app.store
                .consume_budget(&permit, &observation)
                .await
                .unwrap(),
            expected
        );
        if expected {
            app.store.mark_send_unknown(permit.review_id).await.unwrap();
            assert_eq!(
                app.store.saved.as_ref().unwrap().state,
                AntiBloatAttemptState::SendUnknown
            );
        } else {
            assert!(matches!(
                app.finalize_response(&permit, &observation.raw)
                    .await
                    .unwrap(),
                AntiBloatAttemptState::Ranked(_)
            ));
        }
    }
}

#[tokio::test]
async fn one_use_rank_and_provider_invention_is_denied() {
    let mut app = app(true, false);
    let saved = prepare(
        &mut app,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let attempt = app.prepare_send(saved.review_id).await.unwrap();
    assert_eq!(attempt.state, AntiBloatAttemptState::Sending);
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 0);
    assert!(app.store.raw_response.is_none());
    let permit = attempt.permit.unwrap();
    let raw = rank_after_committed_fence(async { Ok(()) }, &app.provider, &permit)
        .await
        .unwrap()
        .unwrap();
    app.seal_response(&permit, &raw.raw).await.unwrap();
    assert!(!app.store.consume_budget(&permit, &raw).await.unwrap());
    assert_eq!(app.store.seals, 0);
    assert_eq!(
        app.finalize_response(&permit, &raw.raw).await.unwrap(),
        app.store.saved.as_ref().unwrap().state
    );
    assert!(
        app.prepare_send(saved.review_id)
            .await
            .unwrap()
            .permit
            .is_none()
    );
    assert_eq!(app.store.sends, 1);
    assert_eq!(app.provider.calls.load(Ordering::SeqCst), 1);
    assert_eq!(app.store.applies, 0);
    let prepared = app.store.prepared.as_ref().unwrap();
    assert_eq!(
        prepared.sha256,
        format!("{:x}", sha2::Sha256::digest(&prepared.bytes))
    );
    let request: serde_json::Value = serde_json::from_slice(&prepared.bytes).unwrap();
    assert_eq!(
        request["review"],
        serde_json::to_value(&saved.review).unwrap()
    );
    let raw = app.store.raw_response.as_ref().unwrap();
    assert_eq!(
        app.store.response_sha256.as_ref().unwrap(),
        &format!("{:x}", sha2::Sha256::digest(raw))
    );
    let mut invented = self::app(true, true);
    let saved = prepare(
        &mut invented,
        WorkspaceAdvisoryMode::Optional,
        AdvisoryRequestPreference::UseWorkspace,
    )
    .await;
    let permit = invented
        .prepare_send(saved.review_id)
        .await
        .unwrap()
        .permit
        .unwrap();
    let raw = rank_after_committed_fence(async { Ok(()) }, &invented.provider, &permit)
        .await
        .unwrap()
        .unwrap();
    invented.seal_response(&permit, &raw.raw).await.unwrap();
    assert!(!invented.store.consume_budget(&permit, &raw).await.unwrap());
    assert!(matches!(
        invented.finalize_response(&permit, &raw.raw).await,
        Err(Error::InputConflict)
    ));
    assert_eq!(invented.store.seals, 0);
    assert_eq!(
        invented.store.raw_response.as_deref(),
        Some(br#"["invented"]"#.as_slice())
    );
    assert_eq!(
        invented.store.saved.as_ref().unwrap().state,
        AntiBloatAttemptState::SendUnknown
    );
    assert_eq!(
        invented.prepare_send(saved.review_id).await.unwrap().state,
        AntiBloatAttemptState::SendUnknown
    );
    assert_eq!(invented.provider.calls.load(Ordering::SeqCst), 1);
}

#[test]
fn independent_verifier_rederives_full_graph_not_receipt_claim() {
    let input = input(true);
    let review = review_anti_bloat(&Sha256ScopeDigest, &input).unwrap();
    let finding = review
        .findings
        .iter()
        .find(|item| item.candidate_id == Uuid::from_u128(70))
        .unwrap();
    let finding_id = finding.id.clone();
    let delta = CandidateDeltaBatch {
        candidate_set_id: review.candidate_set_id,
        expected_revision: review.plan_revision,
        idempotency_key: "verifier-fixture".into(),
        operations: vec![CandidateDeltaOperation::CandidateRemove {
            candidate_id: finding.candidate_id,
            expected_revision: 1,
        }],
    };
    let (preservation, after) = derive_anti_bloat_delta(
        &Sha256ScopeDigest,
        &input,
        &review,
        &finding_id,
        AntiBloatDisposition::Narrow,
        &delta,
    )
    .unwrap();
    let before = input
        .manifest
        .eligible(&input.selected_id)
        .unwrap()
        .material
        .clone();
    let mut material = crate::AntiBloatVerificationMaterial {
        workspace_id: Uuid::from_u128(10),
        review_id: Uuid::from_u128(12),
        review_actor_id: Uuid::from_u128(11),
        selected_disposition_actor_id: Uuid::from_u128(11),
        selected_caller_actor_id: Uuid::from_u128(11),
        selected_caller_session_id: Uuid::from_u128(13),
        input,
        review,
        finding_id,
        disposition: AntiBloatDisposition::Narrow,
        preservation: preservation.clone(),
        delta: delta.clone(),
        claimed_after: after.clone(),
        receipt: AntiBloatApplyReceipt {
            review_id: Uuid::from_u128(12),
            candidate_set_id: delta.candidate_set_id,
            idempotency_key: delta.idempotency_key.clone(),
            caller_request_id: Uuid::from_u128(14),
            from_revision: delta.expected_revision,
            to_revision: delta.expected_revision + 1,
            source_digest: preservation.source_digest.clone(),
            before_material_digest: preservation.before_material_digest.clone(),
            after_material_digest: preservation.after_material_digest.clone(),
        },
        before_saved: before,
        after_saved: after,
        current_revision: delta.expected_revision + 1,
        source_fragments_match: true,
    };
    let golden = include_str!("../verification_material_golden.json").trim_end();
    assert_eq!(serde_json::to_string(&material).unwrap(), golden);
    assert_eq!(
        material.digest().unwrap(),
        "d554eeed3caffa9ea36a4271ba0b8b28b14e02a19f937a7d3edee6616f42c92f"
    );
    let round_trip: crate::AntiBloatVerificationMaterial = serde_json::from_str(golden).unwrap();
    assert_eq!(round_trip, material);
    assert_eq!(material.verdict().0, AntiBloatVerificationVerdict::Pass);
    material.after_saved.candidates[0].title.push_str(" forged");
    assert_eq!(material.verdict().0, AntiBloatVerificationVerdict::Fail);
    material.source_fragments_match = false;
    assert_eq!(material.verdict().0, AntiBloatVerificationVerdict::Unknown);
}

#[test]
fn verification_request_json_shape_remains_stable() {
    let request = crate::VerifyAntiBloatApply {
        request_id: Uuid::from_u128(1),
        review_id: Uuid::from_u128(2),
        expected_evidence_digest: D.into(),
    };
    let golden = format!(
        "{{\"request_id\":\"00000000-0000-0000-0000-000000000001\",\"review_id\":\"00000000-0000-0000-0000-000000000002\",\"expected_evidence_digest\":\"{D}\"}}"
    );
    assert_eq!(serde_json::to_string(&request).unwrap(), golden);
    assert_eq!(
        serde_json::from_str::<crate::VerifyAntiBloatApply>(&golden).unwrap(),
        request
    );
    assert!(
        serde_json::from_str::<crate::VerifyAntiBloatApply>(
            &golden.replace("}", ",\"extra\":true}")
        )
        .is_err()
    );
}
