use crate::TestResult;
use serde_json::{Value, json};
use std::fs;
use std::io::Write;
use std::os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt};
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tect_domain::HostAuth;
use tokio::io::{AsyncBufReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, ChildStdout, Command};

const RESPONSE_TIMEOUT: Duration = Duration::from_secs(30);
const STARTUP_TIMEOUT: Duration = Duration::from_secs(10);

pub(crate) fn write_private_config(path: &Path, auth: &HostAuth) -> TestResult<()> {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    file.write_all(&serde_json::to_vec(auth)?)?;
    file.sync_all()?;
    Ok(())
}

pub(crate) struct OwnedDaemon {
    child: Child,
    socket: PathBuf,
    inode: (u64, u64),
}

impl OwnedDaemon {
    pub(crate) async fn start(socket: &Path, runtime_url: &str) -> TestResult<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tectd"))
            .env("TECT_DATABASE_URL", runtime_url)
            .env("TECT_SOCKET", socket)
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;

        tokio::time::timeout(STARTUP_TIMEOUT, async {
            loop {
                if let Some(status) = child.try_wait()? {
                    return Err(std::io::Error::other(format!(
                        "owned tectd exited during startup: {status}"
                    )));
                }
                if let Ok(metadata) = fs::symlink_metadata(socket)
                    && metadata.file_type().is_socket()
                    && metadata.permissions().mode() & 0o7777 == 0o600
                {
                    return Ok(());
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .map_err(|_| std::io::Error::other("owned tectd startup timed out"))??;

        let metadata = fs::symlink_metadata(socket)?;
        if !metadata.file_type().is_socket() {
            return Err(std::io::Error::other("owned tectd path is not a socket").into());
        }
        Ok(Self {
            child,
            socket: socket.to_owned(),
            inode: (metadata.dev(), metadata.ino()),
        })
    }

    pub(crate) async fn stop(mut self) -> TestResult<()> {
        if self.child.try_wait()?.is_none() {
            self.child.start_kill()?;
        }
        let _ = self.child.wait().await?;
        self.remove_owned_socket()?;
        Ok(())
    }

    fn remove_owned_socket(&self) -> TestResult<()> {
        let metadata = match fs::symlink_metadata(&self.socket) {
            Ok(metadata) => metadata,
            Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
            Err(error) => return Err(error.into()),
        };
        if !metadata.file_type().is_socket() || (metadata.dev(), metadata.ino()) != self.inode {
            return Err(std::io::Error::other(
                "refusing to remove a socket not owned by this test",
            )
            .into());
        }
        fs::remove_file(&self.socket)?;
        Ok(())
    }
}

impl Drop for OwnedDaemon {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}

pub(crate) struct Bridge {
    child: Child,
    input: ChildStdin,
    output: BufReader<ChildStdout>,
    sequence: u64,
    native_id: String,
}

impl Bridge {
    pub(crate) async fn start(
        socket: &Path,
        config: &Path,
        native_id: &str,
        workspace_key: &str,
    ) -> TestResult<Self> {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
            .env("TECT_SOCKET", socket)
            .env("TECT_HOST_CONFIG", config)
            .env("TECT_WORKSPACE_KEY", workspace_key)
            .env_remove("CODEX_SESSION_ID")
            .env_remove("CODEX_THREAD_ID")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true)
            .spawn()?;
        let input = child
            .stdin
            .take()
            .ok_or_else(|| std::io::Error::other("missing bridge stdin"))?;
        let output = child
            .stdout
            .take()
            .ok_or_else(|| std::io::Error::other("missing bridge stdout"))?;
        let mut bridge = Self {
            child,
            input,
            output: BufReader::new(output),
            sequence: 0,
            native_id: native_id.to_owned(),
        };
        let initialized = bridge
            .exchange(
                "initialize",
                json!({
                    "protocolVersion":"2025-06-18",
                    "capabilities":{},
                    "clientInfo":{"name":"tect-performance-test","version":"1"}
                }),
            )
            .await?;
        if initialized.get("error").is_some() {
            return Err(std::io::Error::other("MCP initialize returned an error").into());
        }
        bridge
            .input
            .write_all(b"{\"jsonrpc\":\"2.0\",\"method\":\"notifications/initialized\"}\n")
            .await?;
        bridge.input.flush().await?;
        Ok(bridge)
    }

    pub(crate) async fn tool_call(&mut self, name: &str, arguments: Value) -> TestResult<Value> {
        let (name, arguments) = public_call(name, arguments);
        self.exchange(
            "tools/call",
            json!({
                "name":name, "arguments":arguments,
                "_meta":{"threadId":self.native_id}
            }),
        )
        .await
    }

    async fn exchange(&mut self, method: &str, params: Value) -> TestResult<Value> {
        self.sequence += 1;
        let request_id = self.sequence;
        let message = json!({"jsonrpc":"2.0","id":request_id,"method":method,"params":params});
        self.input
            .write_all(format!("{message}\n").as_bytes())
            .await?;
        self.input.flush().await?;
        let mut line = String::new();
        let size = tokio::time::timeout(RESPONSE_TIMEOUT, self.output.read_line(&mut line))
            .await
            .map_err(|_| std::io::Error::other("MCP response timed out"))??;
        if size == 0 {
            return Err(std::io::Error::other("MCP bridge ended without a response").into());
        }
        let response: Value = serde_json::from_str(&line)?;
        if response["id"] != request_id {
            return Err(std::io::Error::other("MCP response ID mismatch").into());
        }
        Ok(response)
    }

    pub(crate) async fn stop(mut self) {
        let _ = self.input.shutdown().await;
        match tokio::time::timeout(Duration::from_secs(5), self.child.wait()).await {
            Ok(Ok(_)) => {}
            _ => {
                let _ = self.child.start_kill();
                let _ = self.child.wait().await;
            }
        }
    }
}

fn public_call(name: &str, arguments: Value) -> (&str, Value) {
    let routed = match name {
        "get_state" => return ("get_state", arguments),
        "get_program" => ("query", "program.get"),
        "list_programs" => ("query", "program.list"),
        "list_sources" => ("query", "source.list"),
        "get_setup" => ("query", "setup.get"),
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
        "apply_setup" => ("execute", "setup.apply"),
        _ => return (name, arguments),
    };
    (routed.0, json!({"route":routed.1,"params":arguments}))
}

impl Drop for Bridge {
    fn drop(&mut self) {
        let _ = self.child.start_kill();
    }
}
