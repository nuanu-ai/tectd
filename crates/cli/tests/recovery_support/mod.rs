use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tect_domain::HostAuth;
use tokio::{
    io::{AsyncBufReadExt, AsyncWriteExt, BufReader},
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub fn tool_payload(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert!(result.get("structuredContent").is_none(), "{response}");
    let content = result["content"].as_array().expect("tool content array");
    assert_eq!(content.len(), 2, "{response}");
    assert_eq!(content[0]["type"], "text", "{response}");
    let intro = content[0]["text"].as_str().expect("fixed tool intro");
    assert!(!intro.is_empty() && intro.len() <= 2_000, "{response}");
    assert_eq!(content[1]["type"], "text", "{response}");
    let payload: Value =
        serde_json::from_str(content[1]["text"].as_str().expect("JSON tool payload"))
            .expect("content[1] must contain one JSON object");
    assert!(payload.is_object(), "{payload}");
    assert!(payload["actions"].is_array(), "{payload}");
    assert!(
        payload["recommended_action"].is_number() || payload["recommended_action"].is_null(),
        "{payload}"
    );
    payload
}

pub fn host_file(path: &Path, auth: &HostAuth) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(auth).unwrap()).unwrap();
}
pub fn private_temp() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}
pub fn tagged_url(url: &str, tag: &str) -> String {
    format!(
        "{url}{}application_name={tag}",
        if url.contains('?') { "&" } else { "?" }
    )
}

pub fn public_call(name: &str, arguments: Value) -> Value {
    let routed = match name {
        "get_state" => return json!({"name":"get_state","arguments":arguments}),
        "get_program" => ("query", "program.get"),
        "list_programs" => ("query", "program.list"),
        "list_sources" => ("query", "source.list"),
        "get_setup" => ("query", "setup.get"),
        "candidate_context" => ("query", "scope.candidates.context"),
        "open_workspace" => ("command", "workspace.open"),
        "register_source" => ("command", "source.register"),
        "select_worktrees" => ("command", "session.select_worktrees"),
        "begin_program" => ("command", "program.begin"),
        "save_program" => ("command", "program.save"),
        "record_program_input" => ("command", "program.record_input"),
        "inspect_setup" => ("command", "setup.inspect"),
        "begin_setup" => ("command", "setup.begin"),
        "save_setup" => ("command", "setup.save"),
        "record_setup_input" => ("command", "setup.record_input"),
        "begin_candidate_set" => ("command", "scope.candidates.begin"),
        "save_candidate_set" => ("command", "scope.candidates.save"),
        "record_candidate_input" => ("command", "scope.candidates.record_input"),
        "refresh_candidate_set" => ("command", "scope.candidates.refresh"),
        "scope_candidate_delta" => ("command", "scope.candidates.delta"),
        "scope_candidate_delta_status" => ("query", "scope.candidates.delta.status"),
        "pipeline_run_migrate" => ("command", "slice.pipeline.run.migrate"),
        "apply_setup" => ("execute", "setup.apply"),
        "read_skill" => {
            return json!({"name":"help","arguments":{
                "mode":"describe","method":arguments["name"]
            }});
        }
        _ => return json!({"name":name,"arguments":arguments}),
    };
    json!({"name":routed.0,"arguments":{"route":routed.1,"params":arguments}})
}

#[allow(dead_code)]
pub fn action_name(action: &Value) -> Option<&str> {
    action["arguments"]["route"]
        .as_str()
        .or_else(|| action["arguments"]["method"].as_str())
        .or_else(|| action["tool"].as_str())
}

#[allow(dead_code)]
pub fn action_params(action: &Value) -> &Value {
    action["arguments"]
        .get("params")
        .unwrap_or(&action["arguments"])
}

#[allow(dead_code)]
pub fn find_action<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
    payload["actions"]
        .as_array()?
        .iter()
        .find(|action| action_name(action) == Some(name))
}

#[allow(dead_code)]
pub fn ready_action(name: &str, arguments: Value) -> Value {
    let mut call = public_call(name, arguments);
    call["tool"] = call["name"].take();
    call.as_object_mut().unwrap().remove("name");
    call["kind"] = json!("ready_call");
    call
}
pub struct Daemon {
    pub child: Child,
    pub socket: PathBuf,
    inode: (u64, u64),
}
impl Daemon {
    pub async fn start(url: &str, socket: PathBuf) -> Self {
        Self::start_with(Path::new(env!("CARGO_BIN_EXE_tectd")), url, socket).await
    }

    pub async fn start_with(binary: &Path, url: &str, socket: PathBuf) -> Self {
        Self::start_configured(binary, url, socket, None).await
    }

    #[allow(dead_code)]
    pub async fn start_maintenance(url: &str, socket: PathBuf, contexts: &Path) -> Self {
        Self::start_configured(
            Path::new(env!("CARGO_BIN_EXE_tectd")),
            url,
            socket,
            Some(contexts),
        )
        .await
    }

