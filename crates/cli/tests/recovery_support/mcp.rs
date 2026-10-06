use super::{kill_and_reap, normal_finish, public_call, tool_payload, wire};
use serde_json::{Value, json};
use std::{path::Path, process::Stdio, time::Duration};
use tokio::{
    io::BufReader,
    process::{Child, ChildStdin, ChildStdout, Command},
};

pub struct Mcp {
    pub child: Child,
    pub(super) input: ChildStdin,
    pub(super) output: BufReader<ChildStdout>,
    pub(super) sequence: u64,
    pub(super) native: String,
    pub(super) poisoned: bool,
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
        Self::start_with_result(binary, socket, config, native, key)
            .await
            .unwrap()
    }
    pub async fn start_with_result(
        binary: &Path,
        socket: &Path,
        config: &Path,
        native: &str,
        key: &str,
    ) -> Result<Self, String> {
        let child = Self::command(binary, socket, config, key)
            .spawn()
            .map_err(|_| "MCP spawn".to_owned())?;
        Self::from_child(child, native).await
    }
    pub(super) fn command(binary: &Path, socket: &Path, config: &Path, key: &str) -> Command {
        let mut command = Command::new(binary);
        Self::configure(&mut command, socket, config, key);
        command
    }
    pub(super) fn configure(command: &mut Command, socket: &Path, config: &Path, key: &str) {
        command
            .env_clear()
            .env("TECT_SOCKET", socket)
            .env("TECT_HOST_CONFIG", config)
            .env("TECT_WORKSPACE_KEY", key)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::null())
            .kill_on_drop(true);
    }
    pub(super) async fn from_child(mut child: Child, native: &str) -> Result<Self, String> {
        let (input, output) = Self::setup(&mut child).await?;
        Ok(Self {
            child,
            input,
            output,
            sequence: 1,
            native: native.into(),
            poisoned: false,
        })
    }
    pub(super) async fn setup(
        child: &mut Child,
    ) -> Result<(ChildStdin, BufReader<ChildStdout>), String> {
        let input = child.stdin.take();
        let output = child.stdout.take().map(BufReader::new);
        Self::setup_pipes(child, input, output, wire::REQUEST_DEADLINE).await
    }
    pub(super) async fn setup_pipes<I, O>(
        child: &mut Child,
        input: Option<I>,
        output: Option<O>,
        deadline: Duration,
    ) -> Result<(I, O), String>
    where
        I: tokio::io::AsyncWrite + Unpin,
        O: tokio::io::AsyncBufRead + Unpin,
    {
        // Retain both optional pipes until the owned child has been killed/reaped.
        let (mut input, mut output) = match (input, output) {
            (Some(input), Some(output)) => (input, output),
            pipes => {
                let cleanup = kill_and_reap(child).await;
                drop(pipes);
                return Err(super::retain_first("MCP pipe setup".into(), cleanup));
            }
        };
        let init = json!({"jsonrpc":"2.0","id":1,"method":"initialize","params":{
            "protocolVersion":"2025-06-18","capabilities":{},"clientInfo":{"name":"tect-recovery-test","version":"1"}}});
        let initialized = json!({"jsonrpc":"2.0","method":"notifications/initialized"});
        let started = tokio::time::Instant::now();
        let setup = tokio::time::timeout(deadline, async {
            let response = wire::exchange(&mut input, &mut output, &init, 1).await?;
            if response.get("error").is_some() {
                return Err("MCP initialize JSON-RPC error".into());
            }
            if !response["result"].is_object()
                || response["result"]["protocolVersion"] != "2025-06-18"
            {
                return Err("MCP initialize protocol version".into());
            }
            wire::send(&mut input, &initialized).await?;
            if started.elapsed() >= deadline {
                return Err("MCP initialization deadline".into());
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("MCP initialization deadline".into()));
        if let Err(original) = setup {
            let cleanup = kill_and_reap(child).await;
            drop((input, output));
            return Err(super::retain_first(original, cleanup));
        }
        Ok((input, output))
    }
    #[allow(dead_code)]
    pub async fn send_result(&mut self, method: &str, params: Value) -> Result<(), String> {
        let started = tokio::time::Instant::now();
        let result = tokio::time::timeout(wire::REQUEST_DEADLINE, async {
            let message = self.message(method, params)?;
            wire::send(&mut self.input, &message).await?;
            if started.elapsed() >= wire::REQUEST_DEADLINE {
                return Err("MCP send deadline".into());
            }
            Ok(())
        })
        .await
        .unwrap_or_else(|_| Err("MCP send deadline".into()));
        self.after_transport(result).await
    }
    #[allow(dead_code)]
    pub async fn send(&mut self, method: &str, params: Value) {
        self.send_result(method, params).await.unwrap();
    }
    pub async fn exchange_result(&mut self, method: &str, params: Value) -> Result<Value, String> {
        let started = tokio::time::Instant::now();
        let result = tokio::time::timeout(wire::REQUEST_DEADLINE, async {
            let message = self.message(method, params)?;
            let response =
                wire::exchange(&mut self.input, &mut self.output, &message, self.sequence).await?;
            if started.elapsed() >= wire::REQUEST_DEADLINE {
                return Err("MCP request deadline".into());
            }
            Ok(response)
        })
        .await
        .unwrap_or_else(|_| Err("MCP request deadline".into()));
        self.after_transport(result).await
    }
    pub async fn exchange(&mut self, method: &str, params: Value) -> Value {
        self.exchange_result(method, params).await.unwrap()
    }
    fn message(&mut self, method: &str, mut params: Value) -> Result<Value, String> {
        if self.poisoned {
            return Err("MCP connection poisoned".into());
        }
        self.sequence = self
            .sequence
            .checked_add(1)
            .ok_or("MCP request sequence exhausted")?;
        if method == "tools/call" {
            let object = params
                .as_object_mut()
                .ok_or("MCP tools/call params shape")?;
            object.insert("_meta".into(), json!({"threadId": self.native}));
        }
        Ok(json!({"jsonrpc":"2.0","id":self.sequence,"method":method,"params":params}))
    }
    async fn after_transport<T>(&mut self, result: Result<T, String>) -> Result<T, String> {
        match result {
            Ok(value) => Ok(value),
            Err(original) => {
                self.poisoned = true;
                let cleanup = kill_and_reap(&mut self.child).await;
                Err(super::retain_first(original, cleanup))
            }
        }
    }
    pub async fn call(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .exchange("tools/call", public_call(name, arguments))
            .await;
        assert!(
            response.get("error").is_none() && response["result"]["isError"] != true,
            "MCP tool failure"
        );
        tool_payload(&response)
    }
    #[allow(dead_code)]
    pub async fn call_error(&mut self, name: &str, arguments: Value) -> Value {
        let response = self
            .exchange("tools/call", public_call(name, arguments))
            .await;
        assert_eq!(
            response["result"]["isError"], true,
            "MCP expected tool refusal"
        );
        tool_payload(&response)
    }
    #[allow(dead_code)]
    pub async fn kill(&mut self) {
        kill_and_reap(&mut self.child).await.unwrap();
        self.poisoned = true;
    }
    pub async fn finish_result(self) -> Result<(), String> {
        let Self {
            mut child,
            input,
            output: _output,
            ..
        } = self;
        let normal = normal_finish(input, &mut child, Duration::from_secs(5)).await;
        if let Err(original) = normal {
            return Err(super::retain_first(
                original,
                kill_and_reap(&mut child).await,
            ));
        }
        Ok(())
    }
    pub async fn finish(self) {
        self.finish_result().await.unwrap();
    }
}
