use super::*;
use std::{
    os::unix::fs::{FileTypeExt, MetadataExt},
    os::unix::net::UnixListener,
    pin::Pin,
    process::Stdio,
    task::{Context, Poll},
};
use tokio::{io::BufReader, process::Command};

// All processes are controlled builtin-sh or exec-sleep fixtures. No Tect/auth/PG.
fn child(script: &str, input: bool, output: bool) -> tokio::process::Child {
    child_with_arg(script, input, output, None)
}
fn child_with_arg(
    script: &str,
    input: bool,
    output: bool,
    arg: Option<&Path>,
) -> tokio::process::Child {
    let mut command = Command::new("/bin/sh");
    command.arg("-c").arg(script).arg("synthetic-fixture");
    if let Some(arg) = arg {
        command.arg(arg);
    }
    command
        .env_clear()
        .stdin(if input { Stdio::piped() } else { Stdio::null() })
        .stdout(if output {
            Stdio::piped()
        } else {
            Stdio::null()
        })
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap()
}
#[tokio::test]
async fn initialize_success_requires_initialized_send_only_notification() {
    let fixture = child(
        "IFS= read -r first || exit 10; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-06-18\"}}'; IFS= read -r second || exit 11; case \"$second\" in *notifications/initialized*) ;; *) exit 12;; esac; while IFS= read -r line; do :; done; exit 0",
        true,
        true,
    );
    let client = mcp::Mcp::from_child(fixture, "synthetic-startup").await;
    let result = match client {
        Ok(client) => client.finish_result().await,
        Err(error) => Err(error),
    };
    assert_eq!(result, Ok(()));
}
#[tokio::test]
async fn partial_pipe_failure_reaps_before_return_and_keeps_stdout_until_cleanup() {
    for (input, output) in [(false, true), (true, false)] {
        let mut fixture = child("exec /bin/sleep 60", input, output);
        let result = mcp::Mcp::setup(&mut fixture).await;
        let reaped_at_return = fixture.try_wait();
        let cleanup = kill_and_reap(&mut fixture).await;
        let exited = fixture.try_wait();
        assert_eq!(result.unwrap_err(), "MCP pipe setup");
        assert_eq!(cleanup, Ok(()));
        assert!(reaped_at_return.unwrap().is_some());
        assert!(exited.unwrap().is_some());
    }
}
#[tokio::test]
async fn initialize_invalid_response_reaps_and_retains_first_error() {
    for (reply, expected) in [
        (
            "{\"jsonrpc\":\"2.0\",\"id\":99,\"result\":{}}",
            "MCP response envelope or id",
        ),
        ("invalid-json", "MCP response JSON"),
    ] {
        let body = format!("IFS= read -r first; printf '%s\\n' '{reply}'; exec /bin/sleep 60");
        let mut fixture = child(&body, true, true);
        let result = mcp::Mcp::setup(&mut fixture).await;
        let reaped_at_return = fixture.try_wait();
        let cleanup = kill_and_reap(&mut fixture).await;
        let exited = fixture.try_wait();
        assert_eq!(result.unwrap_err(), expected);
        assert_eq!(cleanup, Ok(()));
        assert!(reaped_at_return.unwrap().is_some());
        assert!(exited.unwrap().is_some());
    }
}
#[tokio::test]
async fn initialize_rpc_error_and_invalid_protocol_reap_before_initialized() {
    for (reply, expected) in [
        (
            json!({"jsonrpc":"2.0","id":1,"error":{"code":-32602,"message":"invalid_params"}}),
            "MCP initialize JSON-RPC error",
        ),
        (
            json!({"jsonrpc":"2.0","id":1,"result":{}}),
            "MCP initialize protocol version",
        ),
        (
            json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":null}}),
            "MCP initialize protocol version",
        ),
        (
            json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":7}}),
            "MCP initialize protocol version",
        ),
        (
            json!({"jsonrpc":"2.0","id":1,"result":{"protocolVersion":"2025-03-26"}}),
            "MCP initialize protocol version",
        ),
        (
            json!({"jsonrpc":"2.0","id":1,"result":[]}),
            "MCP initialize protocol version",
        ),
    ] {
        let temp = private_temp();
        let marker = temp.path().join("unexpected-initialized");
        // Fixed replies contain no single quotes; marker is a positional arg.
        let body = format!(
            "IFS= read -r first; printf '%s\\n' '{reply}'; if IFS= read -r second; then printf unexpected > \"$1\"; fi; exec /bin/sleep 60"
        );
        let mut fixture = child_with_arg(&body, true, true, Some(&marker));
        let result = mcp::Mcp::setup(&mut fixture).await;
        let reaped_at_return = fixture.try_wait();
        let cleanup = kill_and_reap(&mut fixture).await;
        let notification_absent = !marker.exists();
        assert_eq!(cleanup, Ok(()));
        assert!(reaped_at_return.unwrap().is_some());
        assert_eq!(result.unwrap_err(), expected);
        assert!(notification_absent);
    }
}
#[tokio::test]
async fn valid_rpc_error_then_success_keeps_the_same_connection() {
    let mut fixture = child(
        "IFS= read -r first; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":1,\"error\":{\"code\":-32601,\"message\":\"method_not_found\"}}'; IFS= read -r second; printf '%s\\n' '{\"jsonrpc\":\"2.0\",\"id\":2,\"result\":{}}'; while IFS= read -r line; do :; done; exit 0",
        true,
        true,
    );
    let input = fixture.stdin.take().unwrap();
    let output = BufReader::new(fixture.stdout.take().unwrap());
    let mut client = Mcp {
        child: fixture,
        input,
        output,
        sequence: 0,
        native: "synthetic".into(),
        poisoned: false,
    };
    let first = client.exchange_result("fixture-error", json!({})).await;
    let second = client.exchange_result("fixture-success", json!({})).await;
    let poisoned = client.poisoned;
    let finished = client.finish_result().await;
    assert_eq!(finished, Ok(()));
    assert!(!poisoned);
    assert_eq!(first.unwrap()["error"]["code"], -32601);
    assert_eq!(second.unwrap()["result"], json!({}));
}
#[tokio::test]
async fn failed_exchange_poisoned_connection_cannot_retry() {
    let mut fixture = child(
        "IFS= read -r first; printf 'invalid\\n'; exec /bin/sleep 60",
        true,
        true,
    );
    let input = fixture.stdin.take().unwrap();
    let output = BufReader::new(fixture.stdout.take().unwrap());
    let mut client = Mcp {
        child: fixture,
        input,
        output,
        sequence: 0,
        native: "synthetic".into(),
        poisoned: false,
    };
    let first = client.exchange_result("fixture", json!({})).await;
    let retry = client.exchange_result("fixture", json!({})).await;
    let cleanup = kill_and_reap(&mut client.child).await;
    let exited = client.child.try_wait();
    assert_eq!(first.unwrap_err(), "MCP response JSON");
    assert_eq!(retry.unwrap_err(), "MCP connection poisoned");
    assert_eq!(cleanup, Ok(()));
    assert!(exited.unwrap().is_some());
}
#[tokio::test]
async fn daemon_readiness_failures_reap_retained_child_and_keep_unknown_file() {
    for (body, bad_file, expected) in [
        ("exit 7", false, "owned daemon exited during startup"),
        ("exec /bin/sleep 60", false, "daemon readiness deadline"),
        (
            "exec /bin/sleep 60",
            true,
            "daemon readiness path is not a socket",
        ),
    ] {
        let temp = private_temp();
        let root = temp.path().canonicalize().unwrap();
        let socket = root.join("fixture.sock");
        if bad_file {
            fs::write(&socket, b"unknown fixture").unwrap();
        }
        let metadata = fs::symlink_metadata(&root).unwrap();
        let mut fixture = child(body, false, false);
        let initial_exit = if body == "exit 7" {
            Some(tokio::time::timeout(Duration::from_secs(2), fixture.wait()).await)
        } else {
            None
        };
        let result = Daemon::startup_owned(
            &mut fixture,
            &socket,
            &root,
            (metadata.dev(), metadata.ino()),
            Duration::from_millis(20),
        )
        .await;
        let reaped_at_return = fixture.try_wait();
        let cleanup = kill_and_reap(&mut fixture).await;
        let unknown = if bad_file {
            Some(fs::read(&socket))
        } else {
            None
        };
        assert_eq!(cleanup, Ok(()));
        assert!(reaped_at_return.unwrap().is_some());
        if let Some(exit) = initial_exit {
            assert!(exit.unwrap().is_ok());
        }
        assert_eq!(result.unwrap_err(), expected);
        if let Some(contents) = unknown {
            assert_eq!(contents.unwrap(), b"unknown fixture");
        }
    }
}
#[tokio::test]
async fn daemon_factory_preserves_preexisting_socket_and_stderr_files() {
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("old.sock");
    fs::write(&socket, b"old socket fixture").unwrap();
    let result =
        Daemon::start_configured_result(Path::new("/bin/sh"), "synthetic", socket.clone(), None)
            .await;
    assert_eq!(
        result.err().unwrap(),
        "daemon socket must be absent before spawn"
    );
    assert_eq!(fs::read(&socket).unwrap(), b"old socket fixture");
    let socket = root.join("new.sock");
    fs::write(socket.with_extension("stderr"), b"old stderr fixture").unwrap();
    let result =
        Daemon::start_configured_result(Path::new("/bin/sh"), "synthetic", socket.clone(), None)
            .await;
    assert_eq!(result.err().unwrap(), "daemon stderr create_new");
    assert_eq!(
        fs::read(socket.with_extension("stderr")).unwrap(),
        b"old stderr fixture"
    );
    assert_eq!(
        fs::symlink_metadata(&root).unwrap().uid(),
        rustix::process::geteuid().as_raw()
    );
}
#[tokio::test]
async fn daemon_success_captures_private_socket_and_unlinks_after_reaped_exit() {
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("synthetic.sock");
    // Controller supplies the synthetic Unix socket; not a real daemon body.
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let metadata = fs::symlink_metadata(&root).unwrap();
    let observed = fs::symlink_metadata(&socket).unwrap();
    let mut fixture = child("exec /bin/sleep 60", false, false);
    let ready = Daemon::startup_owned(
        &mut fixture,
        &socket,
        &root,
        (metadata.dev(), metadata.ino()),
        Duration::from_millis(20),
    )
    .await;
    let live_at_ready = fixture.try_wait();
    let cleanup = kill_and_reap(&mut fixture).await;
    let reaped = fixture.try_wait();
    let unlink = match &ready {
        Ok(inode) => {
            let mut daemon = Daemon {
                child: fixture,
                socket: socket.clone(),
                inode: *inode,
            };
            daemon.unlink_after_stop()
        }
        Err(_) => Err("fixture startup failed".into()),
    };
    let absent = !socket.exists();
    drop(listener);
    assert_eq!(cleanup, Ok(()));
    assert!(reaped.unwrap().is_some());
    assert!(live_at_ready.unwrap().is_none());
    assert_eq!(ready.unwrap(), (observed.dev(), observed.ino()));
    assert_eq!(observed.permissions().mode() & 0o777, 0o600);
    assert_eq!(observed.uid(), rustix::process::geteuid().as_raw());
    assert_eq!(unlink, Ok(()));
    assert!(absent);
}
#[tokio::test]
async fn daemon_bad_socket_mode_is_known_then_reaped_and_replacement_is_retained() {
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let metadata = fs::symlink_metadata(&root).unwrap();
    let socket = root.join("bad-mode.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o644)).unwrap();
    let mut fixture = child("exec /bin/sleep 60", false, false);
    let failed = Daemon::startup_owned(
        &mut fixture,
        &socket,
        &root,
        (metadata.dev(), metadata.ino()),
        Duration::from_millis(20),
    )
    .await;
    let reaped_at_return = fixture.try_wait();
    let cleanup = kill_and_reap(&mut fixture).await;
    let known_removed = !socket.exists();
    drop(listener);
    assert_eq!(cleanup, Ok(()));
    assert!(reaped_at_return.unwrap().is_some());
    assert_eq!(failed.unwrap_err(), "daemon readiness deadline");
    assert!(known_removed);

    let socket = root.join("replace.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let mut fixture = child("exec /bin/sleep 60", false, false);
    let ready = Daemon::startup_owned(
        &mut fixture,
        &socket,
        &root,
        (metadata.dev(), metadata.ino()),
        Duration::from_millis(20),
    )
    .await;
    let cleanup = kill_and_reap(&mut fixture).await;
    let reaped = fixture.try_wait();
    let result = match ready {
        Ok(inode) => {
            fs::remove_file(&socket).unwrap();
            let replacement = UnixListener::bind(&socket).unwrap();
            let replacement_metadata = fs::symlink_metadata(&socket).unwrap();
            let mut daemon = Daemon {
                child: fixture,
                socket: socket.clone(),
                inode,
            };
            let unlink = daemon.unlink_after_stop();
            let retained = socket.exists() && replacement_metadata.file_type().is_socket();
            let different = (replacement_metadata.dev(), replacement_metadata.ino()) != inode;
            drop(replacement);
            Ok((unlink, retained, different))
        }
        Err(error) => Err(error),
    };
    drop(listener);
    assert_eq!(cleanup, Ok(()));
    assert!(reaped.unwrap().is_some());
    let (unlink, retained, different) = result.unwrap();
    assert_eq!(
        unlink.unwrap_err(),
        "owned socket was replaced; left untouched"
    );
    assert!(retained && different);
}
struct InitInput {
    stage: u8,
    writes: usize,
}
impl tokio::io::AsyncWrite for InitInput {
    fn poll_write(
        mut self: Pin<&mut Self>,
        _: &mut Context<'_>,
        bytes: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        if self.stage == 0 || (self.stage == 3 && self.writes > 0) {
            return Poll::Pending;
        }
        self.writes += 1;
        Poll::Ready(Ok(bytes.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.stage == 1 {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Ready(Ok(()))
    }
}
#[tokio::test]
async fn initialize_whole_deadline_reaps_for_pending_write_flush_read_and_notification() {
    for stage in 0..=3 {
        let mut fixture = child("exec /bin/sleep 60", false, false);
        let input = InitInput { stage, writes: 0 };
        let (reader, mut peer) = tokio::io::duplex(1024);
        if stage == 3 {
            peer.write_all(
                b"{\"jsonrpc\":\"2.0\",\"id\":1,\"result\":{\"protocolVersion\":\"2025-06-18\"}}\n",
            )
            .await
            .unwrap();
        }
        let output = BufReader::new(reader);
        let result = Mcp::setup_pipes(
            &mut fixture,
            Some(input),
            Some(output),
            Duration::from_millis(20),
        )
        .await;
        let reaped_at_return = fixture.try_wait();
        let cleanup = kill_and_reap(&mut fixture).await;
        drop(peer);
        assert_eq!(cleanup, Ok(()));
        assert!(reaped_at_return.unwrap().is_some());
        assert_eq!(result.err().unwrap(), "MCP initialization deadline");
    }
}
#[tokio::test]
async fn explicit_child_env_arrives_and_configure_removes_fake_ambient() {
    let mut daemon = Command::new("/bin/sh");
    daemon.env("JEV_FAKE_AMBIENT", "sentinel");
    daemon::Daemon::configure(
        &mut daemon,
        "synthetic-db",
        Path::new("synthetic.sock"),
        None,
    );
    daemon.arg("-c").arg("test -z \"${JEV_FAKE_AMBIENT+x}\" && test \"$TECT_DATABASE_URL\" = synthetic-db && test \"$TECT_SOCKET\" = synthetic.sock && test \"$PATH\" = /usr/bin:/bin && test -z \"${TECT_KNOWLEDGE_MAINTENANCE+x}\"");
    let mut owned = daemon.spawn().unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(2), owned.wait()).await;
    let cleanup = kill_and_reap(&mut owned).await;
    assert_eq!(cleanup, Ok(()));
    assert!(exit.unwrap().unwrap().success());
    let mut maintenance = Command::new("/bin/sh");
    maintenance.env("TECT_KNOWLEDGE_MAINTENANCE", "ambient-wrong");
    daemon::Daemon::configure(
        &mut maintenance,
        "synthetic-db",
        Path::new("synthetic.sock"),
        Some(Path::new("synthetic-contexts")),
    );
    maintenance.arg("-c").arg("test \"$TECT_KNOWLEDGE_MAINTENANCE\" = 1 && test \"$TECT_KNOWLEDGE_SEARCH_CONTEXTS\" = synthetic-contexts");
    let mut owned = maintenance.spawn().unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(2), owned.wait()).await;
    let cleanup = kill_and_reap(&mut owned).await;
    assert_eq!(cleanup, Ok(()));
    assert!(exit.unwrap().unwrap().success());
    let mut mcp = Command::new("/bin/sh");
    mcp.env("JEV_FAKE_AMBIENT", "sentinel");
    mcp::Mcp::configure(
        &mut mcp,
        Path::new("synthetic.sock"),
        Path::new("synthetic.json"),
        "synthetic-key",
    );
    mcp.arg("-c").arg("test -z \"${JEV_FAKE_AMBIENT+x}\" && test \"$TECT_SOCKET\" = synthetic.sock && test \"$TECT_HOST_CONFIG\" = synthetic.json && test \"$TECT_WORKSPACE_KEY\" = synthetic-key");
    let mut owned = mcp.spawn().unwrap();
    let exit = tokio::time::timeout(Duration::from_secs(2), owned.wait()).await;
    let cleanup = kill_and_reap(&mut owned).await;
    assert_eq!(cleanup, Ok(()));
    assert!(exit.unwrap().unwrap().success());
}

#[path = "public_timeout_tests.rs"]
mod public_timeout_tests;

#[path = "daemon_readiness_tests.rs"]
mod daemon_readiness_tests;
