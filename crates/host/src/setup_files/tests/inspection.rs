use super::*;
use std::fs::{File, FileTimes};
use std::os::unix::fs::{PermissionsExt, symlink};
use std::os::unix::net::UnixListener;
use std::process::Command;
use std::time::SystemTime;
use tect_domain::{Error, SetupFileStatus};

#[test]
fn missing_existing_empty_and_non_utf8_are_observed_without_content() {
    let fixture = Fixture::new();
    assert_eq!(
        adapter().inspect(&fixture.directory, 128).unwrap().status,
        SetupFileStatus::Missing
    );

    let bytes = b"workspace instructions";
    fs::write(fixture.target(), bytes).unwrap();
    let observed = adapter().inspect(&fixture.directory, 128).unwrap();
    assert_eq!(observed.status, SetupFileStatus::Existing);
    assert_eq!(observed.byte_length, Some(bytes.len() as u64));
    assert_eq!(observed.sha256, Some(hash(bytes)));
    assert_eq!(observed.reason, None);

    fs::write(fixture.target(), []).unwrap();
    let empty = adapter().inspect(&fixture.directory, 0).unwrap();
    assert_eq!(empty.status, SetupFileStatus::Existing);
    assert_eq!(empty.byte_length, Some(0));
    assert_eq!(empty.sha256, Some(hash(&[])));

    fs::write(fixture.target(), [0xff, 0xfe, 0x00]).unwrap();
    let binary = adapter().inspect(&fixture.directory, 3).unwrap();
    assert_eq!(binary.status, SetupFileStatus::Existing);
    assert_eq!(binary.sha256, Some(hash(&[0xff, 0xfe, 0x00])));
}

#[test]
fn oversized_existing_file_stays_existing_without_hashing() {
    let fixture = Fixture::new();
    fs::write(fixture.target(), b"larger than comparison capacity").unwrap();
    let observed = adapter().inspect(&fixture.directory, 4).unwrap();
    assert_eq!(observed.status, SetupFileStatus::Existing);
    assert_eq!(observed.byte_length, Some(31));
    assert_eq!(observed.sha256, None);
    assert_eq!(
        observed.reason.as_deref(),
        Some("comparison_capacity_exceeded")
    );
}

#[test]
fn equal_length_in_place_change_during_inspection_is_unavailable() {
    let fixture = Fixture::new();
    let original = b"original";
    let replacement = b"replaced";
    fs::write(fixture.target(), original).unwrap();
    File::options()
        .write(true)
        .open(fixture.target())
        .unwrap()
        .set_times(FileTimes::new().set_modified(SystemTime::UNIX_EPOCH))
        .unwrap();

    let observed = unix::inspect_after_metadata(&fixture.directory, original.len(), || {
        fs::write(fixture.target(), replacement).unwrap();
    })
    .unwrap();

    assert_eq!(observed.status, SetupFileStatus::Unavailable);
    assert_eq!(
        observed.reason.as_deref(),
        Some("file_changed_during_inspection")
    );
    assert_eq!(fs::read(fixture.target()).unwrap(), replacement);
}

#[test]
fn symlink_target_and_symlink_ancestor_are_never_followed() {
    let fixture = Fixture::new();
    let foreign = fixture.root.join("foreign");
    fs::write(&foreign, b"foreign").unwrap();
    symlink(&foreign, fixture.target()).unwrap();
    let observed = adapter().inspect(&fixture.directory, 128).unwrap();
    assert_eq!(observed.status, SetupFileStatus::Unavailable);
    assert_eq!(
        adapter().publish(&fixture.directory, "replacement"),
        Err(Error::SetupFileConflict)
    );
    assert_eq!(fs::read(&foreign).unwrap(), b"foreign");

    let real = fixture.root.join("real");
    fs::create_dir(&real).unwrap();
    let alias = fixture.root.join("alias");
    symlink(&real, &alias).unwrap();
    assert_eq!(
        adapter().resolve_directory(path(&alias), &[path(&fixture.root).to_owned()]),
        Err(Error::SetupUnavailable)
    );
}

#[test]
fn directory_fifo_and_socket_targets_are_unavailable_and_preserved() {
    let fixture = Fixture::new();
    fs::create_dir(fixture.target()).unwrap();
    assert_eq!(
        adapter().inspect(&fixture.directory, 128).unwrap().status,
        SetupFileStatus::Unavailable
    );
    assert_eq!(
        adapter().publish(&fixture.directory, "x"),
        Err(Error::SetupFileConflict)
    );
    fs::remove_dir(fixture.target()).unwrap();

    assert!(
        Command::new("mkfifo")
            .arg(fixture.target())
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(
        adapter().inspect(&fixture.directory, 128).unwrap().status,
        SetupFileStatus::Unavailable
    );
    fs::remove_file(fixture.target()).unwrap();

    let listener = UnixListener::bind(fixture.target()).unwrap();
    assert_eq!(
        adapter().inspect(&fixture.directory, 128).unwrap().status,
        SetupFileStatus::Unavailable
    );
    drop(listener);
    fs::remove_file(fixture.target()).unwrap();
}

#[test]
fn denied_regular_file_is_unavailable_when_permissions_are_enforced() {
    let fixture = Fixture::new();
    fs::write(fixture.target(), b"private").unwrap();
    let original = fs::metadata(fixture.target()).unwrap().permissions();
    let mut denied = original.clone();
    denied.set_mode(0o0);
    fs::set_permissions(fixture.target(), denied).unwrap();
    let platform_denies = fs::File::open(fixture.target()).is_err();
    let observed = adapter().inspect(&fixture.directory, 128).unwrap();
    fs::set_permissions(fixture.target(), original).unwrap();
    if platform_denies {
        assert_eq!(observed.status, SetupFileStatus::Unavailable);
        assert_eq!(observed.reason.as_deref(), Some("access_denied"));
    }
}

#[test]
fn path_shape_and_grant_are_checked_before_resolution() {
    let fixture = Fixture::new();
    for invalid in [
        "",
        "relative",
        "/__tect_test__/./task",
        "/__tect_test__/../task",
        "/__tect_test__//task",
        "/__tect_test__/task/",
        "/__tect_test__/bad\0path",
    ] {
        assert_eq!(
            adapter().resolve_directory(invalid, &[path(&fixture.root).to_owned()]),
            Err(Error::InvalidArguments),
            "{invalid:?}"
        );
    }
    let oversized = format!("/{}", "a".repeat(4096));
    assert_eq!(
        adapter().resolve_directory(&oversized, &[path(&fixture.root).to_owned()]),
        Err(Error::InvalidArguments)
    );
    let outside = tempfile::tempdir().unwrap();
    let outside_path = outside.path().canonicalize().unwrap();
    assert_eq!(
        adapter().resolve_directory(path(&outside_path), &[path(&fixture.root).to_owned()]),
        Err(Error::SetupUnavailable)
    );
}

#[test]
fn replaced_directory_inode_never_rebinds() {
    let fixture = Fixture::new();
    let old = fixture.root.join("old-task");
    fs::rename(&fixture.task, &old).unwrap();
    fs::create_dir(&fixture.task).unwrap();
    assert_eq!(
        adapter().inspect(&fixture.directory, 128),
        Err(Error::TaskDirectoryMismatch)
    );
    assert_eq!(
        adapter().publish(&fixture.directory, "new"),
        Err(Error::TaskDirectoryMismatch)
    );
    assert!(!fixture.target().exists());
}
