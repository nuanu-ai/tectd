//! Public MCP stdio bridge smoke only; no Matrix recommendation is prepared.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{os::unix::fs::PermissionsExt, sync::Arc};
use support::{route, route_error};
use tect_application::WorkspaceService;
use tect_postgres::{PgStore, admin};
use tokio::net::UnixListener;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires identity-pinned disposable PG18 and TECT_TEST_* URLs"]
async fn public_model_route_catalogue_and_strict_bridge_smoke() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let identity: (String, i64, String, i64, bool) = sqlx::query_as(
        "SELECT current_database(),(SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_catalog.pg_control_system()),\
         (SELECT max(version) FROM _sqlx_migrations),(SELECT bool_and(success) FROM _sqlx_migrations)",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(
        (
            identity.0.as_str(),
            identity.1,
            identity.2.as_str(),
            identity.3,
            identity.4
        ),
        ("tect_test", 16385, "7689349823162929726", 82, true)
    );
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("route.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-route-smoke-{}", Uuid::new_v4()),
    );
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )); // Model-route provider is intentionally Disabled.
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let enrolled = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let config = root.join("owner.json");
    host_file(&config, &enrolled.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("model-route-smoke-{}", Uuid::new_v4()),
    )
    .await;
    let listed = client.exchange("tools/list", json!({})).await;
    let tools = listed["result"]["tools"].as_array().unwrap();
    assert_eq!(tools.len(), 5);
    let routes = |tool: &str| -> Vec<&str> {
        tools
            .iter()
            .find(|entry| entry["name"] == tool)
            .unwrap()["inputSchema"]["properties"]["route"]["enum"]
            .as_array()
            .unwrap()
            .iter()
            .map(|item| item.as_str().unwrap())
            .collect()
    };
    let commands = routes("command");
    let queries = routes("query");
    for name in [
        "model.route.prepare",
        "model.route.run",
        "model.route.disposition",
    ] {
        assert!(commands.contains(&name), "{name} not advertised");
    }
    assert!(queries.contains(&"model.route.get"));

    let opened = route(&mut client, "command", "workspace.open", json!({})).await;
    assert!(opened["workspace"]["id"].is_string());
    let workspace_id = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let key = format!("absent-{}", Uuid::new_v4());
    let absent = route_error(
        &mut client,
        "query",
        "model.route.get",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(absent["error"]["code"], "not_found");

    for malformed in [
        json!({"preparation_request_key":key,"ranked_route_ids":["route-a"]}),
        json!({"preparation_request_key":key,"actual_route_id":"route-a"}),
        json!({"preparation_request_key":Value::Null}),
    ] {
        let denied = route_error(&mut client, "command", "model.route.run", malformed).await;
        assert_eq!(denied["error"]["code"], "invalid_arguments");
    }
    let rows: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM model_route_preparations WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM model_route_advisory_attempts WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM model_route_decisions WHERE tenant_id=$1 AND workspace_id=$2)",
    )
    .bind(enrolled.tenant_id)
    .bind(workspace_id)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(rows, (0, 0, 0), "bridge smoke must not create a send");
    client.finish().await;
    server.abort();
    let _ = server.await;
}
