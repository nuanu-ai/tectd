//! The three advisory routes through a real daemon and MCP bridge, the daemon's
//! startup rules for the Jev environment, what its children inherit, and the real
//! HTTP adapter writing the journal. Jev itself is a loopback fake; nothing leaves
//! the machine.
#[allow(dead_code)]
mod recovery_support;
#[path = "advice_journal/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Mcp, host_file, private_temp, tagged_url, tool_payload};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    fs,
    io::{Read, Write},
    net::TcpListener,
    os::unix::fs::{FileTypeExt, OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Stdio,
    sync::Arc,
    thread,
    time::Duration,
};
use tect_application::{AdviceRequest, WorkspaceService};
use tect_domain::*;
use tect_postgres::{PgStore, admin};
use tokio::process::{Child, Command};
use uuid::Uuid;

const KEY: &str = "route-test-key-0123456789";
const UNUSED_ENDPOINT: &str = "http://127.0.0.1:9/v1/systemone";

struct Urls {
    pool: PgPool,
    runtime: String,
}

async fn database() -> Urls {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    Urls { pool, runtime }
}

/// The daemon with exactly the given Jev environment, whatever the test runner has.
fn daemon(url: &str, socket: &Path, jev: &[(&str, &str)], path: Option<&str>) -> Child {
    let log = fs::OpenOptions::new()
        .create(true)
        .append(true)
        .mode(0o600)
        .open(socket.with_extension("stderr"))
        .unwrap();
    let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
    command
        .env("TECT_DATABASE_URL", url)
        .env("TECT_SOCKET", socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::from(log))
        .kill_on_drop(true);
    for name in tect_host::JEV_ENVIRONMENT {
        command.env_remove(name);
    }
    for (name, value) in jev {
        command.env(name, value);
    }
    if let Some(path) = path {
        command.env("PATH", path);
    }
    command.spawn().unwrap()
}

async fn serving(child: &mut Child, socket: &Path) {
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "owned daemon exited during startup"
            );
            if fs::symlink_metadata(socket)
                .is_ok_and(|m| m.file_type().is_socket() && m.permissions().mode() & 0o777 == 0o600)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
}

async fn route(client: &mut Mcp, tool: &str, route: &str, params: Value) -> (Value, String) {
    let response = client
        .exchange(
            "tools/call",
            json!({"name":tool,"arguments":{"route":route,"params":params}}),
        )
        .await;
    let raw = response.to_string();
    (response, raw)
}

