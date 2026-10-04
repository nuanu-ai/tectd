use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn erased_origin_begin_replay_refuses_the_frozen_knowledge_copy() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("erased-origin.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk2-erased-origin-{}", Uuid::new_v4()),
    );
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("origin-{}", Uuid::new_v4()),
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope=route(&mut client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
    let mut pipeline_draft = lightweight_draft();
    pipeline_draft["nodes"][0]["pipeline"] = json!("slice.custom-procedure-capture");
    pipeline_draft["nodes"][0]["pipeline_reason"] =
        json!("Exercise frozen origin replay without invoking Promotion routing.");
    let saved = save(
        &mut client,
        &opened_scope["created"]["planning"],
        pipeline_draft,
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let opened_slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let committed = commit_create(&mut client, fixture["document"].clone()).await;
    let unit = committed.receipt["applied_operations"][0]["unit_id"].clone();
    let persisted_pipeline: String =
        sqlx::query_scalar("SELECT pipeline FROM native_slices WHERE id=$1")
            .bind(Uuid::parse_str(opened_slice["created"]["id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(persisted_pipeline, "slice.custom-procedure-capture");
    let begin_request = json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":opened_slice["created"]["id"],"slice_revision":opened_slice["created"]["revision"],
        "qualification_reason":"Frozen erased-origin replay fixture."});
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        begin_request.clone(),
    )
    .await;
    assert_eq!(
        begun["created"]["knowledge_resources"]["selected"][0]["unit_id"],
        unit
    );
    let retracted = commit_single(
        &mut client,
        SingleOperation {
            operation: "retract",
            unit_id: Some(unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        retracted["applied"]["applied_operations"][0]["operation"],
        "retract"
    );
    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":begun["created"]["run"]["id"]}),
    )
    .await;
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    assert_eq!(stale["knowledge_resource_status"]["state"], "needs_context");
    assert!(
        stale["knowledge_resource_status"]["changed_unit_ids"]
            .as_array()
            .unwrap()
            .contains(&unit)
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "pipeline.knowledge_refresh",
            action_params(refresh).clone(),
        )
        .await["error"]["code"],
        "needs_context"
    );
    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(unit),
            expected_revision: Some(1),
            expected_lifecycle: Some("retracted"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "owned_live_copies",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        erased["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.begin",
            begin_request
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    client.finish().await;
    pool.close().await;
}
