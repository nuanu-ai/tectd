//! End-to-end MCP stdio -> daemon -> PostgreSQL contract for the compact Slice catalogue.
#[path = "mcp_summary/catalog_reads.rs"]
mod catalog_reads;
#[path = "recovery_support/help_reads.rs"]
mod help_reads;

use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    process::Stdio,
    sync::Arc,
};
use tect_application::WorkspaceService;
use tect_domain::HostAuth;
use tect_postgres::{PgStore, admin};
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use uuid::Uuid;

fn host_file(path: &std::path::Path, auth: &HostAuth) {
    use std::io::Write;
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(auth).unwrap()).unwrap();
}

struct Bridge {
    child: tokio::process::Child,
    input: tokio::process::ChildStdin,
    output: BufReader<tokio::process::ChildStdout>,
    thread_id: String,
}

impl Bridge {
    async fn start(socket: &std::path::Path, config: &std::path::Path, thread_id: &str) -> Self {
        let mut child = tokio::process::Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
            .env("TECT_SOCKET", socket)
            .env("TECT_HOST_CONFIG", config)
            .env("TECT_WORKSPACE_KEY", "summary-test")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut bridge = Self {
            child,
            input,
            output,
            thread_id: thread_id.into(),
        };
        let init = bridge
            .exchange(json!({"jsonrpc":"2.0","id":1,"method":"initialize",
            "params":{"protocolVersion":"2025-06-18","capabilities":{},
                "clientInfo":{"name":"summary-test","version":"1"}}}))
            .await;
        assert!(init.get("error").is_none(), "{init}");
        bridge
            .input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        bridge
    }

    async fn exchange(&mut self, request: Value) -> Value {
        self.input
            .write_all(format!("{request}\n").as_bytes())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
        let mut line = String::new();
        let size = tokio::time::timeout(
            std::time::Duration::from_secs(5),
            self.output.read_line(&mut line),
        )
        .await
        .unwrap()
        .unwrap();
        assert!(size > 0, "bridge ended before response");
        serde_json::from_str(&line).unwrap()
    }

    async fn call(&mut self, id: usize, tool: &str, arguments: Value) -> Value {
        let thread_id = self.thread_id.clone();
        self.exchange(json!({"jsonrpc":"2.0","id":id,"method":"tools/call",
            "params":{"name":tool,"arguments":arguments,"_meta":{"threadId":thread_id}}}))
            .await
    }

    async fn pipelines(&mut self, id: usize, params: Value) -> Value {
        self.call(
            id,
            "query",
            json!({"route":"slice.pipelines","params":params}),
        )
        .await
    }

    async fn stop(mut self) {
        drop(self.input);
        assert!(
            tokio::time::timeout(std::time::Duration::from_secs(5), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}

fn payload(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn summary_preserves_full_catalogue_and_rejects_unauthorized_host() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
    let service = Arc::new(WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let temp = tempfile::tempdir().unwrap();
    let private_path = temp.path().canonicalize().unwrap();
    std::fs::set_permissions(&private_path, std::fs::Permissions::from_mode(0o700)).unwrap();
    let socket = private_path.join("d.sock");
    let config = private_path.join("host.json");
    host_file(&config, &enrollment.auth);
    let listener = tokio::net::UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let daemon = tokio::spawn(tect_host::serve(listener, service));
    let native_id = Uuid::new_v4().to_string();
    let mut bridge = Bridge::start(&socket, &config, &native_id).await;

    let mut sequence = 100;
    let omitted = catalog_reads::read(
        async |arguments| {
            sequence += 1;
            bridge.call(sequence, "query", arguments).await
        },
        json!({}),
    )
    .await;
    let full = catalog_reads::read(
        async |arguments| {
            sequence += 1;
            bridge.call(sequence, "query", arguments).await
        },
        json!({"view":"full"}),
    )
    .await;
    assert_eq!(omitted.initial_arguments["params"], json!({}));
    assert_eq!(omitted.value, full.value);
    assert_eq!(omitted.source, full.source);
    assert_eq!(omitted.digest, full.digest);
    assert!(
        full.pages > 1,
        "full catalogue must preserve its large bodies"
    );
    assert!(omitted.maximum_envelope_bytes <= 8192 && full.maximum_envelope_bytes <= 8192);
    let full = full.value;
    let summary = catalog_reads::read(
        async |arguments| {
            sequence += 1;
            bridge.call(sequence, "query", arguments).await
        },
        json!({"view":"summary"}),
    )
    .await
    .value;
    assert_eq!(summary["view"], "summary");
    for field in ["revision", "digest", "executable_count"] {
        assert_eq!(summary[field], full[field], "{field}");
    }
    let short = summary["pipelines"].as_array().unwrap();
    let complete = full["pipelines"].as_array().unwrap();
    assert_eq!(short.len(), 9);
    assert_eq!(complete.len(), 9);
    assert_eq!(
        full["knowledge_change_entry"]["route"],
        "knowledge.change_begin"
    );
    assert_eq!(
        full["knowledge_change_entry"]["definition"]["phases"]
            .as_array()
            .unwrap()
            .len(),
        12
    );
    assert_eq!(
        complete
            .iter()
            .filter(|entry| entry["execution_owner"] == "knowledge_change")
            .count(),
        1
    );
    let body = full["promotion_method"]["body"].as_str().unwrap();
    assert_eq!(
        body,
        include_str!("../../host/knowledge-methods/promotion-slice.md")
    );
    assert_eq!(
        full["promotion_method"]["digest"],
        format!("{:x}", Sha256::digest(body.as_bytes()))
    );
    for (short, complete) in short.iter().zip(complete) {
        for field in ["kind", "description", "executable"] {
            assert_eq!(short[field], complete[field], "{field}");
        }
        assert_eq!(
            short["execution_owner"],
            complete
                .get("execution_owner")
                .cloned()
                .unwrap_or_else(|| json!("slice_pipeline_run"))
        );
    }
    assert!(summary.get("promotion_method").is_none());
    assert!(summary.get("knowledge_change_entry").is_none());
    let next = &summary["full_view"];
    assert_eq!(next["tool"], "query");
    assert_eq!(next["arguments"]["route"], "slice.pipelines");
    assert_eq!(
        catalog_reads::read(
            async |arguments| {
                sequence += 1;
                bridge.call(sequence, "query", arguments).await
            },
            next["arguments"]["params"].clone()
        )
        .await
        .value,
        full
    );
    let described = help_reads::describe(
        async |arguments| {
            sequence += 1;
            bridge.call(sequence, "help", arguments).await
        },
        json!({"mode":"describe","tool":"query","route":"slice.pipelines"}),
    )
    .await
    .unwrap()
    .value;
    assert_eq!(
        described["params_schema"]["properties"]["view"]["enum"],
        json!(["full", "summary"])
    );
    bridge.stop().await;

    let wrong_config = private_path.join("wrong-host.json");
    let mut wrong_auth = enrollment.auth;
    wrong_auth.credential = "0".repeat(64);
    host_file(&wrong_config, &wrong_auth);
    let mut wrong = Bridge::start(&socket, &wrong_config, &native_id).await;
    let denied = wrong.pipelines(2, json!({"view":"summary"})).await;
    assert_eq!(denied["result"]["isError"], true);
    let denied = payload(&denied);
    assert_eq!(denied["error"]["code"], "unauthorized");
    assert!(denied.get("pipelines").is_none());
    wrong.stop().await;
    daemon.abort();
    let _ = daemon.await;
}
