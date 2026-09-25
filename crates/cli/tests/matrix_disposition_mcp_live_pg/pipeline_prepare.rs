use super::*;
use crate::support::{ready_source_candidate, repository, review};

#[path = "pipeline_prepare/assertions.rs"]
mod assertions;

async fn disposable_pair_for_prepare() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").as_deref(),
        Ok(SYSTEM_ID)
    );
    assert_eq!(
        std::env::var("TECT_TEST_EXPECTED_DB_OID").as_deref(),
        Ok("16385")
    );
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    for (options, user) in [(&admin_options, "postgres"), (&runtime_options, "tect_ci")] {
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_socket().and_then(|p| p.to_str()), Some(SOCKET));
        assert_eq!(options.get_port(), 55479);
    }
    let pool = PgPool::connect_with(admin_options).await.unwrap();
    let identity: (i32, String, String, i64, String, i64) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         (SELECT max(version) FROM _sqlx_migrations)",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(identity.0, 180006);
    assert_eq!(identity.1, "tect_test");
    assert_eq!(identity.2, "postgres");
    assert_eq!(identity.3, DATABASE_OID);
    assert_eq!(identity.4, SYSTEM_ID);
    assert!(matches!(identity.5, 61..=64));
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 64);
    let runtime = PgPool::connect_with(runtime_options).await.unwrap();
    let role: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    ).fetch_one(&runtime).await.unwrap();
    assert_eq!(role, ("tect_test".into(), "tect_ci".into(), DATABASE_OID));
    (pool, runtime_url)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only pinned disposable PostgreSQL 18.6 fixture at migration 61-64"]
async fn public_prepare_binds_selected_work_and_independent_match_without_dispatch() {
    let (pool, runtime_url) = disposable_pair_for_prepare().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-prepare.sock");
    let matrix_calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false)))))
        .with_matrix_advisory_adapters(Arc::new(Provider(matrix_calls.clone())), Arc::new(Budget))
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        )),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("mcp-pipeline-prepare-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &owner_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
    let reopened = owner.call("open_workspace", json!({})).await;
    let workspace = Uuid::parse_str(reopened["workspace"]["id"].as_str().unwrap()).unwrap();
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;

    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":PROFILE},"model_configuration":{"model":MODEL}
        }),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record_task(&mut owner, task, &["a", "b"]).await;
    verify(&mut independent, &recorded, task).await;
    let advice_key = format!("matrix-prepare-{}", Uuid::new_v4());
    let advised = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,
            "request_key":advice_key
        }),
    )
    .await;
    assert_eq!(advised["state"], "advised");
    assert_eq!(matrix_calls.load(Ordering::SeqCst), 1);
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({"task_id":task,"request_key":advice_key}),
    )
    .await;
    let selection = disposition(
        &recorded,
        task,
        &advised,
        "after_advice",
        Some(&current["current_advice"]),
        json!({"outcome":"selected","selected_choice_id":"b"}),
    );
    let chosen = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        selection,
    )
    .await;
    let verification_digest: String = sqlx::query_scalar(
        "SELECT matrix_verification_digest FROM advisory_opportunity WHERE id=$1",
    )
    .bind(Uuid::parse_str(advised["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let matrix_selection = json!({
        "task_id":task,"task_revision":1,"disposition_id":chosen["disposition_id"],
        "selected_choice_id":"b","expected_input_digest":recorded["input_digest"],
        "expected_choice_set_digest":recorded["choice_set_digest"],
        "expected_verification_digest":verification_digest,
        "mapped_draft_node_indices":[0]
    });
    let opened = route(
        &mut owner,
        "command",
        "scope.open",
        json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let save =
        super::planning_effect::save_request(&opened["created"]["planning"], matrix_selection);
    let saved = route(&mut owner, "command", "slice.candidates.save", save.clone()).await;
    let set = Uuid::parse_str(saved["candidate_set"]["id"].as_str().unwrap()).unwrap();
    let caller_request = Uuid::parse_str(save["request_id"].as_str().unwrap()).unwrap();
    let work = saved["draft"]["nodes"][0].clone();
    let effect = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({"candidate_set_id":set,"caller_request_id":caller_request}),
    )
    .await;
    assert_eq!(effect["material"]["nodes"][0]["node_id"], work["id"]);
    let matched = route(
        &mut independent,
        "command",
        "engineering.matrix.planning_effect.verify",
        json!({"request_id":Uuid::new_v4(),"candidate_set_id":set,
            "caller_request_id":caller_request,
            "expected_result_revision":effect["material"]["result_revision"],
            "expected_effect_digest":effect["effect_digest"],
            "verdict":"matches","summary":"Saved Work node preserves selected choice b."}),
    )
    .await;
    assert_eq!(matched["verdict"], "matches");
    let ready = review(&mut owner, &saved).await;
    assert_eq!(ready["candidate_set"]["status"], "ready");
    assertions::exercise_prepare(
        &pool,
        workspace,
        &mut owner,
        &mut independent,
        set,
        &source,
        &work,
        &ready,
        &chosen,
        &matched,
        task,
    )
    .await;
    assert_eq!(matrix_calls.load(Ordering::SeqCst), 1);
    independent.finish().await;
    owner.finish().await;
    server.abort();
}
