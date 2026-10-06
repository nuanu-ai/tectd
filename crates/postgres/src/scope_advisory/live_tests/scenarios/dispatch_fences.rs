macro_rules! verify {
    ($actor:ident, $auth:ident, $candidate:ident, $dispatch:ident, $dispatch_id:ident, $enrollment:ident, $manifest:ident, $pool:ident, $runtime_pool:ident, $session:ident, $started:ident, $state:ident, $store:ident, $tenant:ident, $workspace:ident $(,)?) => {

    let dispatch_authorization = |opportunity_id, $dispatch_id| AdvisoryDispatchAuthorization {
        $dispatch_id,
        opportunity_id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: "fixture".into(),
        model: "jev".into(),
        configuration_snapshot: serde_json::json!({
            "provider_profile_ref": "fixture",
            "model_configuration": { "model": "jev" }
        }),
        configuration_digest: D.into(),
        material_digest: D.into(),
        payload_digest: D.into(),
        request_payload: b"fixture request".to_vec(),
    };

    // Authorization fences a stale authored source before inserting a dispatch.
    let authorize_stale_opportunity = Uuid::new_v4();
    let authorize_stale_request = format!("request-{authorize_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(authorize_stale_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(&authorize_stale_request).bind(D).execute(&$pool).await.unwrap();
    let authorize_stale_record = ScopeManifestRecord {
        opportunity_id: authorize_stale_opportunity,
        candidate_set_id: $candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        $manifest: $manifest.clone(),
    };
    let mut prepare = rw(&$store, &$enrollment.$auth, $tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(
            $workspace,
            &authorize_stale_record,
            &"c".repeat(64),
        )
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind($tenant).bind($workspace).bind($candidate).execute(&$pool).await.unwrap();
    let stale_authorization = dispatch_authorization(authorize_stale_opportunity, Uuid::new_v4());
    let mut tx = $runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind($tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        crate::advisory::authorize_dispatch_for_test(
            &mut tx,
            $tenant,
            $workspace,
            1,
            &stale_authorization,
        )
        .await,
        Err(Error::StaleContext)
    );
    tx.commit().await.unwrap();
    let stale_auth_dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind(authorize_stale_opportunity)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(stale_auth_dispatches, 0);

    // If the frozen source changes after authorization, dispatch-start is the
    // DB linearization point: it records cancellation and a terminal no-call,
    // leaving send_started_at and provider outcome absent.
    let start_stale_opportunity = Uuid::new_v4();
    let start_stale_request = format!("request-{start_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'4',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(start_stale_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(&start_stale_request).bind(D).execute(&$pool).await.unwrap();
    let mut current_manifest = $manifest.clone();
    current_manifest.source.candidate_set_revision = 4;
    reseal_manifest(&mut current_manifest);
    let start_stale_record = ScopeManifestRecord {
        opportunity_id: start_stale_opportunity,
        candidate_set_id: $candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        $manifest: current_manifest,
    };
    let mut prepare = rw(&$store, &$enrollment.$auth, $tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest($workspace, &start_stale_record, &"d".repeat(64))
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    let authorization = dispatch_authorization(start_stale_opportunity, Uuid::new_v4());
    let mut tx = $runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind($tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let authorized =
        crate::advisory::authorize_dispatch_for_test(&mut tx, $tenant, $workspace, 1, &authorization)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let $dispatch_id = authorized.id;
    sqlx::query("UPDATE scope_candidate_sets SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind($tenant).bind($workspace).bind($candidate).execute(&$pool).await.unwrap();
    let mut tx = $runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind($tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let $started = crate::advisory::start_dispatch_for_test(&mut tx, $tenant, $workspace, $dispatch_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!$started.should_send);
    assert_eq!($started.$dispatch.$state, AdvisoryDispatchState::Cancelled);
    assert_eq!(
        $started.$dispatch.send_certainty,
        AdvisorySendCertainty::NotSent
    );
    let dispatch_audit: (String, String, Option<String>, bool, bool, bool) = sqlx::query_as(
        "SELECT state,send_certainty,outcome,send_started_at IS NULL,response_payload IS NULL,sealed_at IS NOT NULL FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind($dispatch_id)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(
        dispatch_audit,
        (
            "cancelled".into(),
            "not_sent".into(),
            None,
            true,
            true,
            true
        )
    );
    let opportunity_audit: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind(start_stale_opportunity)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(
        opportunity_audit,
        ("no_call".into(), "deterministic_input_invalid".into())
    );

    let config_stale_opportunity = Uuid::new_v4();
    let config_stale_request = format!("request-{config_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'5',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(config_stale_opportunity).bind($tenant).bind($workspace).bind($candidate).bind($session).bind($actor)
        .bind(&config_stale_request).bind(D).execute(&$pool).await.unwrap();
    let mut config_manifest = $manifest.clone();
    config_manifest.source.candidate_set_revision = 5;
    reseal_manifest(&mut config_manifest);
    let config_stale_record = ScopeManifestRecord {
        opportunity_id: config_stale_opportunity,
        candidate_set_id: $candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        $manifest: config_manifest,
    };
    let mut prepare = rw(&$store, &$enrollment.$auth, $tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest($workspace, &config_stale_record, &"e".repeat(64))
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    let config_stale_authorization =
        dispatch_authorization(config_stale_opportunity, Uuid::new_v4());
    let mut tx = $runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind($tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let authorized = crate::advisory::authorize_dispatch_for_test(
        &mut tx,
        $tenant,
        $workspace,
        1,
        &config_stale_authorization,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,2,1,'disabled',NULL,NULL,$3,$4)")
        .bind($tenant).bind($workspace).bind($actor).bind($session).execute(&$pool).await.unwrap();
    sqlx::query("UPDATE advisory_workspace_config SET revision=2,mode='disabled',provider_profile_ref=NULL,model_configuration=NULL WHERE tenant_id=$1 AND workspace_id=$2")
        .bind($tenant).bind($workspace).execute(&$pool).await.unwrap();
    let mut tx = $runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind($tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let $started =
        crate::advisory::start_dispatch_for_test(&mut tx, $tenant, $workspace, authorized.id)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(!$started.should_send);
    assert_eq!($started.$dispatch.$state, AdvisoryDispatchState::Cancelled);
    assert_eq!(
        $started.$dispatch.send_certainty,
        AdvisorySendCertainty::NotSent
    );
    let config_stale_state: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind($tenant)
    .bind($workspace)
    .bind(config_stale_opportunity)
    .fetch_one(&$pool)
    .await
    .unwrap();
    assert_eq!(
        config_stale_state,
        ("invalidated".into(), "configuration_changed".into())
    );
    };
}

pub(in super::super) use verify;
