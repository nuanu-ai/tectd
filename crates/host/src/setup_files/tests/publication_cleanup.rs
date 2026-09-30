use super::*;
use std::fs::{File, FileTimes};
use std::os::unix::fs::{MetadataExt, PermissionsExt, symlink};
use tect_domain::{Error, PublicationOutcome, SetupFileStatus};

const CONTENT: &str = "same content";

fn staged(fixture: &Fixture) -> PathBuf {
    let stage = fixture.task.join(".tectd-agents-owned.tmp");
    fs::write(&stage, CONTENT).unwrap();
    fs::set_permissions(&stage, fs::Permissions::from_mode(0o600)).unwrap();
    fs::hard_link(&stage, fixture.target()).unwrap();
    assert_eq!(fs::metadata(fixture.target()).unwrap().nlink(), 2);
    stage
}

#[test]
fn publication_rechecks_cleanup_once_while_ordinary_inspection_stays_strict() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let observed = unix::inspect_after_metadata(&fixture.directory, CONTENT.len(), || {
        fs::remove_file(&stage).unwrap();
    })
    .unwrap();
    assert_eq!(observed.status, SetupFileStatus::Unavailable);
    assert_eq!(
        observed.reason.as_deref(),
        Some("file_changed_during_inspection")
    );

    // Recreate the same publisher's stage link and force its cleanup while the
    // adopter has already opened and statted the complete published inode.
    fs::hard_link(fixture.target(), &stage).unwrap();
    let inode = fs::metadata(fixture.target()).unwrap().ino();
    let mut reads = 0;
    let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
        reads += 1;
        if reads == 1 {
            fs::remove_file(&stage).unwrap();
        }
    })
    .unwrap();
    assert_eq!(reads, 2, "one original read and exactly one fresh read");
    assert_eq!(result.outcome, PublicationOutcome::AlreadyMatches);
    assert_eq!(fs::metadata(fixture.target()).unwrap().ino(), inode);
    assert_eq!(fs::read_to_string(fixture.target()).unwrap(), CONTENT);
}

#[test]
fn cleanup_with_content_mutation_is_rejected_even_with_restored_mtime() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let modified = fs::metadata(fixture.target()).unwrap().modified().unwrap();
    let mut reads = 0;
    let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
        reads += 1;
        if reads == 1 {
            fs::write(fixture.target(), "bad! content").unwrap();
            File::options()
                .write(true)
                .open(fixture.target())
                .unwrap()
                .set_times(FileTimes::new().set_modified(modified))
                .unwrap();
            fs::remove_file(&stage).unwrap();
        }
    });
    assert_eq!(result, Err(Error::SetupFileConflict));
    assert!(reads <= 2);
    assert_eq!(
        fs::read_to_string(fixture.target()).unwrap(),
        "bad! content"
    );
}

#[test]
fn unsafe_initial_or_changed_mode_is_not_a_cleanup_retry_candidate() {
    for initial in [0o600, 0o640] {
        let fixture = Fixture::new();
        let stage = staged(&fixture);
        fs::set_permissions(fixture.target(), fs::Permissions::from_mode(initial)).unwrap();
        let mut reads = 0;
        let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
            reads += 1;
            if initial == 0o600 {
                fs::set_permissions(fixture.target(), fs::Permissions::from_mode(0o640)).unwrap();
            }
            fs::remove_file(&stage).unwrap();
        });
        assert_eq!(result, Err(Error::SetupFileConflict));
        assert_eq!(reads, 1);
    }
}

#[test]
fn cleanup_with_changed_mtime_or_extra_link_does_not_retry() {
    for extra_link in [false, true] {
        let fixture = Fixture::new();
        let stage = staged(&fixture);
        if extra_link {
            fs::hard_link(fixture.target(), fixture.task.join("foreign-link")).unwrap();
        }
        let mut reads = 0;
        let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
            reads += 1;
            if !extra_link {
                File::options()
                    .write(true)
                    .open(fixture.target())
                    .unwrap()
                    .set_times(FileTimes::new().set_modified(std::time::SystemTime::UNIX_EPOCH))
                    .unwrap();
            }
            fs::remove_file(&stage).unwrap();
        });
        assert_eq!(result, Err(Error::SetupFileConflict));
        assert_eq!(reads, 1);
    }
}

