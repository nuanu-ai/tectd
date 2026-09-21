use serde_json::{Value, json};
use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

pub struct LegacyDaemon {
    child: Child,
    socket: PathBuf,
    identity: (u64, u64),
}

impl LegacyDaemon {
    pub async fn start(binary: &Path, url: &str, socket: PathBuf) -> Self {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(socket.with_extension("legacy-stderr"))
            .unwrap();
        let mut child = Command::new(binary)
            .env("TECT_DATABASE_URL", url)
            .env("TECT_SOCKET", &socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .unwrap();
        tokio::time::timeout(Duration::from_secs(5), async {
            loop {
                assert!(child.try_wait().unwrap().is_none(), "legacy daemon exited");
                if fs::symlink_metadata(&socket).is_ok_and(|metadata| {
                    metadata.file_type().is_socket()
                        && metadata.permissions().mode() & 0o777 == 0o600
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap();
        let metadata = fs::symlink_metadata(&socket).unwrap();
        Self {
            child,
            socket,
            identity: (metadata.dev(), metadata.ino()),
        }
    }

    pub async fn stop(mut self) {
        self.child.start_kill().unwrap();
        let exit = self.child.wait().await.unwrap();
        assert!(!exit.success());
        let metadata = fs::symlink_metadata(&self.socket).unwrap();
        assert!(metadata.file_type().is_socket());
        assert_eq!((metadata.dev(), metadata.ino()), self.identity);
        fs::remove_file(&self.socket).unwrap();
    }
}

pub struct LegacyMcp {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    sequence: u64,
    native: String,
}

impl LegacyMcp {
    pub async fn start(
        binary: &Path,
        socket: &Path,
        config: &Path,
        native: &str,
        workspace: &str,
    ) -> Self {
        let mut child = Command::new(binary)
            .env("TECT_SOCKET", socket)
            .env("TECT_HOST_CONFIG", config)
            .env("TECT_WORKSPACE_KEY", workspace)
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
        let initialized = client
            .exchange(
                "initialize",
                json!({"protocolVersion":"2025-06-18","capabilities":{},
                    "clientInfo":{"name":"legacy-capacity-proof","version":"1"}}),
            )
            .await;
        assert!(initialized.get("error").is_none());
        client
            .input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await
            .unwrap();
        client.input.flush().await.unwrap();
        client
    }

    async fn exchange(&mut self, method: &str, mut params: Value) -> Value {
        if method == "tools/call" {
            params["_meta"] = json!({"threadId":self.native});
        }
        self.sequence += 1;
        let request = json!({"jsonrpc":"2.0","id":self.sequence,"method":method,"params":params});
        self.input
            .write_all(serde_json::to_string(&request).unwrap().as_bytes())
            .await
            .unwrap();
        self.input.write_all(b"\n").await.unwrap();
        self.input.flush().await.unwrap();
        let mut line = String::new();
        let bytes = tokio::time::timeout(Duration::from_secs(15), self.output.read_line(&mut line))
            .await
            .unwrap()
            .unwrap();
        assert!(bytes > 0, "legacy MCP ended without response");
        serde_json::from_str(&line).unwrap()
    }

    pub async fn call(&mut self, name: &str, arguments: Value) -> (bool, Value) {
        let response = self
            .exchange("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        assert!(response.get("error").is_none(), "legacy JSON-RPC error");
        let result = &response["result"];
        let is_error = result["isError"].as_bool().unwrap_or(false);
        let content = result["content"].as_array().expect("legacy content");
        assert_eq!(content.len(), 3, "legacy content shape");
        assert!(
            content[2]["text"]
                .as_str()
                .unwrap()
                .contains("TECTD RESPONSE RULES")
        );
        let payload: Value =
            serde_json::from_str(content[1]["text"].as_str().expect("legacy JSON payload"))
                .unwrap();
        assert!(payload.as_object().is_some());
        (!is_error, payload)
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