fn full_jev(endpoint: &str) -> [(&str, &str); 4] {
    [
        ("TECT_JEV_ENDPOINT", endpoint),
        ("TECT_JEV_MODEL", "jev-latest"),
        ("TECT_JEV_TIMEOUT_MS", "2000"),
        ("TYPESAFE_API_KEY", KEY),
    ]
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn daemon_start_follows_the_three_jev_variables_and_nothing_else() {
    let db = database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let prefix =
        |name: &'static str| tagged_url(&db.runtime, &format!("tect-{name}-{}", Uuid::new_v4()));

    // Any of the three without the complete set, or with an unusable value, stops the start.
    let refused: [&[(&str, &str)]; 6] = [
        &[("TECT_JEV_ENDPOINT", UNUSED_ENDPOINT)],
        &[("TECT_JEV_MODEL", "jev-latest"), ("TYPESAFE_API_KEY", KEY)],
        &[("TECT_JEV_TIMEOUT_MS", "2000"), ("TYPESAFE_API_KEY", KEY)],
        &[
            ("TECT_JEV_ENDPOINT", UNUSED_ENDPOINT),
            ("TECT_JEV_MODEL", "jev-latest"),
            ("TECT_JEV_TIMEOUT_MS", "2000"),
        ],
        &[
            ("TECT_JEV_ENDPOINT", UNUSED_ENDPOINT),
            ("TECT_JEV_MODEL", "jev-latest"),
            ("TECT_JEV_TIMEOUT_MS", "soon"),
            ("TYPESAFE_API_KEY", KEY),
        ],
        &[
            ("TECT_JEV_ENDPOINT", "https://example.com/v1/systemone"),
            ("TECT_JEV_MODEL", "jev-latest"),
            ("TECT_JEV_TIMEOUT_MS", "2000"),
            ("TYPESAFE_API_KEY", KEY),
        ],
    ];
    for (index, jev) in refused.into_iter().enumerate() {
        let socket = root.join(format!("refused-{index}.sock"));
        let mut child = daemon(&prefix("jev-refused"), &socket, jev, None);
        let exit = tokio::time::timeout(Duration::from_secs(5), child.wait())
            .await
            .unwrap()
            .unwrap();
        assert!(!exit.success(), "case {index} started");
        let stderr = fs::read_to_string(socket.with_extension("stderr")).unwrap();
        assert_eq!(stderr, "invalid_configuration\n", "case {index}");
        assert!(!stderr.contains(KEY));
        assert!(fs::symlink_metadata(&socket).is_err());
    }

    // A key alone configures nothing; the complete set configures the provider.
    let alone: &[(&str, &str)] = &[("TYPESAFE_API_KEY", KEY)];
    let complete = full_jev(UNUSED_ENDPOINT);
    for (name, jev, configured) in [("alone", alone, false), ("complete", &complete[..], true)] {
        let socket = root.join(format!("{name}.sock"));
        let mut child = daemon(&prefix("jev-started"), &socket, jev, None);
        serving(&mut child, &socket).await;
        let enrollment = admin::enroll_host(&db.pool, None, Vec::new())
            .await
            .unwrap();
        let config = root.join(format!("{name}.json"));
        host_file(&config, &enrollment.auth);
        let native = Uuid::new_v4().to_string();
        let key = format!("advice-start-{}", Uuid::new_v4().simple());
        let mut client = Mcp::start(&socket, &config, &native, &key).await;
        client.call("open_workspace", json!({})).await;
        let (response, raw) =
            route(&mut client, "query", "workspace.advisory.config", json!({})).await;
        let status = tool_payload(&response);
        assert_eq!(status["provider_configured"], configured, "{name}");
        assert_eq!(status["enabled"], false);
        assert_eq!(status["revision"], 0);
        assert!(!raw.contains(KEY));
        client.finish().await;
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn routes_configure_and_read_the_journal_and_no_child_inherits_the_key() {
    let db = database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();

    // A `git` that records the environment the daemon gave it.
    let bin = root.join("bin");
    fs::create_dir(&bin).unwrap();
    let dump = root.join("git-environment.txt");
    let fake_git = bin.join("git");
    fs::write(
        &fake_git,
        format!("#!/bin/sh\nenv > '{}'\nexit 1\n", dump.display()),
    )
    .unwrap();
    fs::set_permissions(&fake_git, fs::Permissions::from_mode(0o755)).unwrap();
    let path = format!("{}:{}", bin.display(), std::env::var("PATH").unwrap());

    let socket = root.join("advice.sock");
    let runtime = tagged_url(
        &db.runtime,
        &format!("tect-advice-routes-{}", Uuid::new_v4()),
    );
    let mut child = daemon(&runtime, &socket, &full_jev(UNUSED_ENDPOINT), Some(&path));
    serving(&mut child, &socket).await;
    let enrollment = admin::enroll_host(&db.pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: Uuid::new_v4().to_string(),
        workspace_key: format!("advice-routes-{}", Uuid::new_v4().simple()),
    };
    let mut client = Mcp::start(
        &socket,
        &config,
        &context.native_session_id,
        &context.workspace_key,
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let mut transcript = String::new();

    // The setting through the bridge.
    let (response, raw) = route(&mut client, "query", "workspace.advisory.config", json!({})).await;
    transcript.push_str(&raw);
    assert_eq!(
        tool_payload(&response),
        json!({"enabled":false,"daily_call_limit":null,"revision":0,"updated_at":null,
            "updated_by_session":null,"calls_today":0,"provider_configured":true,
            "actions":[],"recommended_action":null})
    );
    let (response, raw) = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"enabled":true,"expected_revision":0}),
    )
    .await;
    transcript.push_str(&raw);
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(
        tool_payload(&response)["error"]["code"],
        "invalid_arguments"
    );
    let (response, raw) = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"enabled":true,"daily_call_limit":3,"expected_revision":0}),
    )
    .await;
    transcript.push_str(&raw);
    let enabled = tool_payload(&response);
    assert_eq!(
        (
            &enabled["enabled"],
            &enabled["daily_call_limit"],
            &enabled["revision"]
        ),
        (&json!(true), &json!(3), &json!(1))
    );
    let (response, raw) = route(
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({"enabled":false,"expected_revision":0}),
    )
    .await;
    transcript.push_str(&raw);
    assert_eq!(response["result"]["isError"], true, "{response}");
    assert_eq!(tool_payload(&response)["error"]["code"], "stale_revision");

    // A real HTTP adapter against a loopback Jev that echoes the key it was sent.
    let answer = json!({"model":"jev-latest","debug":format!("authorized by {KEY}"),
        "answers":{"size":{"type":"choice","choice":"small",
            "probabilities":{"small":0.7,"large":0.2,"ABSTAIN":0.1},"confidence":0.8}},
        "usage":{"input_tokens":120}})
    .to_string();
    let listener = TcpListener::bind("127.0.0.1:0").unwrap();
    let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let jev = thread::spawn(move || {
        let (mut stream, _) = listener.accept().unwrap();
        let mut request = Vec::new();
        let mut chunk = [0; 4096];
        while !request.ends_with(b"}}") {
            let read = stream.read(&mut chunk).unwrap();
            assert!(read > 0);
            request.extend_from_slice(&chunk[..read]);
        }
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            answer.len()
        );
        stream.write_all(head.as_bytes()).unwrap();
        stream.write_all(answer.as_bytes()).unwrap();
        String::from_utf8(request).unwrap()
    });
    let provider = tect_host::JevAdviceProvider::new(
        tect_host::JevConfig::from_values(
            Some(&endpoint),
            Some("jev-latest"),
            Some("2000"),
            Some(KEY),
        )
        .unwrap()
        .unwrap(),
    )
    .unwrap();
    let store = Arc::new(PgStore::connect(&db.runtime, 4).await.unwrap());
    let service = WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    )
    .with_advice_provider(Arc::new(provider));
    // A task text large enough that the journal row exceeds the 8 KiB read budget.
    let mut questions = support::questions();
    questions.state = json!({"task": "Add invitations. ".repeat(900)});
    let work = AdviceWorkRefs {
        request_id: Some(Uuid::new_v4()),
        ..AdviceWorkRefs::default()
    };
    let request = AdviceRequest {
        point: "sizing",
        route: support::ROUTE,
        work: &work,
        questions: &questions,
        shaping: &support::Verbatim,
        reply: &support::Room(usize::MAX),
    };
    let field = service.advise(&context, &request).await.unwrap().unwrap();
    assert_eq!(field, support::advised("small", 0.7, 0.2, 0.1));
    let sent = jev.join().unwrap();
    assert!(sent.to_ascii_lowercase().contains(&format!(
        "authorization: bearer {}",
        KEY.to_ascii_lowercase()
    )));
    // The key travels in the header alone; no stored column and no reply carries it.
    let stored: Vec<String> = sqlx::query_scalar(
        "SELECT row_to_json(j)::text || convert_from(request_body, 'UTF8') \
             || convert_from(response_body, 'UTF8') \
         FROM advice_journal j WHERE tenant_id=$1",
    )
    .bind(enrollment.tenant_id)
    .fetch_all(&db.pool)
    .await
    .unwrap();
    assert_eq!(stored.len(), 1);
    assert!(stored[0].contains("authorized by [redacted]"));
    assert!(!stored[0].contains(KEY));
    assert!(!json!(field).to_string().contains(KEY));

    // The journal through the bridge: one small row whole, then the next page.
    let limited = AdviceRequest {
        point: "pipeline",
        ..request
    };
    let unsent = WorkspaceService::new(
        Arc::new(PgStore::connect(&db.runtime, 2).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    );
    let skipped = unsent.advise(&context, &limited).await.unwrap().unwrap();
    assert_eq!(
        skipped.reason(),
        Some(AdviceUnavailableReason::ProviderUnconfigured)
    );
    let (response, raw) = route(
        &mut client,
        "query",
        "workspace.advisory.audit",
        json!({"limit":1,"bodies":true}),
    )
    .await;
    transcript.push_str(&raw);
    let page = tool_payload(&response);
    assert_eq!(page["entries"].as_array().unwrap().len(), 1);
    assert_eq!(page["entries"][0]["point"], "pipeline");
    assert_eq!(page["entries"][0]["called"], false);
    assert_eq!(page["entries"][0]["advice"], json!(skipped));
    assert_eq!(page["entries"][0]["latency_ms"], Value::Null);
    assert_eq!(page["counters"]["called"], 1);
    assert_eq!(page["counters"]["not_called"], 1);
    assert_eq!(
        page["counters"]["reasons"],
        json!({"provider_unconfigured":1})
    );
    let next = &page["actions"][0];
    assert_eq!(next["arguments"]["route"], "workspace.advisory.audit");

    // The row with both bodies exceeds the read budget: byte windows, losslessly.
    let mut call = json!({"name":next["tool"],"arguments":next["arguments"]});
    let mut text = String::new();
    loop {
        let response = client.exchange("tools/call", call.clone()).await;
        transcript.push_str(&response.to_string());
        let part = tool_payload(&response);
        assert_eq!(part["kind"], "fragment", "{part}");
        text.push_str(part["text"].as_str().unwrap());
        let Some(more) = part["actions"].as_array().unwrap().first() else {
            break;
        };
        call = json!({"name":more["tool"],"arguments":more["arguments"]});
    }
    let older: Value = serde_json::from_str(&text).unwrap();
    let entry = &older["entries"][0];
    assert_eq!(entry["point"], "sizing");
    assert_eq!(entry["advice"], json!(field));
    assert_eq!(entry["request_id"], json!(work.request_id));
    assert_eq!(entry["input_tokens"], 120);
    assert_eq!(entry["output_tokens"], Value::Null);
    assert_eq!(entry["send_certainty"], "sent");
    let request_body: Value =
        serde_json::from_str(entry["request_body"].as_str().unwrap()).unwrap();
    assert_eq!(request_body["model"], "jev-latest");
    assert_eq!(request_body["questions"]["size"]["type"], "choice");
    assert_eq!(request_body["state"], questions.state);
    assert!(entry["request_bytes"].as_u64().unwrap() > 8192);
    assert!(
        entry["response_body"]
            .as_str()
            .unwrap()
            .contains("[redacted]")
    );
    assert_eq!(older["next_after"], Value::Null);

    // The daemon's own child: a git started for source registration.
    let source = root.join("source");
    fs::create_dir(&source).unwrap();
    let (response, raw) = route(
        &mut client,
        "command",
        "source.register",
        json!({"path":source.to_string_lossy()}),
    )
    .await;
    transcript.push_str(&raw);
    assert_eq!(response["result"]["isError"], true, "{response}");
    let environment = fs::read_to_string(&dump).unwrap();
    assert!(environment.contains("TECT_SOCKET="));
    assert!(!environment.contains("TYPESAFE_API_KEY"));
    assert!(!environment.contains("TECT_JEV_"));
    assert!(!environment.contains(KEY));

    assert!(!transcript.contains(KEY));
    assert!(
        !fs::read_to_string(socket.with_extension("stderr"))
            .unwrap()
            .contains(KEY)
    );
    client.finish().await;
}
