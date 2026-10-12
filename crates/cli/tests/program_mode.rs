//! The product level of a Program through a real daemon and MCP bridge against
//! PostgreSQL: where it is required, how a save patches it and how a read shows it.
#[allow(dead_code)]
mod recovery_support;

use recovery_support::native_reads::program_queries::read_program_query;
use recovery_support::{Daemon, Mcp, host_file, private_temp, public_call, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use tect_postgres::admin;
use uuid::Uuid;

/// The Program as `program.get` shows it now.
async fn read(client: &mut Mcp, id: &Value) -> Value {
    let page = read_program_query(
        async |arguments| {
            client
                .exchange("tools/call", json!({"name":"query","arguments":arguments}))
                .await
        },
        json!({"program_id":id}),
    )
    .await
    .unwrap();
    page.value["program"].clone()
}

async fn stored_mode(pool: &PgPool, id: &Value) -> Option<String> {
    let id = Uuid::parse_str(id.as_str().unwrap()).unwrap();
    sqlx::query_scalar("SELECT mode FROM programs WHERE id=$1")
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap()
}

/// A refused save: its error code and the sentence the agent reads above it.
async fn refused(client: &mut Mcp, params: Value) -> (Value, String) {
    let response = client
        .exchange("tools/call", public_call("save_program", params))
        .await;
    assert_eq!(response["result"]["isError"], true, "{response}");
    let intro = response["result"]["content"][0]["text"].as_str().unwrap();
    (
        recovery_support::tool_payload(&response)["error"]["code"].clone(),
        intro.to_owned(),
    )
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn mode_is_optional_in_a_draft_required_to_open_and_kept_until_replaced() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("mode.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-program-mode-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-mode-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &workspace).await;
    client.call("open_workspace", json!({})).await;

    // A Draft has no mode and reads without the key, as it does without this field.
    let begun = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Show invitations by email working."}),
        )
        .await;
    let id = begun["program"]["id"].clone();
    let draft = read(&mut client, &id).await;
    assert!(draft.get("mode").is_none(), "{draft}");
    assert_eq!(stored_mode(&pool, &id).await, None);
    assert!(
        begun["omitted_fields"]
            .as_array()
            .unwrap()
            .contains(&json!("program.mode"))
    );

    // A partial Draft saves without a mode.
    let fields = json!({"program_id":id,"revision":1,"input_cursor":1,
        "name":"Invitations","intent":"Let a user invite another by email",
        "basis":"The original request","boundaries":"Email invitations only",
        "constraints":"No explicit constraints were supplied.",
        "success":"An invited user receives the email and joins"});
    let partial = client.call("save_program", fields).await;
    assert_eq!(partial["program"]["status"], "draft");
    assert!(read(&mut client, &id).await.get("mode").is_none());

    // Opening without one is refused like a missing text field, and the refusal names it.
    let complete = json!({"program_id":id,"revision":2,"input_cursor":1,"complete":true});
    let (code, intro) = refused(&mut client, complete.clone()).await;
    assert_eq!(code, "program_incomplete");
    assert!(
        intro.contains("state mode as demo, mvp, prod or serious_prod"),
        "{intro}"
    );
    // Null does not clear or stand in for a mode, and the scale is closed.
    for invalid in [Value::Null, json!("production"), json!("")] {
        let mut stated = complete.clone();
        stated["mode"] = invalid;
        assert_eq!(refused(&mut client, stated).await.0, "invalid_arguments");
    }
    assert_eq!(stored_mode(&pool, &id).await, None);

    // A value sets it, in a Draft save as well; the read shows it.
    let staged = client
        .call(
            "save_program",
            json!({"program_id":id,"revision":2,"input_cursor":1,"mode":"demo"}),
        )
        .await;
    assert_eq!(staged["program"]["status"], "draft");
    assert!(staged["program"].get("mode").is_none());
    assert_eq!(read(&mut client, &id).await["mode"], "demo");

    let mut opening = complete;
    opening["revision"] = json!(3);
    opening["mode"] = json!("prod");
    let opened = client.call("save_program", opening).await;
    assert_eq!(opened["program"]["status"], "open");
    assert_eq!(read(&mut client, &id).await["mode"], "prod");
    assert_eq!(stored_mode(&pool, &id).await.as_deref(), Some("prod"));

    // Omission keeps it on an Open Program; a value replaces it.
    client
        .call(
            "save_program",
            json!({"program_id":id,"revision":4,"input_cursor":1,"working_notes":"Continue."}),
        )
        .await;
    assert_eq!(read(&mut client, &id).await["mode"], "prod");
    client
        .call(
            "save_program",
            json!({"program_id":id,"revision":5,"input_cursor":1,"mode":"serious_prod"}),
        )
        .await;
    let raised = read(&mut client, &id).await;
    assert_eq!(raised["mode"], "serious_prod");
    assert_eq!(raised["status"], "open");

    // A Program opened before the field existed has none. It reads without the key,
    // and its next save has to state one.
    sqlx::query("UPDATE programs SET mode=NULL WHERE id=$1")
        .bind(Uuid::parse_str(id.as_str().unwrap()).unwrap())
        .execute(&pool)
        .await
        .unwrap();
    let legacy = read(&mut client, &id).await;
    assert_eq!(legacy["status"], "open");
    assert!(legacy.get("mode").is_none(), "{legacy}");
    let notes = json!({"program_id":id,"revision":6,"input_cursor":1,"working_notes":"Later."});
    let (code, intro) = refused(&mut client, notes.clone()).await;
    assert_eq!(code, "program_incomplete");
    assert!(intro.contains("mode"), "{intro}");
    let mut stated = notes;
    stated["mode"] = json!("mvp");
    client.call("save_program", stated).await;
    assert_eq!(read(&mut client, &id).await["mode"], "mvp");

    // The column holds one of the four values or nothing.
    let outside = sqlx::query("UPDATE programs SET mode='production' WHERE id=$1")
        .bind(Uuid::parse_str(id.as_str().unwrap()).unwrap())
        .execute(&pool)
        .await;
    assert!(outside.is_err());
    assert_eq!(stored_mode(&pool, &id).await.as_deref(), Some("mvp"));
    client.finish().await;
}
