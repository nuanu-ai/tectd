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
pub struct Daemon {
    pub child: Child,
    pub socket: PathBuf,
    inode: (u64, u64),
}
impl Daemon {
    pub async fn start(url: &str, socket: PathBuf) -> Self {
        let log = fs::OpenOptions::new()
            .create(true)
            .append(true)
            .mode(0o600)
            .open(socket.with_extension("stderr"))
            .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_tectd"))
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
        let mut child = Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
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
        let response = self
            .exchange("tools/call", json!({"name":name,"arguments":arguments}))
            .await;
        assert!(
            response.get("error").is_none() && response["result"]["isError"] != true,
            "{response}"
        );
        response["result"]["structuredContent"].clone()
    }
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
