//! Explicit mismatch follow-up; synthetic facts, no provider or HTTP adapter.
use super::*;

pub(super) const OWNED_MIGRATION: i64 = 103;

pub(super) async fn guard(pool: &PgPool) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let identity: (i32, String, String, i64, String, i64, i64, bool) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         (SELECT max(version) FROM _sqlx_migrations),(SELECT count(*) FROM _sqlx_migrations),\
         (SELECT bool_and(success) FROM _sqlx_migrations)",
    )
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            "tect_test".into(),
            "postgres".into(),
            16385,
            "7689676854994613066".into(),
            OWNED_MIGRATION,
            OWNED_MIGRATION,
            true
        )
    );
}

async fn call(pool: &PgPool, client: &mut Mcp, name: &str, params: Value) -> Value {
    guard(pool).await;
    route(client, "command", name, params).await
}

async fn counts(pool: &PgPool, workspace: Uuid) -> (i64, i64, i64) {
    sqlx::query_as("SELECT \
        (SELECT count(*) FROM scope_candidate_sets WHERE workspace_id=$1),\
        (SELECT count(*) FROM advisory_opportunity WHERE workspace_id=$1 AND capability='scope_decomposition'),\
        (SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1)")
        .bind(workspace).fetch_one(pool).await.unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires the owned PG18.6 system7689676854994613066 at migration103"]
async fn public_matrix_mismatch_requires_explicit_revision_bound_decomposition() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    for (url, user) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let options = PgConnectOptions::from_str(url).unwrap();
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), 64775);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_username(), user);
    }
    let pool = PgPool::connect(&admin_url).await.unwrap();
    guard(&pool).await;
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())")
        .fetch_one(&runtime).await.unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), "tect_ci".into(), 16385)
    );
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let socket = root.join("parent.sock");
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false))))),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    // Administrative enrollment is restricted to this identity-pinned fixture.
    guard(&pool).await;
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let key = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(&socket, &owner_config, &Uuid::new_v4().to_string(), &key).await;
    let opened = call(&pool, &mut owner, "workspace.open", json!({})).await;
    let workspace = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let registered = call(&pool, &mut owner, "source.register", json!({"path":repo})).await;
    call(
        &pool,
        &mut owner,
        "session.select_worktrees",
        json!({"worktree_ids":[registered["id"]]}),
    )
    .await;
    let begun = call(
        &pool,
        &mut owner,
        "program.begin",
        json!({"request_id":Uuid::new_v4(),"input":"Bound the explicit engineering mismatch."}),
    )
    .await;
    let mut save = json!({"program_id":begun["program"]["id"],"revision":1,"input_cursor":1,
        "name":"Mismatch follow-up","intent":"Bound the engineering mismatch",
        "basis":"Matrix approaches need decomposition","boundaries":"Diagnosis only",
        "constraints":"No deployment","success":"Bounded scope","complete":true});
    let manifest = &begun["program"]["planning_knowledge"]["manifest"];
    if manifest["id"].is_string() {
        save["consumed_knowledge"] = json!({"manifest_id":manifest["id"],"digest":manifest["digest"],"workspace_generation":manifest["workspace_generation"]});
    }
    let program = call(&pool, &mut owner, "program.save", save).await;
    guard(&pool).await;
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let mut independent =
        Mcp::start(&socket, &verifier_config, &Uuid::new_v4().to_string(), &key).await;
    call(&pool, &mut independent, "workspace.open", json!({})).await;
    let task = Uuid::new_v4();
    guard(&pool).await;
    let recorded = record_task(&mut owner, task, &["a", "b"]).await;
    guard(&pool).await;
    verify(&mut independent, &recorded, task).await;
    let opportunity = call(
        &pool,
        &mut owner,
        "engineering.advisory.request",
        json!({"task_id":task,"expected_task_revision":1,"request_key":format!("parent-{task}")}),
    )
    .await;
    assert_eq!(opportunity["state"], "no_call");
    let mismatch =
        "The owner-authored alternatives require a smaller decomposition before selection";
    let blocked = call(
        &pool,
        &mut owner,
        "engineering.matrix.disposition.record",
        disposition(
            &recorded,
            task,
            &opportunity,
            "no_call",
            None,
            json!({"outcome":"blocked","blocked_reason":mismatch}),
        ),
    )
    .await;
    assert!(blocked["disposition_id"].is_string());
    assert_eq!(counts(&pool, workspace).await, (0, 0, 0));
    let parent_id = Uuid::parse_str(opportunity["opportunity_id"].as_str().unwrap()).unwrap();
    let binding: (Option<String>, String) = sqlx::query_as("SELECT matrix_verification_digest,material_digest FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(parent_id).fetch_one(&pool).await.unwrap();
    let request = json!({"request_id":Uuid::new_v4(),"program_id":program["program"]["id"],"program_revision":program["program"]["revision"],"boundary":"ongoing","input":"Explicitly decompose the recorded Matrix mismatch.",
        "parent_matrix":{"opportunity_id":parent_id,"task_id":task,"task_revision":1,"input_digest":recorded["input_digest"],"choice_set_digest":recorded["choice_set_digest"],"verification_digest":binding.0,"opportunity_material_digest":binding.1,"mismatch_rationale":mismatch}});
    let created = call(&pool, &mut owner, "scope.candidates.begin", request.clone()).await;
    assert_eq!(created["disposition"], "created");
    assert_eq!(counts(&pool, workspace).await, (1, 1, 0));
    let replay = call(&pool, &mut owner, "scope.candidates.begin", request.clone()).await;
    assert_eq!(replay["disposition"], "replay");
    assert_eq!(
        created["context"]["candidate_set"]["id"],
        replay["context"]["candidate_set"]["id"]
    );
    assert_eq!(counts(&pool, workspace).await, (1, 1, 0));
    // Runtime-role audit uses the enrolled principal/tenant; no admin authority
    // is used to inspect or modify the canonical lineage payload.
    let mut tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(enrolled.tenant_id.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let lineage: (Value, Uuid) = sqlx::query_as("SELECT s.origin_payload,o.parent_opportunity_id FROM scope_candidate_sets s JOIN advisory_opportunity o ON o.workspace_id=s.workspace_id AND o.request_key=s.origin_request_id::text WHERE s.workspace_id=$1 AND o.capability='scope_decomposition'")
        .bind(workspace).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(lineage.0["parent_matrix"], request["parent_matrix"]);
    assert_eq!(lineage.1, parent_id);
    let child: (String, Uuid, Uuid, Uuid, i64) = sqlx::query_as(
        "SELECT c.state,c.session_id,c.authorized_actor_id,c.work_item_id,c.source_revision::bigint \
         FROM advisory_opportunity c JOIN advisory_opportunity p ON p.id=c.parent_opportunity_id \
         WHERE c.workspace_id=$1 AND c.parent_opportunity_id=$2 \
         AND c.session_id=p.session_id AND c.authorized_actor_id=p.authorized_actor_id",
    )
    .bind(workspace)
    .bind(parent_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(child.0, "no_call");
    assert_eq!(
        child.3.to_string(),
        program["program"]["id"].as_str().unwrap()
    );
    assert_eq!(child.4, program["program"]["revision"].as_i64().unwrap());
    tx.commit().await.unwrap();
    for field in [
        "task_revision",
        "input_digest",
        "verification_digest",
        "opportunity_material_digest",
    ] {
        let mut changed = request.clone();
        changed["request_id"] = json!(Uuid::new_v4());
        changed["parent_matrix"][field] = if field == "task_revision" {
            json!(2)
        } else {
            json!("0".repeat(64))
        };
        guard(&pool).await;
        assert_error(
            &route_error(&mut owner, "command", "scope.candidates.begin", changed).await,
            &["stale_revision", "input_conflict"],
        );
        assert_eq!(counts(&pool, workspace).await, (1, 1, 0));
    }
    // Advance facts only through the immutable public TaskRevision contract.
    let mut next_choice = choice(task, &["a", "b"]);
    next_choice["task_revision"] = json!("2");
    call(
        &pool,
        &mut owner,
        "task.source.record",
        json!({
            "task_id":task,"revision":2,"expected_current_revision":1,
            "request_id":Uuid::new_v4(),"input":input(),"choice_set":next_choice
        }),
    )
    .await;
    let committed_replay = call(&pool, &mut owner, "scope.candidates.begin", request.clone()).await;
    assert_eq!(committed_replay["disposition"], "replay");
    assert_eq!(
        committed_replay["context"]["candidate_set"]["id"],
        created["context"]["candidate_set"]["id"]
    );
    let mut stale = request.clone();
    stale["request_id"] = json!(Uuid::new_v4());
    guard(&pool).await;
    assert_error(
        &route_error(&mut owner, "command", "scope.candidates.begin", stale).await,
        &["stale_revision"],
    );
    assert_eq!(counts(&pool, workspace).await, (1, 1, 0));
    println!(
        "public Matrix no_call -> explicit blocked mismatch -> Scope created/replay; canonical parent/task revision/digests preserved; changed bindings denied; zero dispatches, no provider/HTTP adapter"
    );
    independent.finish().await;
    owner.finish().await;
    server.abort();
    let _ = server.await;
}
