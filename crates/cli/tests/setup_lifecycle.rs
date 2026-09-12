//! Real PostgreSQL, daemon, and stdio-MCP workspace setup lifecycle acceptance.

mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::Path;
use std::process::Command;
use tect_postgres::admin;
use uuid::Uuid;

fn setup_id(payload: &Value) -> Uuid {
    payload["setup"]["id"].as_str().unwrap().parse().unwrap()
}

fn repository(path: &Path) {
    std::fs::create_dir(path).unwrap();
    let output = Command::new("git")
        .arg("-C")
        .arg(path)
        .args(["init", "--quiet", "--initial-branch=main"])
        .output()
        .unwrap();
    assert!(output.status.success(), "git fixture creation failed");
}

async fn canonical(pool: &PgPool, id: Uuid) -> Vec<String> {
    let mut rows = vec![
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(s)::text \
             FROM workspace_setups s WHERE id=$1",
        )
        .bind(id)
        .fetch_one(pool)
        .await
        .unwrap(),
    ];
    rows.extend(
        sqlx::query_scalar::<_, String>(
            "SELECT xmin::text || ':' || row_to_json(i)::text \
             FROM workspace_setup_inputs i WHERE setup_id=$1 ORDER BY sequence",
        )
        .bind(id)
        .fetch_all(pool)
        .await
        .unwrap(),
    );
    rows
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn setup_preserves_narrative_draft_history_and_source_independence() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let directory = root.join("actual-task");
    std::fs::create_dir(&directory).unwrap();
    let repositories = [
        root.join("source-a"),
        root.join("source-b"),
        root.join("source-c"),
    ];
    for repository_path in &repositories {
        repository(repository_path);
    }

    let socket = root.join("setup.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-setup-life-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let host = admin::enroll_host_with_grants(
        &pool,
        None,
        vec![root.to_str().unwrap().to_owned()],
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join("host.json");
    host_file(&config, &host.auth);
    let workspace = format!("setup-lifecycle-{}", Uuid::new_v4().simple());
    let first_native = Uuid::new_v4().to_string();
    let mut first = Mcp::start(&socket, &config, &first_native, &workspace).await;

    let opened = first.call("open_workspace", json!({})).await;
    assert!(opened["selected_worktrees"].as_array().unwrap().is_empty());
    assert_eq!(opened["file"]["status"], "context_unknown");
    assert_eq!(opened["file"]["observed_now"], false);
    let inspected = first
        .call(
            "inspect_setup",
            json!({"task_directory":directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(inspected["file"]["status"], "missing");
    assert_eq!(inspected["file"]["observed_now"], true);

    let request_id = Uuid::new_v4();
    let narrative = "Nuanu builds native developer infrastructure.\nPreserve `facts`, \\\\slashes, emoji 🧭, and trailing spaces.  ";
    let begun = first
        .call(
            "begin_setup",
            json!({"request_id":request_id,"input":narrative}),
        )
        .await;
    let id = setup_id(&begun);
    assert_eq!(begun["setup"]["status"], "draft");
    assert_eq!(begun["setup"]["revision"], 1);
    assert_eq!(begun["setup"]["current_step"], "compose");
    assert_eq!(begun["setup"]["latest_input"], 1);
    assert!(begun["setup"]["content"].is_null());

    let page = first
        .call(
            "get_setup",
            json!({"setup_id":id,"after_input":0,"limit":1}),
        )
        .await;
    assert_eq!(page["inputs"][0]["request_id"], request_id.to_string());
    assert_eq!(page["inputs"][0]["input"], narrative);
    assert_eq!(page["inputs"][0]["sequence"], 1);
    assert_eq!(page["next_after_input"], Value::Null);

    let before_missing_ready = canonical(&pool, id).await;
    let missing_ready = first
        .call_error(
            "save_setup",
            json!({"setup_id":id,"revision":1,"input_cursor":1,"content":"draft"}),
        )
        .await;
    assert_eq!(missing_ready["error"]["code"], "invalid_arguments");
    assert_eq!(canonical(&pool, id).await, before_missing_ready);

    let question = "Which deployment boundary must the workspace instructions preserve?";
    let partial_content = "# Nuanu workspace\n\nDraft pending one deployment answer.\n";
    let paused = first
        .call(
            "save_setup",
            json!({
                "setup_id":id,"revision":1,"input_cursor":1,"ready":false,
                "content":partial_content,
                "working_notes":"Keep the exact narrative and answer separate.",
                "pending_question":question
            }),
        )
        .await;
    assert_eq!(paused["setup"]["revision"], 2);
    assert_eq!(paused["setup"]["current_step"], "waiting_input");
    assert_eq!(paused["setup"]["pending_question"], question);

    let answer_id = Uuid::new_v4();
    let answer = "Staging may auto-deploy; production remains separately authorized.\nKeep this exact reply.\t";
    let resumed = first
        .call(
            "record_setup_input",
            json!({"setup_id":id,"revision":2,"request_id":answer_id,"input":answer}),
        )
        .await;
    assert_eq!(resumed["setup"]["revision"], 3);
    assert_eq!(resumed["setup"]["latest_input"], 2);
    assert_eq!(resumed["setup"]["pending_question"], question);

    let final_content = "# Nuanu workspace\n\n- Preserve exact task identity.\n- Staging may auto-deploy.\n- Production requires separate authority.\n";
    let cleared = first
        .call(
            "save_setup",
            json!({
                "setup_id":id,"revision":3,"input_cursor":2,"ready":false,
                "content":final_content,"working_notes":null,"pending_question":null
            }),
        )
        .await;
    assert_eq!(cleared["setup"]["revision"], 4);
    assert!(cleared["setup"]["working_notes"].is_null());
    assert!(cleared["setup"]["pending_question"].is_null());
    assert_eq!(cleared["setup"]["current_step"], "compose");
    let ready = first
        .call(
            "save_setup",
            json!({"setup_id":id,"revision":4,"input_cursor":2,"ready":true}),
        )
        .await;
    assert_eq!(ready["setup"]["revision"], 5);
    assert_eq!(ready["setup"]["current_step"], "ready_to_apply");
    assert_eq!(ready["setup"]["content"], final_content);

    let cleared_selection = first
        .call("select_worktrees", json!({"worktree_ids":[]}))
        .await;
    assert!(
        cleared_selection["selected_worktrees"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let mut source_ids = Vec::new();
    for repository_path in &repositories {
        let source = first
            .call("register_source", json!({"path":repository_path}))
            .await;
        source_ids.push(source["id"].clone());
    }
    let one = first
        .call(
            "select_worktrees",
            json!({"worktree_ids":[source_ids[0].clone()]}),
        )
        .await;
    assert_eq!(one["selected_worktrees"].as_array().unwrap().len(), 1);
    let many = first
        .call("select_worktrees", json!({"worktree_ids":source_ids}))
        .await;
    assert_eq!(many["selected_worktrees"].as_array().unwrap().len(), 3);
    let still_ready = first
        .call(
            "get_setup",
            json!({"setup_id":id,"after_input":0,"limit":25}),
        )
        .await;
    assert_eq!(still_ready["setup"]["revision"], 5);
    assert_eq!(still_ready["setup"]["content"], final_content);
    first.finish().await;

    let later_native = Uuid::new_v4().to_string();
    let mut later = Mcp::start(&socket, &config, &later_native, &workspace).await;
    let later_open = later.call("open_workspace", json!({})).await;
    assert!(
        later_open["selected_worktrees"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let recovered = later
        .call(
            "inspect_setup",
            json!({"task_directory":directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(recovered["setup_context"]["setup"]["id"], id.to_string());
    let whole = later
        .call(
            "get_setup",
            json!({"setup_id":id,"after_input":0,"limit":25}),
        )
        .await;
    assert_eq!(setup_id(&whole), id);
    assert_eq!(whole["setup"]["content"], final_content);
    assert!(whole["setup"]["working_notes"].is_null());
    assert!(whole["setup"]["pending_question"].is_null());
    assert_eq!(whole["setup"]["input_cursor"], 2);
    assert_eq!(whole["setup"]["latest_input"], 2);
    let exact: Vec<_> = whole["inputs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|entry| entry["input"].as_str().unwrap())
        .collect();
    assert_eq!(exact, [narrative, answer]);
    let consumed = later.call("get_setup", json!({"setup_id":id})).await;
    assert!(consumed["inputs"].as_array().unwrap().is_empty());
    assert_eq!(consumed["next_after_input"], Value::Null);

    let applied = later
        .call("apply_setup", json!({"setup_id":id,"revision":5}))
        .await;
    assert_eq!(applied["setup"]["status"], "applied");
    assert_eq!(applied["file"]["publication"]["outcome"], "created");
    assert_eq!(
        std::fs::read_to_string(directory.join("AGENTS.md")).unwrap(),
        final_content
    );
    later.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
