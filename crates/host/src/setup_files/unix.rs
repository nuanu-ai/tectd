use rustix::fd::OwnedFd;
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags};
use rustix::io::Errno;
use sha2::{Digest, Sha256};
use std::fs::File;
use std::io::{Read, Write};
use tect_domain::{
    Error, FileObservation, FilePublication, PublicationOutcome, Result, SetupDirectory,
    SetupFileStatus, setup_path_is_granted, validate_setup_path,
};
use uuid::Uuid;

mod publication_comparison;
use publication_comparison::{
    exact_target, publication_target_state, same_cleanup_fields, stage_cleanup_transition,
};

const TARGET: &str = "AGENTS.md";
const STAGE_ATTEMPTS: usize = 4;
const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const TARGET_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
struct Identity {
    device: i64,
    inode: i64,
}

#[derive(Debug)]
enum TargetState {
    Missing,
    Existing {
        byte_length: u64,
        sha256: Option<String>,
        reason: Option<&'static str>,
    },
    Unavailable(&'static str),
    StageCleanup(fs::Stat),
}

struct Stage {
    name: String,
    identity: Identity,
    file: File,
}

pub(super) fn resolve_directory(path: &str, current_roots: &[String]) -> Result<SetupDirectory> {
    validate_setup_path(path)?;
    if !setup_path_is_granted(path, current_roots) {
        return Err(Error::SetupUnavailable);
    }
    let (fd, identity) = walk_directory(path)?;
    drop(fd);
    Ok(SetupDirectory {
        path: path.to_owned(),
        device: identity.device,
        inode: identity.inode,
    })
}

pub(super) fn inspect(directory: &SetupDirectory, max_bytes: usize) -> Result<FileObservation> {
    let fd = checked_directory(directory)?;
    let state = target_state(&fd, max_bytes);
    revalidate_directory(directory)?;
    Ok(observation(state))
}

fn observation(state: TargetState) -> FileObservation {
    match state {
        TargetState::Missing => FileObservation::missing(),
        TargetState::Unavailable(reason) => FileObservation::unavailable(reason),
        TargetState::StageCleanup(_) => {
            FileObservation::unavailable("file_changed_during_inspection")
        }
        TargetState::Existing {
            byte_length,
            sha256,
            reason,
        } => FileObservation {
            status: SetupFileStatus::Existing,
            byte_length: Some(byte_length),
            sha256,
            reason: reason.map(str::to_owned),
        },
    }
}

pub(super) fn publish(directory: &SetupDirectory, content: &str) -> Result<FilePublication> {
    publish_after_metadata(directory, content, || {})
}

pub(super) fn publish_after_metadata<F: FnMut()>(
    directory: &SetupDirectory,
    content: &str,
    after_metadata: F,
) -> Result<FilePublication> {
    publish_with_hook(directory, content, false, after_metadata, || {})
}

#[cfg(test)]
pub(super) fn publish_after_file_metadata<F: FnMut()>(
    directory: &SetupDirectory,
    content: &str,
    after_metadata: F,
) -> Result<FilePublication> {
    publish_with_hook(directory, content, true, after_metadata, || {})
}

#[cfg(test)]
pub(super) fn publish_before_retry<F: FnMut(), G: FnMut()>(
    directory: &SetupDirectory,
    content: &str,
    after_metadata: F,
    before_retry: G,
) -> Result<FilePublication> {
    publish_with_hook(directory, content, false, after_metadata, before_retry)
}

fn publish_with_hook<F: FnMut(), G: FnMut()>(
    directory: &SetupDirectory,
    content: &str,
    after_read: bool,
    mut after_metadata: F,
    before_retry: G,
) -> Result<FilePublication> {
    let dir = checked_directory(directory)?;
    let expected = digest(content.as_bytes());
    let byte_length = u64::try_from(content.len()).map_err(|_| Error::RequestTooLarge)?;
    match publication_target_state(
        directory,
        &dir,
        content.len(),
        after_read,
        &mut after_metadata,
        before_retry,
    )? {
        TargetState::Existing {
            byte_length: actual,
            sha256: Some(ref hash),
            ..
        } if actual == byte_length && hash == &expected => {
            revalidate_directory(directory)?;
            return Ok(publication(
                PublicationOutcome::AlreadyMatches,
                expected,
                byte_length,
            ));
        }
        TargetState::Missing => {}
        TargetState::Existing { .. }
        | TargetState::Unavailable(_)
        | TargetState::StageCleanup(_) => {
            return Err(Error::SetupFileConflict);
        }
    }

    let mut stage = create_stage(&dir)?;
    if let Err(error) = write_stage(&mut stage, content.as_bytes()) {
        let _ = cleanup_stage(&dir, &stage);
        return Err(error);
    }
    match fs::linkat(&dir, stage.name.as_str(), &dir, TARGET, AtFlags::empty()) {
        Ok(()) => finish_created(directory, &dir, &stage, &expected, byte_length),
        Err(error) if error == Errno::EXIST => {
            let winner = exact_target(directory, &dir, &expected, byte_length);
            cleanup_stage(&dir, &stage)?;
            fs::fsync(&dir).map_err(storage)?;
            revalidate_directory(directory)?;
            if winner? {
                Ok(publication(
                    PublicationOutcome::AlreadyMatches,
                    expected,
                    byte_length,
                ))
            } else {
                Err(Error::SetupFileConflict)
            }
        }
        Err(_) => {
            cleanup_stage(&dir, &stage)?;
            Err(Error::StorageUnavailable)
        }
    }
}

fn finish_created(
    directory: &SetupDirectory,
    dir: &OwnedFd,
    stage: &Stage,
    expected: &str,
    byte_length: u64,
) -> Result<FilePublication> {
    let first_sync = fs::fsync(dir).map_err(storage);
    let verified =
        first_sync.is_ok() && exact_target(directory, dir, expected, byte_length).unwrap_or(false);
    let identity_result = revalidate_directory(directory);
    let cleanup_result = cleanup_stage(dir, stage);
    let second_sync = cleanup_result
        .as_ref()
        .map(|_| fs::fsync(dir).map_err(storage))
        .unwrap_or(Ok(()));
    first_sync?;
    identity_result?;
    cleanup_result?;
    second_sync?;
    if !verified {
        return Err(Error::SetupFileConflict);
    }
    Ok(publication(
        PublicationOutcome::Created,
        expected.to_owned(),
        byte_length,
    ))
}

fn publication(outcome: PublicationOutcome, sha256: String, byte_length: u64) -> FilePublication {
    FilePublication {
        outcome,
        sha256,
        byte_length,
    }
}

fn walk_directory(path: &str) -> Result<(OwnedFd, Identity)> {
    let mut fd = fs::openat(fs::ABS, "/", DIRECTORY_FLAGS, Mode::empty())
        .map_err(|_| Error::SetupUnavailable)?;
    for component in path[1..].split('/').filter(|part| !part.is_empty()) {
        fd = fs::openat(&fd, component, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|_| Error::SetupUnavailable)?;
    }
    let stat = fs::fstat(&fd).map_err(|_| Error::SetupUnavailable)?;
    if !FileType::from_raw_mode(stat.st_mode).is_dir() {
        return Err(Error::SetupUnavailable);
    }
    Ok((fd, identity(&stat)?))
}

fn checked_directory(directory: &SetupDirectory) -> Result<OwnedFd> {
    validate_setup_path(&directory.path)?;
    let (fd, actual) = walk_directory(&directory.path)?;
    if actual
        != (Identity {
            device: directory.device,
            inode: directory.inode,
        })
    {
        return Err(Error::TaskDirectoryMismatch);
    }
    Ok(fd)
}

fn revalidate_directory(directory: &SetupDirectory) -> Result<()> {
    checked_directory(directory).map(drop)
}

fn target_state(dir: &OwnedFd, max_bytes: usize) -> TargetState {
    target_state_after_metadata(dir, max_bytes, None, false, || {})
}

fn target_state_after_metadata<F>(
    dir: &OwnedFd,
    max_bytes: usize,
    pinned: Option<&fs::Stat>,
    after_read: bool,
    after_metadata: F,
) -> TargetState
where
    F: FnOnce(),
{
    let opened = match fs::openat(dir, TARGET, TARGET_FLAGS, Mode::empty()) {
        Ok(fd) => fd,
        Err(error) if error == Errno::NOENT => return TargetState::Missing,
        Err(error) => return TargetState::Unavailable(open_reason(error)),
    };
    let mut file = File::from(opened);
    let before = match fs::fstat(&file) {
        Ok(stat) => stat,
        Err(_) => return TargetState::Unavailable("inspection_failed"),
    };
    if pinned.is_some_and(|expected| {
        !same_file(expected, &before)
            || !same_cleanup_fields(expected, &before)
            || before.st_nlink != 1
    }) {
        return TargetState::Unavailable("file_changed_during_inspection");
    }
    if !FileType::from_raw_mode(before.st_mode).is_file() {
        return TargetState::Unavailable("unsupported_file_type");
    }
    let Ok(byte_length) = u64::try_from(before.st_size) else {
        return TargetState::Unavailable("inspection_failed");
    };
    let Ok(size) = usize::try_from(byte_length) else {
        return TargetState::Existing {
            byte_length,
            sha256: None,
            reason: Some("comparison_capacity_exceeded"),
        };
    };
    if size > max_bytes {
        return TargetState::Existing {
            byte_length,
            sha256: None,
            reason: Some("comparison_capacity_exceeded"),
        };
    }
    let mut hook = Some(after_metadata);
    if !after_read {
        hook.take().unwrap()();
    }
    let mut bytes = Vec::with_capacity(size.saturating_add(1));
    if Read::by_ref(&mut file)
        .take((max_bytes as u64).saturating_add(1))
        .read_to_end(&mut bytes)
        .is_err()
    {
        return TargetState::Unavailable("inspection_failed");
    }
    let after = match fs::fstat(&file) {
        Ok(stat) => stat,
        Err(_) => return TargetState::Unavailable("inspection_failed"),
    };
    if after_read {
        hook.take().unwrap()();
    }
    let named = match fs::statat(dir, TARGET, AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(_) => return TargetState::Unavailable("file_changed_during_inspection"),
    };
    if bytes.len() != size
        || !same_file(&before, &after)
        || !same_file(&before, &named)
        || !FileType::from_raw_mode(named.st_mode).is_file()
    {
        if pinned.is_none()
            && bytes.len() == size
            && stage_cleanup_transition(&before, &after, &named)
        {
            return TargetState::StageCleanup(named);
        }
        return TargetState::Unavailable("file_changed_during_inspection");
    }
    if pinned.is_some()
        && (!same_cleanup_fields(&before, &after)
            || !same_cleanup_fields(&before, &named)
            || after.st_nlink != 1
            || named.st_nlink != 1)
    {
        return TargetState::Unavailable("file_changed_during_inspection");
    }
    TargetState::Existing {
        byte_length,
        sha256: Some(digest(&bytes)),
        reason: None,
    }
}

#[cfg(test)]
pub(super) fn inspect_after_metadata<F>(
    directory: &SetupDirectory,
    max_bytes: usize,
    after_metadata: F,
) -> Result<FileObservation>
where
    F: FnOnce(),
{
    let fd = checked_directory(directory)?;
    let state = target_state_after_metadata(&fd, max_bytes, None, false, after_metadata);
    revalidate_directory(directory)?;
    Ok(observation(state))
}

fn create_stage(dir: &OwnedFd) -> Result<Stage> {
    for _ in 0..STAGE_ATTEMPTS {
        let name = format!(".tectd-agents-{}.tmp", Uuid::new_v4());
        match fs::openat(
            dir,
            name.as_str(),
            OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
            Mode::RUSR | Mode::WUSR,
        ) {
            Ok(fd) => {
                let stat = fs::fstat(&fd).map_err(storage)?;
                return Ok(Stage {
                    name,
                    identity: identity(&stat)?,
                    file: File::from(fd),
                });
            }
            Err(error) if error == Errno::EXIST => continue,
            Err(_) => return Err(Error::StorageUnavailable),
        }
    }
    Err(Error::StorageUnavailable)
}

fn write_stage(stage: &mut Stage, content: &[u8]) -> Result<()> {
    stage
        .file
        .write_all(content)
        .map_err(|_| Error::StorageUnavailable)?;
    stage.file.sync_all().map_err(|_| Error::StorageUnavailable)
}

fn cleanup_stage(dir: &OwnedFd, stage: &Stage) -> Result<()> {
    let stat = match fs::statat(dir, stage.name.as_str(), AtFlags::SYMLINK_NOFOLLOW) {
        Ok(stat) => stat,
        Err(error) if error == Errno::NOENT => return Ok(()),
        Err(_) => return Err(Error::StorageUnavailable),
    };
    if identity(&stat)? != stage.identity {
        return Err(Error::StorageUnavailable);
    }
    fs::unlinkat(dir, stage.name.as_str(), AtFlags::empty()).map_err(storage)
}

#[allow(clippy::unnecessary_fallible_conversions)]
fn identity(stat: &fs::Stat) -> Result<Identity> {
    Ok(Identity {
        device: i64::try_from(stat.st_dev).map_err(|_| Error::SetupUnavailable)?,
        inode: i64::try_from(stat.st_ino).map_err(|_| Error::SetupUnavailable)?,
    })
}

fn same_file(left: &fs::Stat, right: &fs::Stat) -> bool {
    left.st_dev == right.st_dev
        && left.st_ino == right.st_ino
        && left.st_size == right.st_size
        && left.st_mtime == right.st_mtime
        && left.st_mtime_nsec == right.st_mtime_nsec
        && left.st_ctime == right.st_ctime
        && left.st_ctime_nsec == right.st_ctime_nsec
}

fn open_reason(error: Errno) -> &'static str {
    if error == Errno::ACCESS || error == Errno::PERM {
        "access_denied"
    } else if error == Errno::LOOP {
        "symlink_target"
    } else {
        "inspection_failed"
    }
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn storage(_: Errno) -> Error {
    Error::StorageUnavailable
}
