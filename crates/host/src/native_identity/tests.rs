use super::*;
use crate::context::HostContext;
use serde_json::json;
use std::fs;
use std::os::unix::fs::{PermissionsExt, symlink};
use tect_domain::HostAuth;

const NOW: u64 = 1_700_000_000_000;
fn host_id() -> Uuid {
    Uuid::from_u128(1)
}
fn native_id() -> String {
    Uuid::from_u128(2).to_string()
}
fn record(call: &str, input: Value) -> ClaudeToolAttestation {
    ClaudeToolAttestation::new(
        native_id(),
        call.into(),
        "mcp__tectd__get_state".into(),
        input,
        "tectd".into(),
        host_id(),
        NOW,
        NOW + 30_000,
    )
    .unwrap()
}
fn holder() -> (tempfile::TempDir, PathBuf) {
    let temp = tempfile::tempdir().unwrap();
    let root = temp.path().canonicalize().unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o700)).unwrap();
    (temp, root)
}
fn save(root: &std::path::Path, record: &ClaudeToolAttestation) {
    let path = root.join(claude_attestation_filename(&record.tool_use_id).unwrap());
    fs::write(&path, serde_json::to_vec(record).unwrap()).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
}
fn provider(root: &std::path::Path) -> NativeIdentityProvider {
    NativeIdentityProvider::ClaudePreToolUse(
        ClaudeIdentityConfig::new(root.to_owned(), "tectd".into(), host_id()).unwrap(),
    )
}
fn metadata(call: &str) -> Value {
    json!({"claudecode/toolUseId":call})
}
fn resolve(p: &NativeIdentityProvider, meta: &Value, input: &Value, now: u64) -> Result<String> {
    p.resolve_at(meta.as_object(), "get_state", input, true, now)
}

