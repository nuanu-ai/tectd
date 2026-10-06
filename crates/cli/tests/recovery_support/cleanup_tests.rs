use super::*;
use std::os::unix::net::UnixListener;
use std::time::Instant;
use std::{os::unix::fs::MetadataExt, process::Stdio};
use tokio::{io::BufReader, process::Command};

// Controlled fixtures use shell builtins or exec one sleep;
// they do not fork descendants, run Tect, load host auth, or contact a database.
fn child(script: &str, piped: bool) -> Result<Child, String> {
    Command::new("/bin/sh")
        .arg("-c")
        .arg(script)
        .env_clear()
        .stdin(if piped { Stdio::piped() } else { Stdio::null() })
        .stdout(if piped { Stdio::piped() } else { Stdio::null() })
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .map_err(|error| format!("owned fixture spawn: {error}"))
}
async fn mcp(script: &str) -> Result<Mcp, String> {
    let mut child = child(script, true)?;
    let input = child.stdin.take();
    let output = child.stdout.take();
    match (input, output) {
        (Some(input), Some(output)) => Ok(Mcp {
            input,
            output: BufReader::new(output),
            child,
            sequence: 0,
            native: "offline-cleanup-fixture".into(),
            poisoned: false,
        }),
        pipes => {
            drop(pipes);
            let cleanup = kill_and_reap(&mut child).await;
            Err(format!(
                "owned fixture pipe setup failed; cleanup: {cleanup:?}"
            ))
        }
    }
}
fn daemon(root: &Path, script: &str) -> (Daemon, UnixListener) {
    let socket = root.join("cleanup.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    let metadata = fs::symlink_metadata(&socket).unwrap();
    let daemon = Daemon {
        child: child(script, false).unwrap(),
        socket,
        inode: (metadata.dev(), metadata.ino()),
    };
    (daemon, listener)
}
async fn finite_exit(child: &mut Child) {
    let normal = tokio::time::timeout(Duration::from_secs(2), child.wait()).await;
    let cleanup = if matches!(&normal, Ok(Ok(_))) {
        Ok(())
    } else {
        kill_and_reap(child).await
    };
    assert!(
        matches!(normal, Ok(Ok(_))),
        "finite exit: {normal:?}; cleanup: {cleanup:?}"
    );
}

#[tokio::test]
async fn clean_eof_preserves_finite_unread_stdout_and_reaps() {
    // Child emits AFTER EOF: early stdout drop could produce nonzero SIGPIPE.
    let client = mcp(
        "while IFS= read -r line; do :; done; printf 'finite unread output\\n' || exit 23; exit 0",
    )
    .await
    .unwrap();
    let result = tokio::time::timeout(Duration::from_secs(11), client.finish_result())
        .await
        .unwrap();
    assert_eq!(result, Ok(()));
}
#[tokio::test]
async fn nonzero_eof_retains_original_failure() {
    let client = mcp("while IFS= read -r line; do :; done; exit 7")
        .await
        .unwrap();
    let error = tokio::time::timeout(Duration::from_secs(11), client.finish_result())
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        error.contains("MCP EOF exit was not successful") && error.contains('7'),
        "{error}"
    );
}
#[tokio::test]
async fn refuses_eof_is_bounded_and_fallback_does_not_promote_success() {
    let client = mcp("while IFS= read -r line; do :; done; exec /bin/sleep 60")
        .await
        .unwrap();
    let start = Instant::now();
    let error = tokio::time::timeout(Duration::from_secs(11), client.finish_result())
        .await
        .unwrap()
        .unwrap_err();
    assert!(
        error.contains("normal shutdown deadline exceeded"),
        "{error}"
    );
    assert!(!error.contains("fallback cleanup"), "{error}");
    assert!(start.elapsed() < Duration::from_secs(11));
    // Result proves bounded fallback wait returned; it is not signal delivery
    // or descendant absence proof. Original timeout remains failure.
}
#[tokio::test]
async fn mcp_failure_still_attempts_daemon_stop_and_owned_unlink() {
    let temp = private_temp();
    let (mut daemon, _listener) = daemon(temp.path(), "exec /bin/sleep 60");
    let setup = mcp("while IFS= read -r line; do :; done; exit 7").await;
    let mcp = match setup {
        Ok(client) => client.finish_result().await,
        Err(setup_error) => Err(setup_error),
    };
    let report = cleanup_after_mcp(mcp, &mut daemon).await;
    assert!(report.mcp.is_err());
    assert_eq!(report.daemon_already_exited, Ok(false));
    assert_eq!(report.socket, Ok(()));
    assert!(!report.normal_tail_succeeded());
    assert!(daemon.child.try_wait().unwrap().is_some());
    assert!(!daemon.socket.exists());
}
#[tokio::test]
async fn already_dead_api_is_explicit_and_normal_tail_rejects_it() {
    let temp = private_temp();
    let (mut daemon, _listener) = daemon(temp.path(), "exit 0");
    finite_exit(&mut daemon.child).await;
    let stopped = daemon.stop().await;
    let report = cleanup_after_mcp(Ok(()), &mut daemon).await;
    assert_eq!(stopped, Ok(true));
    assert_eq!(report.daemon_already_exited, Ok(true));
    assert_eq!(report.socket, Ok(()));
    assert!(!report.normal_tail_succeeded());
}
#[tokio::test]
async fn matching_socket_unlinks_only_after_confirmed_exit() {
    let temp = private_temp();
    let (mut daemon, _listener) = daemon(temp.path(), "exec /bin/sleep 60");
    let live_unlink = daemon.unlink_after_stop();
    let live_socket_exists = daemon.socket.exists();
    let stop = daemon.stop().await;
    let unlink = daemon.unlink_after_stop();
    let absent = daemon.unlink_after_stop();
    assert!(live_unlink.is_err());
    assert!(live_socket_exists);
    assert_eq!(stop, Ok(false));
    assert_eq!(unlink, Ok(()));
    assert_eq!(absent, Ok(())); // absent is harmless
}
#[tokio::test]
async fn replaced_socket_is_reported_and_preserved() {
    let temp = private_temp();
    let (mut daemon, listener) = daemon(temp.path(), "exec /bin/sleep 60");
    let stopped = daemon.stop().await;
    // If stop cannot confirm exit, attempt independent cleanup before asserting.
    let cleanup = kill_and_reap(&mut daemon.child).await;
    assert_eq!(stopped, Ok(false));
    assert_eq!(cleanup, Ok(()));
    fs::remove_file(&daemon.socket).unwrap();
    // Retain the original listener until replacement metadata is captured;
    // this prevents reuse of the original inode for the replacement.
    let replacement = UnixListener::bind(&daemon.socket).unwrap();
    let metadata = fs::symlink_metadata(&daemon.socket).unwrap();
    assert_ne!((metadata.dev(), metadata.ino()), daemon.inode);
    let error = daemon.unlink_after_stop().unwrap_err();
    assert!(error.contains("replaced"));
    assert!(daemon.socket.exists());
    drop(replacement);
    drop(listener);
}

