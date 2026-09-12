use crate::legacy::{LegacyDaemon, LegacyMcp, exact_action};
use crate::recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url, tool_payload};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::HashSet;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Path, PathBuf};
use tect_postgres::admin;
use uuid::Uuid;

const BASELINE: &str = "b6d988ef4eec91f9a90ce12ce8ac9fd75decc1a7";
const SEARCH_HIGH: usize = 8_000_000;
const MAX_ATTEMPTS: usize = 24;

async fn seed_worst_selection(
    pool: &PgPool,
    tenant: Uuid,
    host: Uuid,
    workspace: Uuid,
    session: Uuid,
) -> Vec<Uuid> {
    let repository = Uuid::new_v4();
    let worktrees: Vec<Uuid> = (0..100).map(|_| Uuid::new_v4()).collect();
    let mut transaction = pool.begin().await.unwrap();
    sqlx::query(
        "INSERT INTO source_repositories \
         (id,tenant_id,workspace_id,host_id,common_dir) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(repository)
    .bind(tenant)
    .bind(workspace)
    .bind(host)
    .bind(format!("/legacy-capacity/{}.git", Uuid::new_v4().simple()))
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO source_worktrees \
         (id,tenant_id,workspace_id,host_id,repository_id,path) \
         SELECT fixture.id,$1,$2,$3,$4, \
                chr((14 + (fixture.ordinality-1)/18)::integer) || \
                chr((14 + (fixture.ordinality-1)%18)::integer) || repeat(chr(1),4094) \
         FROM unnest($5::uuid[]) WITH ORDINALITY AS fixture(id,ordinality)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(host)
    .bind(repository)
    .bind(&worktrees)
    .execute(&mut *transaction)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO session_worktrees \
         (tenant_id,workspace_id,host_id,session_id,worktree_id) \
         SELECT $1,$2,$3,$4,id FROM unnest($5::uuid[]) AS selected(id)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(host)
    .bind(session)
    .bind(&worktrees)
    .execute(&mut *transaction)
    .await
    .unwrap();
    transaction.commit().await.unwrap();
    let dimensions: (i64, Option<i32>, Option<i32>) = sqlx::query_as(
        "SELECT count(*),min(octet_length(path)),max(octet_length(path)) \
         FROM source_worktrees WHERE tenant_id=$1 AND repository_id=$2",
    )
    .bind(tenant)
    .bind(repository)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(dimensions, (100, Some(4096), Some(4096)));
    worktrees
}

fn program_id(payload: &Value) -> Uuid {
    payload["program"]["id"].as_str().unwrap().parse().unwrap()
}

fn save_arguments(program: Uuid, revision: i64, name: String) -> Value {
    json!({"program_id":program,"revision":revision,"input_cursor":1,"name":name})
}

fn binary(path: &str) -> PathBuf {
    let path = PathBuf::from(std::env::var(path).unwrap());
    assert!(path.is_absolute() && path.is_file());
    path
}

fn write_result(path: &Path, value: &Value) {
    fs::write(path, serde_json::to_vec_pretty(value).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}

pub async fn run() {
    assert_eq!(std::env::var("TECT_LEGACY_COMMIT").unwrap(), BASELINE);
    let legacy_daemon = binary("TECT_LEGACY_DAEMON");
    let legacy_mcp = binary("TECT_LEGACY_MCP");
    let result_path = PathBuf::from(std::env::var("TECT_LEGACY_RESULT").unwrap());
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let setup_directory = root.join("current-setup-context");
    fs::create_dir(&setup_directory).unwrap();
    let enrollment = admin::enroll_host_with_grants(
        &pool,
        None,
        Vec::new(),
        vec![root.to_str().unwrap().to_owned()],
    )
    .await
    .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("legacy-program-{}", Uuid::new_v4().simple());
    let native = Uuid::new_v4().to_string();
    let socket = root.join("legacy.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-legacy-capacity-{}", Uuid::new_v4()),
    );
    let old_daemon = LegacyDaemon::start(&legacy_daemon, &runtime, socket.clone()).await;
    let mut old = LegacyMcp::start(&legacy_mcp, &socket, &config, &native, &workspace_key).await;
    let (opened_ok, opened) = old.call("open_workspace", json!({})).await;
    assert!(opened_ok);
    let workspace: Uuid = opened["workspace"]["id"].as_str().unwrap().parse().unwrap();
    let session: Uuid = opened["session"]["id"].as_str().unwrap().parse().unwrap();
    let worktrees = seed_worst_selection(
        &pool,
        enrollment.tenant_id,
        enrollment.auth.host_id,
        workspace,
        session,
    )
    .await;
    let (state_ok, state) = old.call("get_state", json!({})).await;
    assert!(state_ok);
    assert_eq!(state["selected_worktrees"].as_array().unwrap().len(), 100);
    assert!(
        state["selected_worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["path"].as_str().unwrap().len() == 4096)
    );

    let original = "baseline API accepted this exact original";
    let (begun_ok, begun) = old
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":original}),
        )
        .await;
    assert!(begun_ok);
    let legacy_program = program_id(&begun);
    let mut revision = begun["program"]["revision"].as_i64().unwrap();
    let (mut low, mut high, mut attempts) = (0_usize, SEARCH_HIGH, 0_usize);
    while low + 1 < high {
        assert!(attempts < MAX_ATTEMPTS);
        attempts += 1;
        let middle = low + (high - low) / 2;
        let candidate = "L".repeat(middle);
        let (accepted, payload) = old
            .call(
                "save_program",
                save_arguments(legacy_program, revision, candidate),
            )
            .await;
        if accepted {
            assert_eq!(payload["program"]["name"].as_str().unwrap().len(), middle);
            revision = payload["program"]["revision"].as_i64().unwrap();
            low = middle;
        } else {
            assert_eq!(payload["error"]["code"], "request_too_large");
            high = middle;
        }
    }
    assert!(attempts <= MAX_ATTEMPTS && low > 0 && high == low + 1);
    let legacy_name = "L".repeat(low);
    let (replay_ok, replay) = old
        .call(
            "save_program",
            save_arguments(legacy_program, revision, legacy_name.clone()),
        )
        .await;
    assert!(replay_ok);
    revision = replay["program"]["revision"].as_i64().unwrap();
    assert_eq!(replay["program"]["name"], legacy_name);
    let (upper_ok, upper) = old
        .call(
            "save_program",
            save_arguments(legacy_program, revision, "L".repeat(high)),
        )
        .await;
    assert!(!upper_ok);
    assert_eq!(upper["error"]["code"], "request_too_large");
    let (old_get_ok, old_get) = old
        .call(
            "get_program",
            json!({"program_id":legacy_program,"after_input":0,"limit":25}),
        )
        .await;
    assert!(old_get_ok);
    assert_eq!(old_get["program"]["name"], legacy_name);
    assert_eq!(old_get["inputs"][0]["input"], original);
    old.finish().await;
    old_daemon.stop().await;

    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut current = Mcp::start(&socket, &config, &native, &workspace_key).await;
    let inspected = current
        .call(
            "inspect_setup",
            json!({"task_directory":setup_directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(
        inspected["setup_context"]["task_directory"],
        setup_directory.to_str().unwrap()
    );
    let state_response = current
        .exchange("tools/call", json!({"name":"get_state","arguments":{}}))
        .await;
    let intro = state_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(!intro.contains("No Programs"));
    let current_state = tool_payload(&state_response);
    assert_eq!(current_state["programs_delivery"], "use_list_programs");
    assert!(current_state["programs"].as_array().unwrap().is_empty());
    assert!(current_state["next_after"].is_null());
    let list = exact_action(&current_state, "list_programs").unwrap();
    assert_eq!(list["arguments"], json!({"limit":25}));
    let fallback_page = current
        .call("list_programs", list["arguments"].clone())
        .await;
    assert_eq!(fallback_page["programs"].as_array().unwrap().len(), 1);
    assert_eq!(
        fallback_page["programs"][0]["id"],
        legacy_program.to_string()
    );
    assert_eq!(fallback_page["programs"][0]["name"], legacy_name);
    assert!(fallback_page["next_after"].is_null());
    assert_eq!(
        current_state["actions"].as_array().unwrap().last().unwrap()["tool"],
        "begin_program"
    );

    // A distinct native session rebuilds the same truthful fallback from durable state.
    let mut fresh = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let fresh_open = fresh.call("open_workspace", json!({})).await;
    assert_eq!(fresh_open["workspace"]["id"], workspace.to_string());
    assert!(
        fresh_open["selected_worktrees"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let selected = fresh
        .call("select_worktrees", json!({"worktree_ids":worktrees}))
        .await;
    assert_eq!(
        selected["selected_worktrees"].as_array().unwrap().len(),
        100
    );
    assert!(
        selected["selected_worktrees"]
            .as_array()
            .unwrap()
            .iter()
            .all(|entry| entry["path"].as_str().unwrap().len() == 4096)
    );
    let fresh_inspected = fresh
        .call(
            "inspect_setup",
            json!({"task_directory":setup_directory.to_str().unwrap()}),
        )
        .await;
    assert_eq!(
        fresh_inspected["setup_context"]["task_directory"],
        setup_directory.to_str().unwrap()
    );
    let fresh_response = fresh
        .exchange("tools/call", json!({"name":"get_state","arguments":{}}))
        .await;
    let fresh_intro = fresh_response["result"]["content"][0]["text"]
        .as_str()
        .unwrap();
    assert!(!fresh_intro.contains("No Programs"));
    let fresh_state = tool_payload(&fresh_response);
    assert_eq!(fresh_state["programs_delivery"], "use_list_programs");
    assert!(fresh_state["programs"].as_array().unwrap().is_empty());
    assert!(fresh_state["next_after"].is_null());
    let fresh_action = exact_action(&fresh_state, "list_programs").unwrap();
    assert_eq!(fresh_action["arguments"], json!({"limit":25}));
    let fresh_page = fresh
        .call("list_programs", fresh_action["arguments"].clone())
        .await;
    assert!(fresh_page["next_after"].is_null());
    assert_eq!(fresh_page["programs"].as_array().unwrap().len(), 1);
    assert_eq!(fresh_page["programs"][0]["id"], legacy_program.to_string());
    assert_eq!(fresh_page["programs"][0]["name"], legacy_name);
    fresh.finish().await;

    // Add a small current Program only after proving the historical fallback.
    let current_program = current
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"cursor sentinel"}),
        )
        .await;
    let current_program = program_id(&current_program);
    let mut after: Option<String> = None;
    let mut seen = HashSet::new();
    let mut saw_legacy = false;
    let mut saw_cursor = false;
    loop {
        let arguments = after.as_ref().map_or_else(
            || json!({"limit":1}),
            |cursor| json!({"after":cursor,"limit":1}),
        );
        let page = current.call("list_programs", arguments).await;
        let programs = page["programs"].as_array().unwrap();
        assert_eq!(programs.len(), 1);
        let id: Uuid = programs[0]["id"].as_str().unwrap().parse().unwrap();
        assert!(seen.insert(id));
        if id == legacy_program {
            assert_eq!(programs[0]["name"], legacy_name);
            saw_legacy = true;
        }
        if page["next_after"].is_null() {
            assert_eq!(
                page["actions"].as_array().unwrap().last().unwrap()["tool"],
                "begin_program"
            );
            break;
        }
        let cursor = page["next_after"].as_str().unwrap().to_owned();
        let action = exact_action(&page, "list_programs").unwrap();
        assert_eq!(action["arguments"], json!({"after":cursor,"limit":25}));
        after = Some(cursor);
        saw_cursor = true;
    }
    assert_eq!(seen, HashSet::from([legacy_program, current_program]));
    assert!(saw_legacy && saw_cursor);
    write_result(
        &result_path,
        &json!({
            "baseline_commit":BASELINE,
            "search_attempts":attempts,
            "accepted_name_chars":low,
            "accepted_name_utf8_bytes":legacy_name.len(),
            "first_rejected_name_chars":high,
            "selected_worktrees":100,
            "selected_path_bytes":4096,
            "legacy_api":{"open":"ok","get_state":"ok","begin_program":"ok",
                "save_boundary":"ok","exact_replay":"ok","upper_bound":"request_too_large",
                "get_program_exact":"ok"},
            "current_api":{"inspect_setup":"ok","get_state_fallback":"ok",
                "programs_delivery":"use_list_programs","fallback_action_executed":"limit_25",
                "list_programs_exact":"ok","fresh_native_session":"truthful",
                "fresh_selected_worktrees":100,
                "cursor":"paged_and_terminal_null","begin_program_final":"ok",
                "false_no_programs_intro":false}
        }),
    );
    current.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
