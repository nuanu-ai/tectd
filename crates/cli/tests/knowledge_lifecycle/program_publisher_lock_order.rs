use super::*;
use std::time::Duration;

async fn ordered_call(
    pool: &PgPool,
    tag: &str,
    mut client: Mcp,
    route_name: &'static str,
    params: Value,
) -> (Mcp, Value) {
    let mut publisher = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))")
        .execute(&mut *publisher)
        .await
        .unwrap();
    let operation = tokio::spawn(async move {
        let result = route(&mut client, "command", route_name, params).await;
        (client, result)
    });
    let waiter = tokio::time::timeout(Duration::from_secs(8), async {
        loop {
            let pid: Option<i32> = sqlx::query_scalar(
                "SELECT pid FROM pg_stat_activity WHERE application_name=$1 \
                 AND wait_event_type='Lock' AND query LIKE '%pg_advisory_xact_lock%' LIMIT 1",
            )
            .bind(tag)
            .fetch_optional(pool)
            .await
            .unwrap();
            if let Some(pid) = pid {
                break pid;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap_or_else(|_| panic!("{route_name} must wait on the publisher gate"));
    let premature_locks: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_locks WHERE pid=$1 AND granted \
         AND relation IN ('pgrdf._pgrdf_quads'::regclass,'programs'::regclass)",
    )
    .bind(waiter)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        premature_locks, 0,
        "{route_name} took native/program locks before the publisher gate"
    );
    let graph_iri = format!("urn:tect:dk2:lock-order:publisher:{}", Uuid::new_v4());
    let graph_id: i64 = sqlx::query_scalar("SELECT pgrdf.add_graph($1)")
        .bind(&graph_iri)
        .fetch_one(&mut *publisher)
        .await
        .unwrap();
    publisher.commit().await.unwrap();
    let (client, result) = tokio::time::timeout(Duration::from_secs(12), operation)
        .await
        .unwrap_or_else(|_| panic!("{route_name} must finish after publisher commits"))
        .unwrap();
    let persisted_graph: Option<i64> = sqlx::query_scalar("SELECT pgrdf.graph_id($1)")
        .bind(&graph_iri)
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(persisted_graph, Some(graph_id));
    (client, result)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn program_writes_wait_for_publisher_before_native_or_program_locks() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1")
        || std::env::var("TECT_TEST_LOCK_ORDER").as_deref() != Ok("1")
    {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let database: String = sqlx::query_scalar("SELECT current_database()")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert!(database.starts_with("tect_dk2_lock_order_"), "{database}");
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("program-lock-order.sock");
    let tag = format!("tect-program-lock-order-{}", Uuid::new_v4());
    let _daemon = Daemon::start(&tagged_url(&runtime_url, &tag), socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let native_session = Uuid::new_v4().to_string();
    let workspace_key = format!("program-lock-order-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &native_session, &workspace_key).await;
    client.call("open_workspace", json!({})).await;
    let target = format!("urn:tect:dk2:lock-order:{}", Uuid::new_v4());
    commit_create(&mut client, planning_document(&target, &["required"])).await;
    let (next_client, begun) = ordered_call(
        &pool,
        &tag,
        client,
        "program.begin",
        json!({"request_id":Uuid::new_v4(),"input":"Exercise native planning read order.",
            "task_context":{"target_iris":[target]}}),
    )
    .await;
    client = next_client;
    let program = &begun["program"];
    let program_id = Uuid::parse_str(program["id"].as_str().unwrap()).unwrap();
    assert_eq!(
        program["planning_knowledge"]["manifest"]["selected"]
            .as_array()
            .unwrap()
            .len(),
        1,
        "the save must exercise a native planning read"
    );
    let guard = planning_guard(&program["planning_knowledge"]["manifest"]);
    let (next_client, saved) = ordered_call(
        &pool,
        &tag,
        client,
        "program.save",
        json!({"program_id":program_id,"revision":1,"input_cursor":1,
            "name":"Ordered save","intent":"Keep native knowledge consistent",
            "basis":"Reviewed fixture","boundaries":"One test workspace",
            "constraints":"Publisher order","success":"Save commits","complete":true,
            "consumed_knowledge":guard}),
    )
    .await;
    client = next_client;
    assert_eq!(saved["program"]["id"], json!(program_id));
    assert_eq!(saved["program"]["revision"], 2);
    let persisted_revision: i64 = sqlx::query_scalar("SELECT revision FROM programs WHERE id=$1")
        .bind(program_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(persisted_revision, 2);
    let consumptions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM planning_knowledge_consumptions \
         WHERE relation_name='programs' AND row_id=$1 AND row_revision=2 AND NOT redacted",
    )
    .bind(program_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(consumptions, 1);
    let input_id = Uuid::new_v4();
    let (next_client, recorded) = ordered_call(
        &pool,
        &tag,
        client,
        "program.record_input",
        json!({"program_id":program_id,"request_id":input_id,
            "input":"Record one follow-up under the publisher gate.",
            "task_context":{"target_iris":[target]}}),
    )
    .await;
    client = next_client;
    assert_eq!(recorded["program"]["revision"], 3);
    assert_eq!(recorded["program"]["latest_input"], 2);
    let input_rows: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM program_inputs WHERE program_id=$1 AND request_id=$2",
    )
    .bind(program_id)
    .bind(input_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(input_rows, 1);

    let refresh_id = Uuid::new_v4();
    let refresh = json!({"program_id":program_id,"revision":3,"input_cursor":1,
        "request_id":refresh_id,"task_context":{"target_iris":[target]}});
    let (next_client, refreshed) = ordered_call(
        &pool,
        &tag,
        client,
        "program.knowledge.refresh",
        refresh.clone(),
    )
    .await;
    client = next_client;
    assert_eq!(refreshed["program"]["revision"], 4);
    let (client, replay) =
        ordered_call(&pool, &tag, client, "program.knowledge.refresh", refresh).await;
    assert_eq!(replay["program"]["revision"], 4);
    let persisted_revision: i64 = sqlx::query_scalar("SELECT revision FROM programs WHERE id=$1")
        .bind(program_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(persisted_revision, 4);
    client.finish().await;
}
