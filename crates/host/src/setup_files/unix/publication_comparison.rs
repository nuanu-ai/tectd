use super::*;

// A stage unlink can invalidate a strict read without changing the published file.
// Only publication gets one new strict read, pinned to the observed final inode.
pub(super) fn publication_target_state<F, G>(
    directory: &SetupDirectory,
    dir: &OwnedFd,
    max_bytes: usize,
    after_read: bool,
    mut after_metadata: F,
    mut before_retry: G,
) -> Result<TargetState>
where
    F: FnMut(),
    G: FnMut(),
{
    let first = target_state_after_metadata(dir, max_bytes, None, after_read, &mut after_metadata);
    let state = match first {
        TargetState::StageCleanup(ref pinned) => {
            before_retry();
            let fresh = target_state_after_metadata(
                dir,
                max_bytes,
                Some(pinned),
                after_read,
                after_metadata,
            );
            revalidate_directory(directory)?;
            match fresh {
                TargetState::Missing => TargetState::Unavailable("file_changed_during_inspection"),
                state => state,
            }
        }
        state => state,
    };
    Ok(state)
}

pub(super) fn exact_target(
    directory: &SetupDirectory,
    dir: &OwnedFd,
    expected: &str,
    byte_length: u64,
) -> Result<bool> {
    Ok(
        matches!(publication_target_state(directory, dir, usize::try_from(byte_length).unwrap_or(usize::MAX), false, || {}, || {})?,
        TargetState::Existing { byte_length: actual, sha256: Some(ref hash), reason: None }
            if actual == byte_length && hash == expected),
    )
}

pub(super) fn same_cleanup_fields(left: &fs::Stat, right: &fs::Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_mode == right.st_mode
        && left.st_uid == right.st_uid
        && left.st_gid == right.st_gid
}

pub(super) fn stage_cleanup_transition(
    before: &fs::Stat,
    after: &fs::Stat,
    named: &fs::Stat,
) -> bool {
    FileType::from_raw_mode(before.st_mode).is_file()
        && before.st_mode & 0o7777 == 0o600
        && before.st_uid == rustix::process::geteuid().as_raw()
        && before.st_nlink == 2
        && named.st_nlink == 1
        && ((after.st_nlink == 1 && same_file(after, named))
            || (after.st_nlink == 2 && same_file(before, after)))
        && same_cleanup_fields(before, after)
        && same_cleanup_fields(before, named)
        && (before.st_ctime != named.st_ctime || before.st_ctime_nsec != named.st_ctime_nsec)
}