    async fn start_configured(
        binary: &Path,
        url: &str,
        socket: PathBuf,
        maintenance_contexts: Option<&Path>,
    ) -> Self {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(socket.with_extension("stderr"))
            .unwrap();
        let mut command = Command::new(binary);
        command
            .env("TECT_DATABASE_URL", url)
            .env("TECT_SOCKET", &socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        if let Some(contexts) = maintenance_contexts {
            command
                .env("TECT_KNOWLEDGE_MAINTENANCE", "1")
                .env("TECT_KNOWLEDGE_SEARCH_CONTEXTS", contexts);
        }
        let mut child = command.spawn().unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "owned daemon exited during startup"
                );
                if fs::symlink_metadata(&socket).is_ok_and(|m| {
                    m.file_type().is_socket() && m.permissions().mode() & 0o777 == 0o600
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let metadata = fs::symlink_metadata(&socket).unwrap();
        assert!(metadata.file_type().is_socket());
        Self {
            child,
            socket,
            inode: (metadata.dev(), metadata.ino()),
        }
    }
    pub async fn crash(&mut self) {
        self.child.start_kill().unwrap();
        let exit = self.child.wait().await.unwrap();
        assert!(!exit.success());
    }
    pub fn remove_owned_stale_socket(&mut self) {
        assert!(
            self.child.try_wait().unwrap().is_some(),
            "never clean a live child's socket"
        );
        let metadata = fs::symlink_metadata(&self.socket).unwrap();
        assert!(metadata.file_type().is_socket());
        assert_eq!((metadata.dev(), metadata.ino()), self.inode);
        fs::remove_file(&self.socket).unwrap();
    }
}

pub struct Mcp {
    pub child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    sequence: u64,
    native: String,
}
impl Mcp {
    /// Native IDs here are explicitly synthetic integration fixtures.
    pub async fn start(socket: &Path, config: &Path, native: &str, key: &str) -> Self {
        Self::start_with(
            Path::new(env!("CARGO_BIN_EXE_tectd-mcp")),
            socket,
            config,
            native,
            key,
        )
        .await
    }

    pub async fn start_with(
        binary: &Path,
        socket: &Path,
        config: &Path,
        native: &str,
        key: &str,
    ) -> Self {
        let mut child = Command::new(binary)
            .env("TECT_SOCKET", socket)
            .env("TECT_HOST_CONFIG", config)
            .env("TECT_WORKSPACE_KEY", key)
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CODEX_THREAD_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        let input = child.stdin.take().unwrap();
        let output = BufReader::new(child.stdout.take().unwrap());
        let mut client = Self {
            child,
            input,
            output,
            sequence: 0,
            native: native.to_owned(),
        };
        client.exchange("initialize", json!({"protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"tect-recovery-test","version":"1"}})).await;
        client
            .input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        client.input.flush().await.unwrap();
        client
    }
    pub async fn send(&mut self, method: &str, mut params: Value) {
        if method == "tools/call" {
            params["_meta"] = json!({"threadId": self.native});
        }
        self.sequence += 1;
        let message = json!({"jsonrpc":"2.0","id":self.sequence,"method":method,"params":params});
        self.input
            .write_all(format!("{message}\n").as_bytes())
            .await
            .unwrap();
        self.input.flush().await.unwrap();
    }
    pub async fn exchange(&mut self, method: &str, params: Value) -> Value {
        self.send(method, params).await;
        let mut line = String::new();
        let size = tokio::time::timeout(Duration::from_secs(15), self.output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        assert!(size > 0, "MCP bridge ended without a response");
        serde_json::from_str(&line).unwrap()
    }
    pub async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let call = public_call(name, arguments);
        let response = self.exchange("tools/call", call).await;
        assert!(
            response.get("error").is_none() && response["result"]["isError"] != true,
            "{response}"
        );
        tool_payload(&response)
    }
    // This module is compiled once per integration binary; only refusal suites use this path.
    #[allow(dead_code)]
    pub async fn call_error(&mut self, name: &str, arguments: Value) -> Value {
        let call = public_call(name, arguments);
        let response = self.exchange("tools/call", call).await;
        assert_eq!(response["result"]["isError"], true, "{response}");
        tool_payload(&response)
    }
    // Only the process-loss binary intentionally kills a live MCP child.
    #[allow(dead_code)]
    pub async fn kill(&mut self) {
        self.child.start_kill().unwrap();
        let _ = self.child.wait().await.unwrap();
    }
    pub async fn finish(mut self) {
        self.input.shutdown().await.unwrap();
        drop(self.input);
        assert!(
            tokio::time::timeout(Duration::from_secs(5), self.child.wait())
                .await
                .unwrap()
                .unwrap()
                .success()
        );
    }
}
