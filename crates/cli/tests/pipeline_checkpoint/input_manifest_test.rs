use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn checkpoint_resolution_rebinds_changed_dk_and_completes_from_returned_context() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated admin URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("dedicated runtime URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("dedicated runtime role required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    enable_durable_knowledge(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("checkpoint-input-manifest.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("checkpoint-input-manifest-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("checkpoint-input-manifest-{}", Uuid::new_v4()),
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],
        "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let scope = ScopeOpenFixture::from_mutation(scope, "created");
    let planning = scope.read_planning(&mut client).await.value;
    let saved = save(&mut client, &planning, brainstorming_draft()).await;
    let reviewed = review(&mut client, &saved).await;
    let slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let scope_id = Uuid::parse_str(reviewed["scope"]["id"].as_str().unwrap()).unwrap();
    let program_id: Uuid = sqlx::query_scalar(
        "SELECT sc.program_id FROM native_scopes ns JOIN scope_candidate_sets sc ON sc.tenant_id=ns.tenant_id AND sc.workspace_id=ns.workspace_id AND sc.id=ns.source_candidate_set_id WHERE ns.id=$1",
    ).bind(scope_id).fetch_one(&pool).await.unwrap();
    let public = commit_create(
        &mut client,
        knowledge_document(
            "checkpoint-resume",
            "urn:fixture:public-decision",
            "workspace_members",
            program_id,
        ),
    )
    .await;
    let unit = public.receipt["applied_operations"][0]["unit_id"].clone();
    let mut producer = begin_run(
        &mut client,
        &reviewed["scope"],
        &slice["created"],
        decision_inquiry(),
        None,
    )
    .await;
    while producer.run()["current_phase_ordinal"].as_u64().unwrap() < 5 {
        producer = advance(&mut client, producer).await;
    }
    assert!(contains_unit(&producer, &unit));
    let checkpoint = create_checkpoint(&mut client, &producer).await;
    let waiting = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":checkpoint["producer_run_id"]}),
    )
    .await;
    let waiting = resolve_pipeline(&mut client, waiting).await.unwrap();
    assert_eq!(waiting.run()["status"], "waiting_input");
    let generation: i64 = sqlx::query_scalar(
        "UPDATE workspace_knowledge_state state SET generation=state.generation+1 FROM slice_pipeline_runs run WHERE run.id=$1 AND state.tenant_id=run.tenant_id AND state.workspace_id=run.workspace_id RETURNING state.generation",
    ).bind(checkpoint["producer_run_id"].as_str().unwrap().parse::<Uuid>().unwrap())
        .fetch_one(&pool).await.unwrap();
    let resolved = route(
        &mut client,
        "command",
        "slice.pipeline.checkpoint.resolve",
        json!({
        "request_id":Uuid::new_v4(),"producer_run_id":checkpoint["producer_run_id"],
        "producer_run_revision":checkpoint["producer_run_revision"],
        "checkpoint":checkpoint["checkpoint"],"action":"cancel",
        "reason":"Cancel after a changed DK generation to reassess the decision."}),
    )
    .await;
    let current = resolve_pipeline(&mut client, resolved).await.unwrap();
    assert_eq!(resolved_checkpoint(&current)["status"], "cancelled");
    assert_eq!(
        current.details_data()["knowledge_resource_status"]["state"],
        "current"
    );
    assert_eq!(
        current.details_data()["knowledge_resources"]["run_revision"],
        current.run()["revision"]
    );
    assert_eq!(
        current.details_data()["knowledge_resources"]["workspace_generation"],
        generation
    );
    assert_ne!(
        current.details_data()["knowledge_resources"]["id"],
        waiting.details_data()["knowledge_resources"]["id"]
    );
    assert!(contains_unit(&current, &unit));
    let reworked = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(
            &current,
            "rework",
            "completed",
            "continue",
            Some("B04"),
            None,
        ),
    )
    .await;
    let reworked = resolve_pipeline(&mut client, reworked).await.unwrap();
    assert_eq!(reworked.run()["current_phase_id"], "B04");
}
