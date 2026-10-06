use super::super::super::*;

pub(in super::super::super) async fn verify(
    pool: &sqlx::PgPool,
    store: &PgStore,
    enrollment: &admin::Enrollment,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    observe_request: &SelectedSaveObservationRequest,
) {
    let candidate = observe_request.candidate_set_id;
    let selected_opportunity = observe_request.opportunity_id;
    // A fresh verifier principal, not a second owner session, qualifies the
    // independent observation. Enrollment itself creates no session.
    let verifier = admin::prepare_verifier_enrollment(pool, tenant, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let independent_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(independent_session).bind(tenant).bind(verifier.auth.host_id).bind(workspace)
        .bind(independent_session.to_string()).execute(pool).await.unwrap();
    let mut qualified_request = observe_request.clone();
    qualified_request.request_id = Uuid::new_v4();
    qualified_request.session_id = independent_session;
    let mut owner_attempt = rw(store, &enrollment.auth, tenant).await;
    assert_eq!(
        owner_attempt
            .independently_observe_selected_scope_save(workspace, &qualified_request)
            .await,
        Err(Error::Forbidden)
    );
    drop(owner_attempt);
    let no_owner_qualified: i64 = sqlx::query_scalar("SELECT count(*) FROM advisory_scope_selected_save_observation WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(qualified_request.request_id).fetch_one(pool).await.unwrap();
    assert_eq!(no_owner_qualified, 0);
    let mut verifier_attempt = rw(store, &verifier.auth, tenant).await;
    let mut forged_session = qualified_request.clone();
    forged_session.session_id = session;
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &forged_session)
            .await,
        Err(Error::InvalidArguments)
    );
    let mut forged_caller = qualified_request.clone();
    forged_caller.caller_link_id = Uuid::new_v4();
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &forged_caller)
            .await,
        Err(Error::Forbidden)
    );
    let mut forged_receipt = qualified_request.clone();
    forged_receipt.caller_receipt_request_id = Uuid::new_v4();
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &forged_receipt)
            .await,
        Err(Error::Forbidden)
    );
    let mut wrong_workspace = qualified_request.clone();
    wrong_workspace.request_id = Uuid::new_v4();
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(Uuid::new_v4(), &wrong_workspace)
            .await,
        Err(Error::Forbidden)
    );
    let qualified_pass = verifier_attempt
        .independently_observe_selected_scope_save(workspace, &qualified_request)
        .await
        .unwrap();
    assert_eq!(qualified_pass.status, SelectedSaveObservationStatus::Passed);
    assert_eq!(qualified_pass.actor_id, verifier.principal_id);
    assert_eq!(qualified_pass.qualification, "independently_observed");
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &qualified_request)
            .await
            .unwrap(),
        qualified_pass
    );
    let second_independent_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(second_independent_session).bind(tenant).bind(verifier.auth.host_id).bind(workspace)
        .bind(second_independent_session.to_string()).execute(pool).await.unwrap();
    let mut changed_session = qualified_request.clone();
    changed_session.session_id = second_independent_session;
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &changed_session)
            .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        verifier_attempt
            .observe_selected_scope_save(workspace, &qualified_request)
            .await,
        Err(Error::InputConflict)
    );
    let mut changed_qualified = qualified_request.clone();
    changed_qualified.target_revision = 5;
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &changed_qualified)
            .await,
        Err(Error::Forbidden)
    );
    verifier_attempt.commit().await.unwrap();
    let mut audit_unit = rw(store, &enrollment.auth, tenant).await;
    let public_qualified = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity
        .selected_save_observation
        .unwrap();
    assert_eq!(public_qualified.id, qualified_pass.id);
    assert_eq!(public_qualified.qualification, "independently_observed");
    assert!(!public_qualified.establishes_independent_approval);
    assert!(!public_qualified.establishes_current_acceptance);
    drop(audit_unit);

    sqlx::query("UPDATE scope_candidate_drafts SET payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=6")
        .bind(tenant).bind(workspace).bind(candidate).execute(pool).await.unwrap();
    let mut failed_request = qualified_request.clone();
    failed_request.request_id = Uuid::new_v4();
    let mut verifier_attempt = rw(store, &verifier.auth, tenant).await;
    let qualified_fail = verifier_attempt
        .independently_observe_selected_scope_save(workspace, &failed_request)
        .await
        .unwrap();
    assert_eq!(qualified_fail.status, SelectedSaveObservationStatus::Failed);
    assert_eq!(qualified_fail.qualification, "independently_observed");
    assert!(
        qualified_fail
            .reason_codes
            .contains(&"saved_material_missing_or_mismatched".into())
    );
    verifier_attempt.commit().await.unwrap();
    let mut audit_unit = rw(store, &enrollment.auth, tenant).await;
    let public_failure = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity
        .selected_save_observation
        .unwrap();
    assert_eq!(public_failure.id, qualified_fail.id);
    assert!(!public_failure.establishes_independent_approval);
    assert!(!public_failure.establishes_current_acceptance);
    drop(audit_unit);
    sqlx::query("UPDATE scope_candidate_sets SET revision=7 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate).execute(pool).await.unwrap();
    let mut stale_qualified_request = qualified_request.clone();
    stale_qualified_request.request_id = Uuid::new_v4();
    let mut verifier_attempt = rw(store, &verifier.auth, tenant).await;
    let stale_qualified = verifier_attempt
        .independently_observe_selected_scope_save(workspace, &stale_qualified_request)
        .await
        .unwrap();
    assert_eq!(
        stale_qualified.status,
        SelectedSaveObservationStatus::Failed
    );
    assert_eq!(stale_qualified.qualification, "independently_observed");
    assert!(
        stale_qualified
            .reason_codes
            .contains(&"candidate_revision_stale".into())
    );
    verifier_attempt.commit().await.unwrap();
}
