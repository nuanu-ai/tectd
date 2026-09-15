use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn upgraded_dk1_is_hidden_from_high_inquiry_and_retained_for_slice_topic() {
    if std::env::var("TECT_TEST_DK1_UPGRADE").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let runtime_role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let legacy_admin = std::env::var("TECT_LEGACY_ADMIN").unwrap();
    let legacy_daemon = std::env::var("TECT_LEGACY_DAEMON").unwrap();
    let legacy_mcp = std::env::var("TECT_LEGACY_MCP").unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let config = root.join("legacy-host.json");
    run_legacy_admin(
        Path::new(&legacy_admin),
        &admin_url,
        &[
            "migrate",
            "--runtime-role",
            &runtime_role,
            "--enable-durable-knowledge",
        ],
    );
    run_legacy_admin(
        Path::new(&legacy_admin),
        &admin_url,
        &["enroll", "--out", config.to_str().unwrap()],
    );
    let socket = root.join("inquiry-legacy-upgrade.sock");
    let native = Uuid::new_v4().to_string();
    let workspace_key = format!("inquiry-legacy-upgrade-{}", Uuid::new_v4());
    let old_daemon = LegacyDaemon::start(
        Path::new(&legacy_daemon),
        &tagged_url(&runtime_url, "inquiry-legacy-daemon"),
        socket.clone(),
    )
    .await;
    let mut old = LegacyMcp::start(
        Path::new(&legacy_mcp),
        &socket,
        &config,
        &native,
        &workspace_key,
    )
    .await;
    legacy_route(&mut old, "workspace.open", json!({})).await;
    let prepared = legacy_route(
        &mut old,
        "knowledge.change_prepare",
        json!({"request_id":Uuid::new_v4(),"operation":"create","expected_generation":0,
            "draft":{"title":"Legacy inquiry fixture","statement":LEGACY_MARKER,
                "modality":"must","action":"retain","target_iri":"urn:fixture:legacy",
                "conditions":[],"exceptions":[],
                "source":{"title":"legacy source","uri":"urn:fixture:legacy:source",
                    "text":"legacy source"},
                "binding":{"kind":"workspace"},"purpose":"execution_constraint",
                "version_resolution":"current_accepted"},
            "reason":"Publish a real DK-1 record before the current-schema upgrade.",
            "authority_basis":"Authenticated legacy workspace owner."}),
    )
    .await;
    let change = &prepared["prepared"];
    let approved = legacy_route(
        &mut old,
        "knowledge.change_review",
        json!({"request_id":Uuid::new_v4(),"change_id":change["id"],
            "change_revision":change["change_revision"],
            "proposal_digest":change["proposal_digest"],"verdict":"approve",
            "review_summary":"Reviewed the exact legacy inquiry fixture.",
            "method_read":{"id":change["review_method"]["id"],
                "version":change["review_method"]["version"],
                "digest":change["review_method"]["digest"]}}),
    )
    .await;
    let ready = &approved["approved"];
    let published = legacy_route(
        &mut old,
        "knowledge.change_publish",
        json!({"request_id":Uuid::new_v4(),"change_id":ready["id"],
            "change_revision":ready["change_revision"],
            "proposal_digest":ready["proposal_digest"]}),
    )
    .await;
    let legacy_unit = published["published"]["unit_id"].clone();
    old.finish().await;
    old_daemon.stop().await;

    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &runtime_role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &runtime_role)
        .await
        .unwrap();
    let tenant: Uuid = sqlx::query_scalar("SELECT tenant_id FROM workspaces WHERE key=$1")
        .bind(&workspace_key)
        .fetch_one(&pool)
        .await
        .unwrap();
    let current_enrollment = admin::enroll_host(
        &pool,
        Some(tenant),
        vec![root.to_string_lossy().into_owned()],
    )
    .await
    .unwrap();
    let current_config = root.join("current-host.json");
    host_file(&current_config, &current_enrollment.auth);
    let _daemon = Daemon::start(
        &tagged_url(&runtime_url, "inquiry-current-daemon"),
        socket.clone(),
    )
    .await;
    let mut current = Mcp::start(&socket, &current_config, &native, &workspace_key).await;
    current.call("open_workspace", json!({})).await;
    let (scope, slices) = open_targets(&mut current, &pool, &repo).await;
    let matched = json!({"target_iris":["urn:fixture:r1"],
        "environment_iris":["urn:fixture:prod"],"action_classes":["deploy"]});
    let program = begin(
        &mut current,
        &scope,
        &slices[0],
        inquiry("program", matched.clone()),
    )
    .await;
    assert_eq!(
        program["knowledge_resources"]["projection_policy"],
        "program_planning_briefs"
    );
    assert_eq!(program["knowledge"]["selected"], json!([]));
    assert!(
        program["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!program.to_string().contains(LEGACY_MARKER));
    assert!(!program.to_string().contains(legacy_unit.as_str().unwrap()));

    let slice = begin(&mut current, &scope, &slices[1], inquiry("slice", matched)).await;
    assert_eq!(
        slice["knowledge_resources"]["projection_policy"],
        "full_resources"
    );
    assert_eq!(selected_for(&slice, &legacy_unit).len(), 1);
    assert_eq!(
        selected_for(&slice, &legacy_unit)[0]["canonical_text"],
        LEGACY_MARKER
    );
    assert!(
        slice["knowledge"]["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["unit_id"] == legacy_unit)
    );

    current.finish().await;
    pool.close().await;
}
