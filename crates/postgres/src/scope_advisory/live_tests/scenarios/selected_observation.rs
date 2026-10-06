mod qualified;
pub(in super::super) use qualified::verify as verify_independent;

macro_rules! verify {
    ($actor:ident, $after:ident, $audit_query:ident, $audit_unit:ident, $auth:ident, $candidate:ident, $context:ident, $cursor:ident, $enrollment:ident, $failed:ident, $foreign:ident, $foreign_unit:ident, $missing:ident, $observation:ident, $observed:ident, $opportunity:ident, $page:ident, $pool:ident, $save:ident, $second_auth:ident, $second_store:ident, $selected_audit:ident, $selected_opportunity:ident, $service:ident, $session:ident, $store:ident, $stored:ident, $tenant:ident, $unit:ident, $verifier_session:ident, $workspace:ident $(,)?) => {
    let observe_request = SelectedSaveObservationRequest {
        request_id: Uuid::new_v4(),
        opportunity_id: $selected_opportunity,
        candidate_set_id: $candidate,
        caller_link_id: $selected_audit.caller_link_id.unwrap(),
        caller_receipt_request_id: $save.request_id,
        target_revision: 6,
        session_id: $session,
    };
    let mut observation_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let $observed = observation_unit
        .observe_selected_scope_save($workspace, &observe_request)
        .await
        .unwrap();
    assert_eq!($observed.status, SelectedSaveObservationStatus::Passed);
    assert!($observed.reason_codes.is_empty());
    assert_eq!($observed.qualification, "unresolved");
    observation_unit.commit().await.unwrap();
    let mut $audit_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let passed_audit = $audit_unit
        .candidate_advisory_opportunity_detail($workspace, $candidate, $selected_opportunity)
        .await
        .unwrap()
        .$opportunity;
    let public_pass = passed_audit.selected_save_observation.unwrap();
    assert_eq!(public_pass.id, $observed.id);
    assert_eq!(public_pass.status, SelectedSaveObservationStatus::Passed);
    assert_eq!(public_pass.target_revision, 6);
    assert!(public_pass.reason_codes.is_empty());
    assert_eq!(public_pass.evidence_digest, $observed.evidence_digest);
    assert_eq!(public_pass.qualification, "unresolved");
    assert!(!public_pass.establishes_independent_approval);
    assert!(!public_pass.establishes_current_acceptance);
    assert_eq!(passed_audit.verifier_receipt_id, None);
    drop($audit_unit);
    let $foreign = admin::enroll_host(&$pool, None, vec![]).await.unwrap();
    let mut $foreign_unit = rw(&$store, &$foreign.$auth, $foreign.tenant_id).await;
    assert!(
        $foreign_unit
            .candidate_advisory_audit($workspace, $candidate, &$audit_query)
            .await
            .unwrap()
            .opportunities
            .is_empty()
    );
    assert!(matches!(
        $foreign_unit
            .candidate_advisory_opportunity_detail($workspace, $candidate, $selected_opportunity)
            .await,
        Err(Error::NotFound)
    ));
    drop($foreign_unit);
    let mut replay_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        replay_unit
            .observe_selected_scope_save($workspace, &observe_request)
            .await
            .unwrap(),
        $observed
    );
    let mut changed_target = observe_request.clone();
    changed_target.caller_link_id = Uuid::new_v4();
    assert_eq!(
        replay_unit
            .observe_selected_scope_save($workspace, &changed_target)
            .await,
        Err(Error::InputConflict)
    );
    let mut changed_identity = observe_request.clone();
    changed_identity.session_id = $verifier_session;
    assert_eq!(
        replay_unit
            .observe_selected_scope_save($workspace, &changed_identity)
            .await,
        Err(Error::InputConflict)
    );
    drop(replay_unit);
    let mut concurrent_request = observe_request.clone();
    concurrent_request.request_id = Uuid::new_v4();
    let mut first_observer = rw(&$store, &$enrollment.$auth, $tenant).await;
    let first_observation = first_observer
        .observe_selected_scope_save($workspace, &concurrent_request)
        .await
        .unwrap();
    let $second_store = $store.clone();
    let $second_auth = $enrollment.$auth.clone();
    let second_request = concurrent_request.clone();
    let mut second_observer = tokio::spawn(async move {
        let mut $unit = rw(&$second_store, &$second_auth, $tenant).await;
        let $observation = $unit
            .observe_selected_scope_save($workspace, &second_request)
            .await?;
        $unit.commit().await?;
        Ok::<_, Error>($observation)
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut second_observer)
            .await
            .is_err()
    );
    first_observer.commit().await.unwrap();
    let second_observation = second_observer.await.unwrap().unwrap();
    assert_eq!(second_observation, first_observation);
    let effects_before: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1)",
    )
    .bind($candidate)
    .fetch_one(&$pool)
    .await
    .unwrap();
    let original_draft: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=6",
    ).bind($tenant).bind($workspace).bind($candidate).fetch_one(&$pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_drafts SET payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=6")
        .bind($tenant).bind($workspace).bind($candidate).execute(&$pool).await.unwrap();
    let mut tampered = observe_request.clone();
    tampered.request_id = Uuid::new_v4();
    let mut observation_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let tamper_result = observation_unit
        .observe_selected_scope_save($workspace, &tampered)
        .await
        .unwrap();
    assert_eq!(tamper_result.status, SelectedSaveObservationStatus::Failed);
    assert!(
        tamper_result
            .reason_codes
            .contains(&"saved_material_missing_or_mismatched".into())
    );
    observation_unit.commit().await.unwrap();
    let mut $audit_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let latest = $audit_unit
        .candidate_advisory_opportunity_detail($workspace, $candidate, $selected_opportunity)
        .await
        .unwrap()
        .$opportunity
        .selected_save_observation
        .unwrap();
    assert_eq!(latest.id, tamper_result.id);
    assert_eq!(latest.status, SelectedSaveObservationStatus::Failed);
    assert_eq!(latest.target_revision, 6);
    assert!(
        latest
            .reason_codes
            .contains(&"saved_material_missing_or_mismatched".into())
    );
    assert_eq!(latest.qualification, "unresolved");
    let mut $cursor = None;
    let mut selected_rows = 0;
    loop {
        let $page = $audit_unit
            .candidate_advisory_audit(
                $workspace,
                $candidate,
                &AdvisoryAuditQuery {
                    $after: $cursor,
                    ..$audit_query.clone()
                },
            )
            .await
            .unwrap();
        assert!($page.opportunities.len() <= 1);
        for $opportunity in $page.opportunities {
            if $opportunity.id == $selected_opportunity {
                selected_rows += 1;
                assert_eq!(
                    $opportunity.selected_save_observation.as_ref(),
                    Some(&latest)
                );
            }
        }
        $cursor = $page.next_after;
        if $cursor.is_none() {
            break;
        }
    }
    assert_eq!(selected_rows, 1);
    drop($audit_unit);
    let mut stale = observe_request.clone();
    stale.request_id = Uuid::new_v4();
    stale.target_revision = 5;
    let mut observation_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let stale_result = observation_unit
        .observe_selected_scope_save($workspace, &stale)
        .await
        .unwrap();
    assert_eq!(stale_result.status, SelectedSaveObservationStatus::Failed);
    assert!(
        stale_result
            .reason_codes
            .contains(&"candidate_revision_stale".into())
    );
    observation_unit.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_drafts SET payload=$1 WHERE tenant_id=$2 AND workspace_id=$3 AND candidate_set_id=$4 AND set_revision=6")
        .bind(original_draft).bind($tenant).bind($workspace).bind($candidate).execute(&$pool).await.unwrap();
    let mut $missing = observe_request.clone();
    $missing.request_id = Uuid::new_v4();
    $missing.caller_link_id = Uuid::new_v4();
    let mut observation_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let missing_result = observation_unit
        .observe_selected_scope_save($workspace, &$missing)
        .await
        .unwrap();
    assert_eq!(missing_result.status, SelectedSaveObservationStatus::Failed);
    assert!(
        missing_result
            .reason_codes
            .contains(&"caller_link_missing_or_mismatched".into())
    );
    observation_unit.commit().await.unwrap();
    let effects_after: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1)",
    )
    .bind($candidate)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(effects_after, effects_before);
    let mut changed_payload = $save.clone();
    changed_payload.draft.candidates[0].title = "Conflicting replay".into();
    let mut conflict = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        conflict
            .save_selected_candidate_draft($workspace, $actor, $session, &changed_payload)
            .await,
        Err(Error::InputConflict),
    );
    drop(conflict);
    let mut replay_unit = rw(&$store, &$enrollment.$auth, $tenant).await;
    let replayed = replay_unit
        .save_selected_candidate_draft($workspace, $actor, $session, &$save)
        .await
        .unwrap();
    assert_eq!(replayed.draft, $stored.draft);
    assert_eq!(
        replayed.$context.candidate_set.revision,
        $stored.$context.candidate_set.revision
    );
    replay_unit.commit().await.unwrap();
    let mut wrong_session = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        wrong_session
            .save_selected_candidate_draft($workspace, $actor, $verifier_session, &$save)
            .await,
        Err(Error::InputConflict),
    );
    drop(wrong_session);
    let same_session_replay = $service
        .save_candidate_draft(
            &$context,
            &$save,
            &FixtureCandidateGuidance,
            &FixtureCandidateOutputGuard,
        )
        .await
        .unwrap();
    assert_eq!(same_session_replay.draft, $stored.draft);
    let other_context = tect_domain::RequestContext {
        native_session_id: $verifier_session.to_string(),
        ..$context.clone()
    };
    assert_eq!(
        $service
            .save_candidate_draft(
                &other_context,
                &$save,
                &FixtureCandidateGuidance,
                &FixtureCandidateOutputGuard,
            )
            .await,
        Err(Error::InputConflict),
    );
    let mut stale_save = $save.clone();
    stale_save.request_id = Uuid::new_v4();
    let mut $failed = rw(&$store, &$enrollment.$auth, $tenant).await;
    assert_eq!(
        $failed
            .save_selected_candidate_draft($workspace, $actor, $session, &stale_save)
            .await,
        Err(Error::StaleRevision)
    );
    drop($failed);
    let stale_effects: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=7),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind($candidate).bind(stale_save.request_id).fetch_one(&$pool).await.unwrap();
    assert_eq!(stale_effects, (0, 0));
    $crate::scope_advisory::live_tests::scenarios::selected_observation::verify_independent(
        &$pool, &$store, &$enrollment, $tenant, $workspace, $session, &observe_request,
    ).await;
    };
}

pub(in super::super) use verify;