#[test]
fn same_bytes_replacement_or_symlink_during_retry_never_rebinds() {
    for link in [false, true] {
        let fixture = Fixture::new();
        let stage = staged(&fixture);
        let replacement = fixture.task.join("replacement");
        fs::write(&replacement, CONTENT).unwrap();
        fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
        let mut reads = 0;
        let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
            reads += 1;
            if reads == 1 {
                fs::remove_file(&stage).unwrap();
            } else {
                fs::remove_file(fixture.target()).unwrap();
                if link {
                    symlink(&replacement, fixture.target()).unwrap();
                } else {
                    fs::rename(&replacement, fixture.target()).unwrap();
                }
            }
        });
        assert_eq!(result, Err(Error::SetupFileConflict));
        assert_eq!(reads, 2);
    }
}

#[test]
fn retry_budget_is_exhausted_by_a_second_link_transition() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let mut reads = 0;
    let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
        reads += 1;
        if reads == 1 {
            fs::remove_file(&stage).unwrap();
        } else {
            fs::hard_link(fixture.target(), &stage).unwrap();
        }
    });
    assert_eq!(result, Err(Error::SetupFileConflict));
    assert_eq!(reads, 2);
    assert_eq!(fs::read_to_string(fixture.target()).unwrap(), CONTENT);
}

#[test]
fn replaced_directory_during_fresh_read_is_rejected() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let mut reads = 0;
    let result = unix::publish_after_metadata(&fixture.directory, CONTENT, || {
        reads += 1;
        if reads == 1 {
            fs::remove_file(&stage).unwrap();
        } else {
            fs::rename(&fixture.task, fixture.root.join("old-task")).unwrap();
            fs::create_dir(&fixture.task).unwrap();
        }
    });
    assert_eq!(result, Err(Error::TaskDirectoryMismatch));
    assert_eq!(reads, 2);
    assert!(!fixture.target().exists());
}

#[test]
fn publication_rechecks_cleanup_between_post_read_stat_and_named_stat() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let inode = fs::metadata(fixture.target()).unwrap().ino();
    let mut reads = 0;
    let result = unix::publish_after_file_metadata(&fixture.directory, CONTENT, || {
        reads += 1;
        if reads == 1 {
            fs::remove_file(&stage).unwrap();
        }
    })
    .unwrap();
    assert_eq!(reads, 2);
    assert_eq!(result.outcome, PublicationOutcome::AlreadyMatches);
    assert_eq!(fs::metadata(fixture.target()).unwrap().ino(), inode);
    assert_eq!(fs::read_to_string(fixture.target()).unwrap(), CONTENT);
}

#[test]
fn post_read_cleanup_with_same_bytes_inode_replacement_still_conflicts() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let replacement = fixture.task.join("replacement");
    fs::write(&replacement, CONTENT).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
    let mut reads = 0;
    let result = unix::publish_after_file_metadata(&fixture.directory, CONTENT, || {
        reads += 1;
        fs::remove_file(&stage).unwrap();
        fs::rename(&replacement, fixture.target()).unwrap();
    });
    assert_eq!(result, Err(Error::SetupFileConflict));
    assert_eq!(reads, 1);
}

#[test]
fn disappearance_before_fresh_open_conflicts_without_creating_an_inode() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let result = unix::publish_before_retry(
        &fixture.directory,
        CONTENT,
        || {
            fs::remove_file(&stage).unwrap();
        },
        || {
            fs::remove_file(fixture.target()).unwrap();
        },
    );
    assert_eq!(result, Err(Error::SetupFileConflict));
    assert!(!fixture.target().exists());
    assert!(stage_names(&fixture.task).is_empty());
}

#[test]
fn identical_replacement_before_fresh_open_is_not_adopted() {
    let fixture = Fixture::new();
    let stage = staged(&fixture);
    let replacement = fixture.task.join("replacement");
    fs::write(&replacement, CONTENT).unwrap();
    fs::set_permissions(&replacement, fs::Permissions::from_mode(0o600)).unwrap();
    let replacement_inode = fs::metadata(&replacement).unwrap().ino();
    let result = unix::publish_before_retry(
        &fixture.directory,
        CONTENT,
        || {
            fs::remove_file(&stage).unwrap();
        },
        || {
            fs::rename(&replacement, fixture.target()).unwrap();
        },
    );
    assert_eq!(result, Err(Error::SetupFileConflict));
    assert_eq!(
        fs::metadata(fixture.target()).unwrap().ino(),
        replacement_inode
    );
}
