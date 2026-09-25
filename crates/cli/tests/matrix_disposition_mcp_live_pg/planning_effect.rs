use super::*;
use crate::support::{ready_source_candidate, repository};

// Kept separate from the migration-58/59 guards used by the older tests.
async fn disposable_pair_for_effect() -> (PgPool, String) {
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
    assert!(matches!(identity.5, 60 | 61));
    let runtime = PgPool::connect_with(runtime_options).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    ).fetch_one(&runtime).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), DATABASE_OID)
    );
    (pool, runtime_url)
}

pub(super) fn save_request(planning: &Value, selection: Value) -> Value {
    let mut request = json!({
        "kind":"draft","scope_id":planning["scope"]["id"],
        "candidate_set_id":planning["candidate_set"]["id"],
        "revision":planning["candidate_set"]["revision"],
        "snapshot_id":planning["snapshot"]["id"],
        "input_cursor":planning["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),
        "draft":{"coverage_summary":"Selected Matrix choice b bounds this diagnosis",
            "nodes":[{"kind":"work","identity":{"local":"choice-b"},
                "title":"Investigate selected approach b",
                "outcome":"The selected approach has a documented diagnosis",
                "includes":["selected approach b"],"excludes":["deployment"],
                "dependencies":[],"proof":["Synthetic diagnosis is recorded"],
                "pipeline":"slice.debug-root-cause",
                "pipeline_reason":"The selected approach needs a bounded diagnosis",
                "source_result_ids":[]}],"supersessions":[]},
        "matrix_selection":selection
    });
    let manifest = &planning["planning_knowledge"]["manifest"];
    if manifest["id"].as_str().is_some() {
        request["consumed_knowledge"] = json!({
            "manifest_id":manifest["id"],"digest":manifest["digest"],
            "workspace_generation":manifest["workspace_generation"]
        });
    }
    request
}

async fn attestation_count(pool: &PgPool, workspace: Uuid, set: Uuid) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM matrix_planning_effect_attestations WHERE workspace_id=$1 AND candidate_set_id=$2")
        .bind(workspace).bind(set).fetch_one(pool).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only pinned disposable PostgreSQL 18.6 fixture at migration 60 or 61"]
async fn independent_verifier_attests_exact_saved_matrix_effect() {
    let (pool, runtime_url) = disposable_pair_for_effect().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 61);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("matrix-effect.sock");
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false)))))
        .with_matrix_advisory_adapters(Arc::new(Provider(calls.clone())), Arc::new(Budget)),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("mcp-matrix-effect-{}", Uuid::new_v4());
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
    let advice_key = format!("effect-{}", Uuid::new_v4());
    let advised = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":advice_key
        }),
    )
    .await;
    assert_eq!(advised["state"], "advised");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":advice_key
        }),
    )
    .await;
    let selected = disposition(
        &recorded,
        task,
        &advised,
        "after_advice",
        Some(&current["current_advice"]),
        json!({"outcome":"selected","selected_choice_id":"b"}),
    );
    let disposition = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        selected,
    )
    .await;
    let verification_digest: String = sqlx::query_scalar(
        "SELECT matrix_verification_digest FROM advisory_opportunity WHERE id=$1",
    )
    .bind(Uuid::parse_str(advised["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let selection = json!({
        "task_id":task,"task_revision":1,"disposition_id":disposition["disposition_id"],
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
            "request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let request = save_request(&opened["created"]["planning"], selection);
    let saved = route(
        &mut owner,
        "command",
        "slice.candidates.save",
        request.clone(),
    )
    .await;
    let set = Uuid::parse_str(request["candidate_set_id"].as_str().unwrap()).unwrap();
    let caller_request = Uuid::parse_str(request["request_id"].as_str().unwrap()).unwrap();
    let before: (i64, String) = sqlx::query_as(
        "SELECT revision,status FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(set)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(attestation_count(&pool, workspace, set).await, 0);
    let get_args = json!({"candidate_set_id":set,"caller_request_id":caller_request});
    let read = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        get_args.clone(),
    )
    .await;
    assert_eq!(read["material"]["selected_choice"]["candidate_id"], "b");
    assert_eq!(
        read["material"]["caller_principal_id"],
        read["material"]["matrix_owner_principal_id"]
    );
    assert_eq!(
        read["material"]["selected_choice"]["approach"],
        "Synthetic approach b"
    );
    assert_eq!(
        read["material"]["result_revision"],
        saved["candidate_set"]["revision"]
    );
    assert_eq!(read["material"]["nodes"][0]["draft_index"], 0);
    assert_eq!(
        read["material"]["nodes"][0]["node_id"],
        saved["draft"]["nodes"][0]["id"]
    );
    assert_eq!(
        read["material"]["nodes"][0]["node_revision"],
        saved["draft"]["nodes"][0]["revision"]
    );
    assert_eq!(
        read["material"]["nodes"][0]["body"],
        saved["draft"]["nodes"][0]
    );
    assert_eq!(read["effect_digest"].as_str().unwrap().len(), 64);

    let verify_args = json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller_request,
        "expected_result_revision":read["material"]["result_revision"],
        "expected_effect_digest":read["effect_digest"],
        "verdict":"matches","summary":"Saved node 0 implements selected choice b."
    });
    for route_name in [
        "engineering.matrix.planning_effect.get",
        "engineering.matrix.planning_effect.verify",
    ] {
        let (tool, args) = if route_name.ends_with(".get") {
            ("query", get_args.clone())
        } else {
            ("command", verify_args.clone())
        };
        assert_error(
            &route_error(&mut owner, tool, route_name, args).await,
            &["forbidden"],
        );
    }
    let mut wrong_digest = verify_args.clone();
    wrong_digest["request_id"] = json!(Uuid::new_v4());
    wrong_digest["expected_effect_digest"] = json!("0".repeat(64));
    assert_error(
        &route_error(
            &mut independent,
            "command",
            "engineering.matrix.planning_effect.verify",
            wrong_digest,
        )
        .await,
        &["input_conflict"],
    );
    let mut stale = verify_args.clone();
    stale["request_id"] = json!(Uuid::new_v4());
    stale["expected_result_revision"] = json!(before.0 + 1);
    assert_error(
        &route_error(
            &mut independent,
            "command",
            "engineering.matrix.planning_effect.verify",
            stale,
        )
        .await,
        &["stale_revision"],
    );
    assert_eq!(attestation_count(&pool, workspace, set).await, 0);

    let receipt = route(
        &mut independent,
        "command",
        "engineering.matrix.planning_effect.verify",
        verify_args.clone(),
    )
    .await;
    assert_eq!(receipt["verdict"], "matches");
    assert_eq!(receipt["effect_digest"], read["effect_digest"]);
    let replay = route(
        &mut independent,
        "command",
        "engineering.matrix.planning_effect.verify",
        verify_args.clone(),
    )
    .await;
    assert_eq!(replay, receipt);
    let mut conflict = verify_args.clone();
    conflict["summary"] = json!("Different summary on the same request id.");
    assert_error(
        &route_error(
            &mut independent,
            "command",
            "engineering.matrix.planning_effect.verify",
            conflict,
        )
        .await,
        &["input_conflict"],
    );
    assert_eq!(attestation_count(&pool, workspace, set).await, 1);
    let row: (Uuid, Uuid, Uuid, i64, String, Uuid, Uuid, String, Uuid, Uuid) = sqlx::query_as(
        "SELECT a.candidate_set_id,a.caller_request_id,a.verifier_request_id,a.result_revision,\
         a.effect_digest,a.verifier_principal_id,a.verifier_session_id,a.verdict,\
         l.candidate_set_id,r.request_id FROM matrix_planning_effect_attestations a \
         JOIN matrix_planning_selection_links l ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=\
            (a.tenant_id,a.workspace_id,a.candidate_set_id,a.caller_request_id)\
         JOIN native_planning_receipts r ON (r.tenant_id,r.workspace_id,r.entity_id,r.operation,r.request_id)=\
            (a.tenant_id,a.workspace_id,a.candidate_set_id,a.operation,a.caller_request_id)\
         WHERE a.workspace_id=$1 AND a.candidate_set_id=$2 AND a.verifier_request_id=$3"
    ).bind(workspace).bind(set).bind(Uuid::parse_str(verify_args["request_id"].as_str().unwrap()).unwrap())
     .fetch_one(&pool).await.unwrap();
    assert_eq!(
        (row.0, row.1, row.2, row.3),
        (
            set,
            caller_request,
            Uuid::parse_str(verify_args["request_id"].as_str().unwrap()).unwrap(),
            before.0
        )
    );
    assert_eq!(row.4, read["effect_digest"]);
    assert_eq!(json!(row.5), receipt["verifier_principal_id"]);
    assert_eq!(json!(row.6), receipt["verifier_session_id"]);
    assert_eq!(row.7, "match");
    assert_eq!((row.8, row.9), (set, caller_request));
    let after: (i64, String) = sqlx::query_as(
        "SELECT revision,status FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(set)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(after, before);
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    independent.finish().await;
    owner.finish().await;
    server.abort();
}
