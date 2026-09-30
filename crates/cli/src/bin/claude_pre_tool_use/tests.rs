use super::*;
use std::os::unix::fs::{PermissionsExt, symlink};

fn fixture() -> (tempfile::TempDir, Options, ClaudeToolAttestation) {
    let directory = tempfile::tempdir().unwrap();
    std::fs::set_permissions(directory.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
    let options = Options {
        destination_host_id: Uuid::new_v4(),
        server_alias: "tectd".into(),
        context_dir: directory.path().canonicalize().unwrap(),
    };
    let record = parse(
        &serde_json::to_vec(&serde_json::json!({
            "hook_event_name":"PreToolUse", "session_id":"ea29bc62-e367-4c25-ab8c-a03b2ee642cd",
            "tool_use_id":"call/opaque", "tool_name":"mcp__tectd__get_state", "tool_input":{},
            "cwd":"/tmp", "transcript_path":"/private/transcript", "permission_mode":"default"
        }))
        .unwrap(),
        &options,
        1000,
    )
    .unwrap();
    (directory, options, record)
}

#[test]
fn original_official_fields_and_opaque_hash_are_preserved() {
    let (_directory, options, record) = fixture();
    assert_eq!(record.tool_use_id, "call/opaque");
    assert_eq!(
        record.native_session_id,
        "ea29bc62-e367-4c25-ab8c-a03b2ee642cd"
    );
    let name = claude_attestation_filename(&record.tool_use_id).unwrap();
    assert_eq!(name.len(), 69);
    assert!(!name.contains('/'));
    publish(&options, &record, 1000).unwrap();
    let fd = open_claude_attestation_directory(&options.context_dir).unwrap();
    assert_eq!(read_existing(&fd, &name).unwrap().unwrap(), record);
    assert_eq!(std::fs::read_dir(&options.context_dir).unwrap().count(), 1);
}

#[test]
fn identical_retry_is_immutable_and_conflict_or_expiry_fails() {
    let (_directory, options, record) = fixture();
    publish(&options, &record, 1000).unwrap();
    let path = options
        .context_dir
        .join(claude_attestation_filename(&record.tool_use_id).unwrap());
    let original = std::fs::read(&path).unwrap();
    let mut retry = record.clone();
    retry.issued_at_unix_ms += 100;
    retry.expires_at_unix_ms += 100;
    publish(&options, &retry, 1100).unwrap();
    assert_eq!(std::fs::read(&path).unwrap(), original);
    retry.tool_input = serde_json::json!({"changed":true});
    assert!(publish(&options, &retry, 1100).is_err());
    assert!(publish(&options, &record, record.expires_at_unix_ms).is_err());
    assert_eq!(std::fs::read(&path).unwrap(), original);
}

#[test]
fn malformed_symlink_and_hardlinked_records_are_rejected() {
    let (_directory, options, record) = fixture();
    let name = claude_attestation_filename(&record.tool_use_id).unwrap();
    let target = options.context_dir.join("target");
    std::fs::write(&target, b"malformed").unwrap();
    std::fs::set_permissions(&target, std::fs::Permissions::from_mode(0o600)).unwrap();
    let path = options.context_dir.join(&name);
    symlink(&target, &path).unwrap();
    assert!(publish(&options, &record, 1000).is_err());
    std::fs::remove_file(&path).unwrap();
    std::fs::hard_link(&target, &path).unwrap();
    assert!(publish(&options, &record, 1000).is_err());
    std::fs::remove_file(&target).unwrap();
    assert!(publish(&options, &record, 1000).is_err());
}

#[test]
fn event_nil_session_noncanonical_identity_and_bounded_input_fail() {
    let (_directory, options, record) = fixture();
    let mut input = serde_json::json!({"hook_event_name":"PreToolUse", "session_id":record.native_session_id,
        "tool_use_id":record.tool_use_id,"tool_name":record.tool_name,"tool_input":{}});
    input["session_id"] = Value::String(Uuid::nil().to_string());
    assert!(parse(&serde_json::to_vec(&input).unwrap(), &options, 1000).is_err());
    input["session_id"] = Value::String("EA29BC62-E367-4C25-AB8C-A03B2EE642CD".into());
    assert!(parse(&serde_json::to_vec(&input).unwrap(), &options, 1000).is_err());
    input["session_id"] = Value::String(record.native_session_id);
    input["hook_event_name"] = Value::String("PostToolUse".into());
    assert!(parse(&serde_json::to_vec(&input).unwrap(), &options, 1000).is_err());
    assert!(
        parse(
            &vec![b' '; CLAUDE_ATTESTATION_MAX_BYTES + 1],
            &options,
            1000
        )
        .is_err()
    );
}

#[test]
fn unsafe_directory_and_oversized_serialization_fail() {
    let (_directory, mut options, mut record) = fixture();
    record.tool_input = serde_json::json!({"large":"x".repeat(CLAUDE_ATTESTATION_MAX_BYTES)});
    assert!(publish(&options, &record, 1000).is_err());
    options.context_dir = options.context_dir.join("..");
    assert!(publish(&options, &record, 1000).is_err());
}
