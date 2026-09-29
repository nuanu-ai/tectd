use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn erasing_shared_direct_assertion_preserves_its_other_owner() {
    if std::env::var("TECT_TEST_DK3_LIFECYCLE").as_deref() != Ok("1") {
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
    support::repository(&repo);
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("dk3-shared-assertion-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native.clone(),
        workspace_key: workspace_key.clone(),
    };
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    service.open_workspace(&context).await.unwrap();
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(enrollment.tenant_id)
            .bind(&workspace_key)
            .fetch_one(&pool)
            .await
            .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let socket = root.join("shared-assertion.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk3-shared-assertion-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;

    let subject = format!("urn:tect:dk3:shared-assertion:{}:subject", Uuid::new_v4());
    let object = format!("urn:tect:dk3:shared-assertion:{}:object", Uuid::new_v4());
    let predicate = "urn:tect:dk:v2:broaderConcept";
    let assertion =
        json!({"subject_iri":subject,"predicate":"broader_concept","object_iri":object});
    let mut first_document = document("shared-assertion-first");
    first_document["graph_assertions"] = json!([assertion.clone()]);
    let mut second_document = document("shared-assertion-second");
    second_document["graph_assertions"] = json!([assertion.clone()]);
    let first = commit_create(&mut client, first_document.clone()).await;
    let second = commit_create(&mut client, second_document.clone()).await;
    let first_id = Uuid::parse_str(
        first.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let second_id = Uuid::parse_str(
        second.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let graph = format!(
        "urn:tect:dk:workspace:{}:{}",
        enrollment.tenant_id, workspace
    );
    let graph_query = format!(
        "CONSTRUCT {{ ?s <{predicate}> ?o }} WHERE {{ GRAPH <{graph}> {{ ?s <{predicate}> ?o }} }}"
    );
    let expected_row = json!({
        "subject":{"type":"iri","value":subject},
        "predicate":{"type":"iri","value":predicate},
        "object":{"type":"iri","value":object}
    });

    for (id, document) in [(first_id, &first_document), (second_id, &second_document)] {
        let exact = support::route(
            &mut client,
            "query",
            "knowledge.unit",
            json!({"unit_id":id,"revision":1}),
        )
        .await;
        assert_eq!(&exact["document"]["document"], document);
        let event: Uuid = sqlx::query_scalar(
            "SELECT publication_event_id FROM knowledge_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND revision=1",
        )
        .bind(enrollment.tenant_id)
        .bind(workspace)
        .bind(id)
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(enrollment.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let rows: Vec<Value> =
            sqlx::query_scalar("SELECT * FROM public.tect_dk2_native_read($1,$2,$3,1,$4,true)")
                .bind(enrollment.tenant_id)
                .bind(workspace)
                .bind(id)
                .bind(event)
                .fetch_all(&mut *tx)
                .await
                .unwrap();
        assert!(
            rows.contains(&expected_row),
            "unit {id} native read omitted shared assertion"
        );
        tx.rollback().await.unwrap();
    }
    let graph_rows: Vec<Value> =
        sqlx::query_scalar("SELECT row FROM pgrdf.construct($1) AS native(row)")
            .bind(&graph_query)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        graph_rows
            .iter()
            .filter(|row| **row == expected_row)
            .count(),
        1
    );

    let erased_first = commit_single(&mut client, remove(json!(first_id), 1, "erase")).await;
    assert_eq!(
        erased_first["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    let survivor = support::route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":second_id,"revision":1}),
    )
    .await;
    assert_eq!(survivor["document"]["document"], second_document);
    let graph_rows: Vec<Value> =
        sqlx::query_scalar("SELECT row FROM pgrdf.construct($1) AS native(row)")
            .bind(&graph_query)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        graph_rows
            .iter()
            .filter(|row| **row == expected_row)
            .count(),
        1
    );

    let erased_second = commit_single(&mut client, remove(json!(second_id), 1, "erase")).await;
    assert_eq!(
        erased_second["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    let graph_rows: Vec<Value> =
        sqlx::query_scalar("SELECT row FROM pgrdf.construct($1) AS native(row)")
            .bind(&graph_query)
            .fetch_all(&pool)
            .await
            .unwrap();
    assert!(!graph_rows.contains(&expected_row));
    for id in [first_id, second_id] {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(enrollment.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let residual: Value =
            sqlx::query_scalar("SELECT public.tect_dk_native_owned_residual($1,$2,$3)")
                .bind(enrollment.tenant_id)
                .bind(workspace)
                .bind(id)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(
            residual["owned_triples"], 0,
            "unit {id} retained native triples"
        );
        tx.rollback().await.unwrap();
    }

    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    pool.close().await;
}