use std::pin::Pin;
use std::sync::{
    Arc,
    atomic::{AtomicBool, Ordering},
};
use std::task::{Context, Poll};

struct PendingInput {
    pending_flush: bool,
    dropped: Arc<AtomicBool>,
}
impl Drop for PendingInput {
    fn drop(&mut self) {
        self.dropped.store(true, Ordering::SeqCst);
    }
}
impl tokio::io::AsyncWrite for PendingInput {
    fn poll_write(
        self: Pin<&mut Self>,
        _: &mut Context<'_>,
        data: &[u8],
    ) -> Poll<std::io::Result<usize>> {
        Poll::Ready(Ok(data.len()))
    }
    fn poll_flush(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        if self.pending_flush {
            Poll::Pending
        } else {
            Poll::Ready(Ok(()))
        }
    }
    fn poll_shutdown(self: Pin<&mut Self>, _: &mut Context<'_>) -> Poll<std::io::Result<()>> {
        Poll::Pending
    }
}
async fn pending_input_case(pending_flush: bool) {
    let dropped = Arc::new(AtomicBool::new(false));
    let input = PendingInput {
        pending_flush,
        dropped: Arc::clone(&dropped),
    };
    let mut owned = child("exec /bin/sleep 60", false).unwrap();
    let normal = normal_finish(input, &mut owned, Duration::from_millis(20)).await;
    // Cleanup precedes assertions: bounded normal cancellation must leave
    // this owned Child available for fallback, and must drop input.
    let cleanup = kill_and_reap(&mut owned).await;
    let error = normal.unwrap_err();
    assert!(error.contains("normal shutdown deadline exceeded"));
    assert!(dropped.load(Ordering::SeqCst));
    assert_eq!(cleanup, Ok(()));
    assert!(owned.try_wait().unwrap().is_some());
}
#[tokio::test]
async fn pending_flush_is_inside_whole_deadline_and_dropped() {
    pending_input_case(true).await;
}
#[tokio::test]
async fn pending_shutdown_is_inside_whole_deadline_and_dropped() {
    pending_input_case(false).await;
}
#[tokio::test]
async fn probe_error_still_kills_and_reaps_but_retains_first_failure() {
    let mut owned = child("exec /bin/sleep 60", false).unwrap();
    let probe = Err(std::io::Error::other("injected status observation failure"));
    let observed = kill_and_reap_with_probe(&mut owned, probe).await;
    let cleanup = kill_and_reap(&mut owned).await;
    let error = observed.unwrap_err();
    assert_eq!(cleanup, Ok(()));
    assert!(
        error.starts_with("owned child status: injected status observation failure"),
        "{error}"
    );
    assert!(owned.try_wait().unwrap().is_some());
}
