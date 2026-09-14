use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn dk4_pinned_revision_status_survives_maintenance_resolution() {
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
    let socket = root.join("dk4-pinned-maintenance.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk4-pinned-maintenance-{}", Uuid::new_v4()),
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
        &format!("dk4-pinned-maintenance-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let pinned_required = ready_program(&mut client, "Pinned Required Program").await;
    let pinned_reference = ready_program(&mut client, "Pinned Reference Program").await;
    let current_required = ready_program(&mut client, "Current Required Program").await;

    let target = "urn:tect:dk4:pinned-maintenance";
    let mut revision_one = planning_document(target, &["required"]);
    revision_one["bindings"] = json!([
        {"target":{"kind":"program","program_id":pinned_required["id"]},
         "purpose":"required","version_resolution":{"kind":"pinned_revision","revision":1}},
        {"target":{"kind":"program","program_id":pinned_reference["id"]},
         "purpose":"reference","version_resolution":{"kind":"pinned_revision","revision":1}},
        {"target":{"kind":"program","program_id":current_required["id"]},
         "purpose":"required","version_resolution":{"kind":"current_accepted"}}
    ]);
    let committed = commit_create(&mut client, revision_one.clone()).await;
    let unit = committed.receipt["applied_operations"][0]["unit_id"].clone();
    let unit_id = Uuid::parse_str(unit.as_str().unwrap()).unwrap();
    let pinned_required = refresh_program(&mut client, &pinned_required, target).await;
    let pinned_reference = refresh_program(&mut client, &pinned_reference, target).await;
    let current_required = refresh_program(&mut client, &current_required, target).await;
    let initial_consumers: Vec<bool> = sqlx::query_scalar(
        "SELECT required FROM knowledge_maintenance_consumers \
         WHERE unit_id=$1 AND unit_revision=1 AND relation_name='planning_knowledge_manifests' \
           AND active ORDER BY required",
    )
    .bind(unit_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(initial_consumers, vec![false, true, true]);

    let (task_id, basis_digest) = mark_needs_review(&pool, &unit).await;
    let basis_digest = json!(basis_digest);
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state='exhausted',attempts=5, \
         failure_code='storage_unavailable' WHERE id=$1",
    )
    .bind(task_id)
    .execute(&pool)
    .await
    .unwrap();
    let pinned_required = refresh_program(&mut client, &pinned_required, target).await;
    let pinned_reference = refresh_program(&mut client, &pinned_reference, target).await;
    let current_required = refresh_program(&mut client, &current_required, target).await;
    assert!(
        pinned_required["planning_knowledge"]["stale_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "knowledge_needs_review")
    );
    assert_eq!(
        pinned_reference["planning_knowledge"]["warnings"],
        json!(["reference_knowledge_needs_review"])
    );

    let maintenance = route(
        &mut client,
        "query",
        "knowledge.maintenance",
        json!({"unit_id":unit,"states":["exhausted"],"limit":10}),
    )
    .await;
    let mut begin = maintenance["actions"][0]["arguments"]["params"].clone();
    let mut revision_two = revision_one.clone();
    revision_two["title"] = json!("Pinned maintenance revision two");
    revision_two["canonical_text"] = json!(
        "The reviewed revision updates current consumers while preserving pinned revision status."
    );
    revision_two["sources"][0]["snapshot"]["uri"] =
        json!("urn:tect:dk4:pinned-maintenance:revision-two");
    revision_two["sources"][0]["snapshot"]["text"] =
        json!("Reviewed revision two for current consumers.");
    begin["change"]["intent"] =
        json!("Resolve the exact maintenance task through reviewed revision two.");
    begin["change"]["desired_outcome"] = json!(
        "Current consumers move to revision two while pinned consumers remain on revision one."
    );
    begin["change"]["sources"] = revision_two["sources"].clone();
    begin["change"]["operation_hints"] = json!([{
        "client_label":"maintenance-revision","operation":"revise","unit_id":unit,
        "expected_revision":1,"expected_lifecycle":"active",
        "reason":"Resolve the exact reviewed maintenance basis.",
        "authority_basis":"Current authenticated workspace owner."
    }]);
    begin["change"]["completion"] = json!({"canonical_result":true,"exact_delivery":true,
        "impact_recorded":true,"search":"not_required","erasure":"not_required"});
    begin["change"]["delivery_mode"] = json!("phasewise");
    let begun = route(&mut client, "command", "knowledge.maintenance_begin", begin).await;
    let change_id = begun["created"]["change"]["created"]["change_id"].clone();
    let change = route(
        &mut client,
        "query",
        "knowledge.lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )
    .await;
    let origin = &context(&change)["origin"];
    let current = complete_agent(
        &mut client,
        &change,
        json!({"phase":"kc-intake","data":{"bounded_outcome":origin["desired_outcome"],
            "operation_hints":origin["operation_hints"],
            "authority_boundary":"Current authenticated workspace owner.",
            "completion":origin["completion"]}}),
    )
    .await;
    let spec = SingleOperation {
        operation: "revise",
        unit_id: Some(json!(unit_id)),
        expected_revision: Some(1),
        expected_lifecycle: Some("active"),
        document: Some(revision_two.clone()),
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        sources: revision_two["sources"].clone(),
        knowledge_kind: json!("constraint"),
        profiles: json!(["general"]),
        erasure: "not_required",
        authored_followup: false,
    };
    let publication = ready_single_from_baseline_with_reviewed(
        &mut client,
        spec,
        current,
        std::slice::from_ref(&basis_digest),
    )
    .await;
    let committed = route(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await;
    assert_eq!(committed["applied"]["applied_operations"][0]["revision"], 2);

    let pinned_required = refresh_program(&mut client, &pinned_required, target).await;
    let pinned_reference = refresh_program(&mut client, &pinned_reference, target).await;
    let current_required = refresh_program(&mut client, &current_required, target).await;
    assert_eq!(
        pinned_required["planning_knowledge"]["manifest"]["selected"][0]["unit_revision"],
        1
    );
    assert!(
        pinned_required["planning_knowledge"]["stale_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|reason| reason == "knowledge_needs_review")
    );
    assert_eq!(
        pinned_reference["planning_knowledge"]["manifest"]["selected"][0]["unit_revision"],
        1
    );
    assert_eq!(
        pinned_reference["planning_knowledge"]["warnings"],
        json!(["reference_knowledge_needs_review"])
    );
    assert_eq!(
        current_required["planning_knowledge"]["manifest"]["selected"][0]["unit_revision"],
        2
    );
    assert!(
        current_required["planning_knowledge"]["stale_reasons"].is_null()
            || current_required["planning_knowledge"]["stale_reasons"]
                .as_array()
                .is_some_and(Vec::is_empty)
    );
    assert!(
        current_required["planning_knowledge"]["warnings"].is_null()
            || current_required["planning_knowledge"]["warnings"]
                .as_array()
                .is_some_and(Vec::is_empty)
    );
    let current_consumers: Vec<(i64, bool)> = sqlx::query_as(
        "SELECT unit_revision,required FROM knowledge_maintenance_consumers \
         WHERE unit_id=$1 AND relation_name='planning_knowledge_manifests' AND active \
         ORDER BY unit_revision,required",
    )
    .bind(unit_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(current_consumers, vec![(1, false), (1, true), (2, true)]);
    client.finish().await;
}