#[test]
fn default_codex_metadata_remains_required_and_never_reads_claude_records() {
    let p = NativeIdentityProvider::default();
    let meta = json!({"threadId":native_id(),"claudecode/toolUseId":"no-record"});
    assert_eq!(resolve(&p, &meta, &json!({}), NOW).unwrap(), native_id());
    for meta in [
        json!({}),
        json!({"claudecode/toolUseId":"call"}),
        json!({"threadId":null}),
        json!({"threadId":Uuid::nil()}),
    ] {
        assert_eq!(
            resolve(&p, &meta, &json!({}), NOW),
            Err(Error::InvalidNativeSession)
        );
    }
    let meta = json!({"threadId":"00000000-0000-0000-0000-00000000000A"});
    assert_eq!(
        resolve(&p, &meta, &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
}

#[test]
fn valid_claude_record_and_matching_thread_id_resolve_original_native_identity() {
    let (_temp, root) = holder();
    let r = record("call-1", json!({}));
    save(&root, &r);
    let p = provider(&root);
    assert_eq!(
        resolve(&p, &metadata("call-1"), &json!({}), NOW).unwrap(),
        native_id()
    );
    let meta = json!({"claudecode/toolUseId":"call-1","threadId":native_id()});
    assert_eq!(resolve(&p, &meta, &json!({}), NOW).unwrap(), native_id());
    for meta in [
        json!({"threadId":native_id()}),
        json!({"claudecode/toolUseId":"call-1","threadId":Uuid::from_u128(3)}),
        json!({"claudecode/toolUseId":"call-1","threadId":null}),
    ] {
        assert_eq!(
            resolve(&p, &meta, &json!({}), NOW),
            Err(Error::InvalidNativeSession)
        );
    }
}

#[test]
fn missing_malformed_expired_future_and_conflicting_records_fail_closed() {
    let (_temp, root) = holder();
    let p = provider(&root);
    assert_eq!(
        resolve(&p, &metadata("missing"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
    let r = record("call", json!({}));
    let mut cases = Vec::new();
    let mut v = r.clone();
    v.native_session_id = Uuid::nil().to_string();
    cases.push(v);
    let mut v = r.clone();
    v.native_session_id = "00000000-0000-0000-0000-00000000000A".into();
    cases.push(v);
    let mut v = r.clone();
    v.issued_at_unix_ms = NOW + 1;
    cases.push(v);
    let mut v = r.clone();
    v.expires_at_unix_ms = NOW;
    cases.push(v);
    let mut v = r.clone();
    v.expires_at_unix_ms = NOW + CLAUDE_ATTESTATION_MAX_TTL_MS + 1;
    cases.push(v);
    let mut v = r.clone();
    v.destination = "other-daemon".into();
    cases.push(v);
    let mut v = r.clone();
    v.destination_host_id = Uuid::from_u128(9);
    cases.push(v);
    let mut v = r.clone();
    v.server_alias = "other".into();
    cases.push(v);
    let mut v = r.clone();
    v.tool_name = "mcp__tectd__help".into();
    cases.push(v);
    let mut v = r.clone();
    v.tool_name = "mcp__other__get_state".into();
    cases.push(v);
    let mut v = r.clone();
    v.tool_input = Value::Null;
    cases.push(v);
    let mut v = r.clone();
    v.format_version = 2;
    cases.push(v);
    for v in cases {
        save(&root, &v);
        assert_eq!(
            resolve(&p, &metadata("call"), &json!({}), NOW),
            Err(Error::InvalidNativeSession)
        );
    }
    let path = root.join(claude_attestation_filename("call").unwrap());
    let mut changed = r.clone();
    changed.tool_use_id = "other-call".into();
    fs::write(&path, serde_json::to_vec(&changed).unwrap()).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
    let mut unknown = serde_json::to_value(&r).unwrap();
    unknown["unexpected"] = json!(true);
    fs::write(&path, serde_json::to_vec(&unknown).unwrap()).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
    let valid_json = serde_json::to_string(&r).unwrap();
    let duplicate = format!("{{\"tool_use_id\":\"call\",{}", &valid_json[1..]);
    fs::write(&path, duplicate).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
    for raw in [
        b"not-json".as_slice(),
        b"{\"format_version\":1,\"format_version\":1}".as_slice(),
    ] {
        fs::write(&path, raw).unwrap();
        assert_eq!(
            resolve(&p, &metadata("call"), &json!({}), NOW),
            Err(Error::InvalidNativeSession)
        );
    }
}

#[test]
fn structural_binding_preserves_omitted_null_and_numeric_representations() {
    let (_temp, root) = holder();
    let p = provider(&root);
    let input = json!({"number":1,"optional":null,"array":[1,2]});
    save(&root, &record("call", input.clone()));
    assert!(resolve(&p, &metadata("call"), &input, NOW).is_ok());
    for different in [
        json!({"number":1,"array":[1,2]}),
        json!({"number":1.0,"optional":null,"array":[1,2]}),
        json!({"number":1,"optional":null,"array":[2,1]}),
        Value::Null,
    ] {
        assert_eq!(
            resolve(&p, &metadata("call"), &different, NOW),
            Err(Error::InvalidNativeSession)
        );
    }
    assert_eq!(
        p.resolve_at(
            metadata("call").as_object(),
            "get_state",
            &input,
            false,
            NOW
        ),
        Err(Error::InvalidNativeSession)
    );
}

#[test]
fn private_regular_same_owner_single_link_files_and_holders_are_required() {
    let (_temp, root) = holder();
    let r = record("call", json!({}));
    save(&root, &r);
    let p = provider(&root);
    let path = root.join(claude_attestation_filename("call").unwrap());
    for mode in [0o640, 0o644, 0o660] {
        fs::set_permissions(&path, fs::Permissions::from_mode(mode)).unwrap();
        assert_eq!(
            resolve(&p, &metadata("call"), &json!({}), NOW),
            Err(Error::InvalidNativeSession)
        );
    }
    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
    let directory_stat = rustix::fs::stat(&root).unwrap();
    assert_eq!(
        secure_file::validate_directory(&directory_stat, directory_stat.st_uid.wrapping_add(1)),
        Err(Error::InvalidNativeSession)
    );
    let stat = rustix::fs::stat(&path).unwrap();
    assert_eq!(
        secure_file::validate_file(&stat, stat.st_uid.wrapping_add(1)),
        Err(Error::InvalidNativeSession)
    );
    let link = root.join("hard-link");
    fs::hard_link(&path, &link).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
    fs::remove_file(link).unwrap();
    fs::set_permissions(&root, fs::Permissions::from_mode(0o750)).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
}

#[test]
fn ancestor_traversal_keeps_final_holder_readable_and_fsync_capable() {
    let (_temp, root) = holder();
    let ancestor = root.join("ancestor");
    let final_holder = ancestor.join("private");
    fs::create_dir(&ancestor).unwrap();
    fs::create_dir(&final_holder).unwrap();
    fs::set_permissions(&final_holder, fs::Permissions::from_mode(0o700)).unwrap();
    let fd = open_claude_attestation_directory(&final_holder).unwrap();
    let stat = rustix::fs::fstat(&fd).unwrap();
    assert!(rustix::fs::FileType::from_raw_mode(stat.st_mode).is_dir());
    assert_eq!(stat.st_mode & 0o7777, 0o700);
    // Writer publication needs a real readable directory fd, not O_PATH/O_SEARCH.
    rustix::fs::fsync(&fd).unwrap();
    save(&final_holder, &record("nested", json!({})));
    assert_eq!(
        resolve(
            &provider(&final_holder),
            &metadata("nested"),
            &json!({}),
            NOW
        ),
        Ok(native_id())
    );
    let file = root.join("regular");
    fs::write(&file, b"regular file").unwrap();
    fs::set_permissions(&file, fs::Permissions::from_mode(0o700)).unwrap();
    assert!(open_claude_attestation_directory(&file).is_err());
    assert!(open_claude_attestation_directory(&file.join("private")).is_err());
    let ancestor_alias = root.join("ancestor-alias");
    symlink(&ancestor, &ancestor_alias).unwrap();
    assert!(open_claude_attestation_directory(&ancestor_alias.join("private")).is_err());
    let final_alias = ancestor.join("private-alias");
    symlink(&final_holder, &final_alias).unwrap();
    assert!(open_claude_attestation_directory(&final_alias).is_err());
}

#[test]
fn record_and_ancestor_symlinks_and_oversized_files_are_rejected() {
    let (_temp, root) = holder();
    let r = record("call", json!({}));
    save(&root, &r);
    let p = provider(&root);
    let path = root.join(claude_attestation_filename("call").unwrap());
    let target = root.join("target");
    fs::rename(&path, &target).unwrap();
    symlink(&target, &path).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
    fs::remove_file(&path).unwrap();
    fs::rename(&target, &path).unwrap();
    let alias = root.join("alias");
    symlink(&root, &alias).unwrap();
    assert!(ClaudeIdentityConfig::new(alias, "tectd".into(), host_id()).is_err());
    let alias = root.join("parent-alias");
    let child = root.join("child");
    fs::create_dir(&child).unwrap();
    fs::set_permissions(&child, fs::Permissions::from_mode(0o700)).unwrap();
    symlink(&root, &alias).unwrap();
    assert!(ClaudeIdentityConfig::new(alias.join("child"), "tectd".into(), host_id()).is_err());
    fs::write(&path, vec![b'x'; CLAUDE_ATTESTATION_MAX_BYTES + 1]).unwrap();
    assert_eq!(
        resolve(&p, &metadata("call"), &json!({}), NOW),
        Err(Error::InvalidNativeSession)
    );
}

#[test]
fn opaque_ids_are_hashed_exactly_and_separate_calls_keep_identity_and_expiry() {
    let (_temp, root) = holder();
    let p = provider(&root);
    let first = record("../opaque/one", json!({"call":1}));
    let mut second = record("opaque/two", json!({"call":2}));
    second.native_session_id = Uuid::from_u128(7).to_string();
    save(&root, &first);
    save(&root, &second);
    assert_ne!(
        claude_attestation_filename(&first.tool_use_id).unwrap(),
        claude_attestation_filename(&second.tool_use_id).unwrap()
    );
    let name = claude_attestation_filename(&first.tool_use_id).unwrap();
    assert_eq!(name.len(), 69);
    assert!(!name.contains('/'));
    std::thread::scope(|scope| {
        let p1 = &p;
        let first = &first;
        let a = scope.spawn(move || {
            resolve(p1, &metadata(&first.tool_use_id), &first.tool_input, NOW).unwrap()
        });
        let p2 = &p;
        let second = &second;
        let b = scope.spawn(move || {
            resolve(p2, &metadata(&second.tool_use_id), &second.tool_input, NOW).unwrap()
        });
        assert_eq!(a.join().unwrap(), first.native_session_id);
        assert_eq!(b.join().unwrap(), second.native_session_id);
    });
    for _ in 0..2 {
        assert_eq!(
            resolve(
                &p,
                &metadata(&first.tool_use_id),
                &first.tool_input,
                NOW + 1
            )
            .unwrap(),
            first.native_session_id
        );
    }
    assert_eq!(
        resolve(
            &p,
            &metadata(&first.tool_use_id),
            &first.tool_input,
            first.expires_at_unix_ms
        ),
        Err(Error::InvalidNativeSession)
    );
    let bytes = secure_file::read_record(&root, &first.tool_use_id).unwrap();
    let retained: ClaudeToolAttestation = serde_json::from_slice(&bytes).unwrap();
    assert_eq!(retained, first); // resolution never renews/replaces an attestation
}

#[test]
fn configured_destination_is_bound_to_current_host_auth() {
    let (_temp, root) = holder();
    let host = HostContext::new(
        HostAuth {
            host_id: host_id(),
            credential: "0".repeat(64),
        },
        "test-workspace".into(),
    )
    .unwrap();
    let config = ClaudeIdentityConfig::new(root, "tectd".into(), Uuid::from_u128(9)).unwrap();
    assert!(matches!(
        host.with_claude_identity(config),
        Err(Error::InvalidConfiguration)
    ));
}
