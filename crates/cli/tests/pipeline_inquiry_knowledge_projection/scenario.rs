use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn inquiry_topic_projects_only_exact_brief_height_and_preserves_full_slice_delivery() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("inquiry-projection.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("inquiry-projection-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("owner-host.json");
    host_file(&config, &enrollment.auth);
    let matched = json!({"target_iris":["urn:fixture:r1"],
        "environment_iris":["urn:fixture:prod"],"action_classes":["deploy"]});
    let empty_workspace = format!("inquiry-no-dk-{}", Uuid::new_v4());
    let mut inactive = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &empty_workspace,
    )
    .await;
    let (empty_scope, empty_slices) = open_targets(&mut inactive, &pool, &repo).await;
    let inactive_run = begin(
        &mut inactive,
        &empty_scope,
        &empty_slices[0],
        inquiry("program", matched.clone()),
    )
    .await;
    assert!(inactive_run["knowledge"].is_null());
    assert!(inactive_run["knowledge_resources"].is_null());
    assert_eq!(
        inactive_run["knowledge_resource_status"]["state"],
        "inactive"
    );
    inactive.finish().await;

    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let workspace_key = format!("inquiry-projection-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (scope, slices) = open_targets(&mut owner, &pool, &repo).await;
    let scope_id = Uuid::parse_str(scope["id"].as_str().unwrap()).unwrap();
    let program_id: Uuid = sqlx::query_scalar(
        "SELECT sc.program_id FROM native_scopes ns JOIN scope_candidate_sets sc ON sc.tenant_id=ns.tenant_id AND sc.workspace_id=ns.workspace_id AND sc.id=ns.source_candidate_set_id WHERE ns.id=$1",
    )
    .bind(scope_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let main_bindings = json!([
        binding(json!({"kind":"workspace"}), "required"),
        binding(
            json!({"kind":"program","program_id":program_id}),
            "required"
        ),
        binding(json!({"kind":"scope","scope_id":scope_id}), "required"),
        binding(
            json!({"kind":"slice","scope_id":scope_id,"slice_id":slices[2]["id"]}),
            "required"
        )
    ]);
    let main = commit_create(
        &mut owner,
        document("main", main_bindings, "workspace_members", true),
    )
    .await;
    let main_unit = main.receipt["applied_operations"][0]["unit_id"].clone();
    let private = commit_create(
        &mut owner,
        document(
            "private",
            json!([binding(json!({"kind":"workspace"}), "reference")]),
            "owners_only",
            true,
        ),
    )
    .await;
    let private_unit = private.receipt["applied_operations"][0]["unit_id"].clone();
    let no_brief = commit_create(
        &mut owner,
        document(
            "detailed-without-brief",
            json!([binding(json!({"kind":"workspace"}), "required")]),
            "workspace_members",
            false,
        ),
    )
    .await;
    let no_brief_unit = no_brief.receipt["applied_operations"][0]["unit_id"].clone();
    let program = begin(
        &mut owner,
        &scope,
        &slices[0],
        inquiry("program", matched.clone()),
    )
    .await;
    assert_eq!(
        program["knowledge_resources"]["projection_policy"],
        "program_planning_briefs"
    );
    assert_eq!(
        program["knowledge_resources"]["inquiry"]["topic_level"],
        "program"
    );
    assert_eq!(program["knowledge"]["selected"], json!([]));
    let program_resources = selected_for(&program, &main_unit);
    assert_eq!(program_resources.len(), 2);
    assert_eq!(
        program_resources
            .iter()
            .map(|value| value["binding"]["target"]["kind"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        ["program", "workspace"].into_iter().collect()
    );
    program_resources
        .iter()
        .for_each(|value| assert_projected(value, PROGRAM_INSTRUCTION, "program-r1"));
    assert_eq!(selected_for(&program, &private_unit).len(), 1);
    assert!(selected_for(&program, &no_brief_unit).is_empty());
    assert!(
        program["knowledge_resources"]["unresolved_needs"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    assert!(!program.to_string().contains(LEGACY_MARKER));
    let program_current = route(
        &mut owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":program["run"]["id"]}),
    )
    .await;
    let completion = find_action(&program_current, "slice.pipeline.phase.complete").unwrap();
    assert_eq!(
        action_params(completion)["consumed_knowledge"],
        json!({"manifest_id":program["knowledge_resources"]["id"],
            "digest":program["knowledge_resources"]["digest"]})
    );

    let scope_topic = begin(
        &mut owner,
        &scope,
        &slices[1],
        inquiry("scope", matched.clone()),
    )
    .await;
    assert_eq!(
        scope_topic["knowledge_resources"]["projection_policy"],
        "scope_planning_briefs"
    );
    let scope_resources = selected_for(&scope_topic, &main_unit);
    assert_eq!(scope_resources.len(), 3);
    assert_eq!(
        scope_resources
            .iter()
            .map(|value| value["binding"]["target"]["kind"].as_str().unwrap())
            .collect::<BTreeSet<_>>(),
        ["program", "scope", "workspace"].into_iter().collect()
    );
    scope_resources
        .iter()
        .for_each(|value| assert_projected(value, SCOPE_INSTRUCTION, "scope-r1"));
    assert_eq!(scope_topic["knowledge"]["selected"], json!([]));
    assert!(!scope_topic.to_string().contains(LEGACY_MARKER));

    let slice_topic = begin(
        &mut owner,
        &scope,
        &slices[2],
        inquiry("slice", matched.clone()),
    )
    .await;
    assert_eq!(
        slice_topic["knowledge_resources"]["projection_policy"],
        "full_resources"
    );
    let slice_resources = selected_for(&slice_topic, &main_unit);
    assert_eq!(slice_resources.len(), 4);
    assert!(
        slice_resources
            .iter()
            .all(|value| value["canonical_text"] == DETAIL_MARKER)
    );
    assert!(
        slice_resources
            .iter()
            .all(|value| value.get("inquiry_briefs").is_none())
    );
    assert_eq!(selected_for(&slice_topic, &no_brief_unit).len(), 1);

    let unknown = begin(
        &mut owner,
        &scope,
        &slices[3],
        inquiry(
            "program",
            json!({
        "environment_iris":["urn:fixture:prod"],"action_classes":["deploy"]}),
        ),
    )
    .await;
    assert_eq!(
        unknown["knowledge_resource_status"]["state"],
        "needs_context"
    );
    assert!(
        unknown["knowledge_resources"]["unresolved_needs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "required_selector_context_missing")
    );
    assert!(selected_for(&unknown, &main_unit).is_empty());

    let empty = begin(
        &mut owner,
        &scope,
        &slices[4],
        inquiry(
            "program",
            json!({
        "target_iris":[],"environment_iris":["urn:fixture:prod"],"action_classes":["deploy"]}),
        ),
    )
    .await;
    assert_eq!(empty["knowledge_resource_status"]["state"], "current");
    assert!(selected_for(&empty, &main_unit).is_empty());
    assert!(
        empty["knowledge_resources"]["unresolved_needs"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let mismatch = begin(
        &mut owner,
        &scope,
        &slices[5],
        inquiry(
            "program",
            json!({
        "target_iris":["urn:fixture:r2"],"action_classes":["deploy"]}),
        ),
    )
    .await;
    assert_eq!(mismatch["knowledge_resource_status"]["state"], "current");
    assert!(selected_for(&mismatch, &main_unit).is_empty());
    assert!(
        mismatch["knowledge_resources"]["unresolved_needs"]
            .as_array()
            .unwrap()
            .is_empty()
    );

    let added = commit_create(
        &mut owner,
        document(
            "generation-change",
            json!([binding(json!({"kind":"workspace"}), "required")]),
            "workspace_members",
            true,
        ),
    )
    .await;
    let added_unit = added.receipt["applied_operations"][0]["unit_id"].clone();
    let stale = route(
        &mut owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":program["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    let action = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    route(
        &mut owner,
        "command",
        "pipeline.knowledge_refresh",
        action_params(action).clone(),
    )
    .await;
    let refreshed = route(
        &mut owner,
        "query",
        "slice.pipeline.context",
        json!({"run_id":program["run"]["id"]}),
    )
    .await;
    assert_eq!(refreshed["knowledge_resource_status"]["state"], "current");
    assert_eq!(selected_for(&refreshed, &added_unit).len(), 1);

    let (mut member, member_principal, workspace) =
        member_client(&pool, enrollment.tenant_id, &workspace_key, &root, &socket).await;
    let allowed = route(
        &mut member,
        "query",
        "slice.pipeline.context",
        json!({"run_id":program["run"]["id"]}),
    )
    .await;
    assert_eq!(selected_for(&allowed, &private_unit).len(), 1);
    sqlx::query(
        "DELETE FROM memberships WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(member_principal)
    .execute(&pool)
    .await
    .unwrap();
    let denied = member
        .call_error(
            "query",
            json!({"route":"slice.pipeline.context",
        "params":{"run_id":program["run"]["id"]}}),
        )
        .await;
    assert_eq!(denied["error"]["code"], "forbidden");
    assert!(!denied.to_string().contains("private"));
    member.finish().await;

    owner.finish().await;
    pool.close().await;
}
