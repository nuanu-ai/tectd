use std::io::Write;
use std::os::unix::fs::PermissionsExt;
use std::process::{Command, Stdio};

#[test]
fn native_hook_success_has_empty_stdout_and_invalid_event_blocks() {
    let holder = tempfile::tempdir().unwrap();
    std::fs::set_permissions(holder.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let canonical = holder.path().canonicalize().unwrap();
    let event = serde_json::json!({
        "hook_event_name":"PreToolUse", "session_id":"ea29bc62-e367-4c25-ab8c-a03b2ee642cd",
        "tool_use_id":"native-call-1", "tool_name":"mcp__tectd__get_state", "tool_input":{},
        "cwd":"/tmp", "transcript_path":"/private/transcript", "permission_mode":"default"
    });
    let invoke = |input: &[u8]| {
        let mut child = Command::new(env!("CARGO_BIN_EXE_tectd-mcp"))
            .args([
                "claude-pre-tool-use",
                "--destination-host-id",
                "550e8400-e29b-41d4-a716-446655440000",
                "--server-alias",
                "tectd",
                "--context-dir",
            ])
            .arg(&canonical)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .env_remove("TECT_SOCKET")
            .env_remove("TECT_HOST_CONFIG")
            .spawn()
            .unwrap();
        child.stdin.take().unwrap().write_all(input).unwrap();
        child.wait_with_output().unwrap()
    };
    let output = invoke(&serde_json::to_vec(&event).unwrap());
    assert!(output.status.success());
    assert!(output.stdout.is_empty());
    assert!(output.stderr.is_empty());
    let output = invoke(b"{invalid native payload and private data}");
    assert_eq!(output.status.code(), Some(2));
    assert!(output.stdout.is_empty());
    assert!(!String::from_utf8_lossy(&output.stderr).contains("private data"));
    assert!(output.stderr.len() < 100);
}
