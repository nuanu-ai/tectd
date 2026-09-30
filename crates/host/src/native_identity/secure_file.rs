use super::record::{CLAUDE_ATTESTATION_MAX_BYTES, claude_attestation_filename};
use crate::Result;
use rustix::fd::OwnedFd;
use rustix::fs::{self, AtFlags, FileType, Mode, OFlags, Stat};
use std::fs::File;
use std::io::Read;
use std::os::unix::ffi::OsStrExt;
use std::path::{Component, Path};
use tect_domain::Error;

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

fn ancestor_traversal_flags() -> Result<OFlags> {
    #[cfg(target_os = "macos")]
    {
        // Darwin's named O_SEARCH is O_EXEC | O_DIRECTORY. rustix does not
        // expose it; retain the platform-defined bits without a numeric copy.
        Ok(OFlags::from_bits_retain(libc::O_SEARCH as _) | OFlags::NOFOLLOW | OFlags::CLOEXEC)
    }
    #[cfg(target_os = "linux")]
    {
        // O_PATH obtains a directory capability without reading its contents.
        Ok(OFlags::PATH | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC)
    }
    #[cfg(not(any(target_os = "macos", target_os = "linux")))]
    {
        Err(Error::InvalidNativeSession)
    }
}

/// Open every path component relative to the prior fd; never check then open
/// an absolute path. The final holder belongs to this effective user, mode 0700.
pub fn open_claude_attestation_directory(path: &Path) -> Result<OwnedFd> {
    if !path.is_absolute()
        || path
            .as_os_str()
            .as_bytes()
            .split(|byte| *byte == b'/')
            .any(|part| matches!(part, b"." | b".."))
    {
        return Err(Error::InvalidNativeSession);
    }
    let traversal_flags = ancestor_traversal_flags()?;
    let mut components = path.components().peekable();
    let root_flags = if path == Path::new("/") {
        DIRECTORY_FLAGS
    } else {
        traversal_flags
    };
    let mut directory = fs::openat(fs::ABS, "/", root_flags, Mode::empty())
        .map_err(|_| Error::InvalidNativeSession)?;
    while let Some(component) = components.next() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                let flags = if components.peek().is_none() {
                    DIRECTORY_FLAGS
                } else {
                    traversal_flags
                };
                directory = fs::openat(&directory, name, flags, Mode::empty())
                    .map_err(|_| Error::InvalidNativeSession)?;
            }
            _ => return Err(Error::InvalidNativeSession),
        }
    }
    let stat = fs::fstat(&directory).map_err(|_| Error::InvalidNativeSession)?;
    validate_directory(&stat, rustix::process::geteuid().as_raw())?;
    Ok(directory)
}

pub(crate) fn read_record(directory: &Path, tool_use_id: &str) -> Result<Vec<u8>> {
    let directory = open_claude_attestation_directory(directory)?;
    let name = claude_attestation_filename(tool_use_id)?;
    let fd = fs::openat(&directory, &name, FILE_FLAGS, Mode::empty())
        .map_err(|_| Error::InvalidNativeSession)?;
    let mut file = File::from(fd);
    let before = fs::fstat(&file).map_err(|_| Error::InvalidNativeSession)?;
    validate_file(&before, rustix::process::geteuid().as_raw())?;
    let mut bytes = Vec::with_capacity(before.st_size as usize);
    Read::by_ref(&mut file)
        .take(CLAUDE_ATTESTATION_MAX_BYTES as u64 + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidNativeSession)?;
    let after = fs::fstat(&file).map_err(|_| Error::InvalidNativeSession)?;
    let named = fs::statat(&directory, &name, AtFlags::SYMLINK_NOFOLLOW)
        .map_err(|_| Error::InvalidNativeSession)?;
    validate_file(&after, rustix::process::geteuid().as_raw())?;
    validate_file(&named, rustix::process::geteuid().as_raw())?;
    if bytes.len() > CLAUDE_ATTESTATION_MAX_BYTES
        || bytes.len() as u64 != before.st_size as u64
        || !same_file(&before, &after)
        || !same_file(&before, &named)
    {
        return Err(Error::InvalidNativeSession);
    }
    Ok(bytes)
}

pub(super) fn validate_directory(stat: &Stat, owner: u32) -> Result<()> {
    if !FileType::from_raw_mode(stat.st_mode).is_dir()
        || stat.st_mode & 0o7777 != 0o700
        || stat.st_uid != owner
    {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}

pub(super) fn validate_file(stat: &Stat, owner: u32) -> Result<()> {
    if !FileType::from_raw_mode(stat.st_mode).is_file()
        || stat.st_mode & 0o7777 != 0o600
        || stat.st_uid != owner
        || stat.st_nlink != 1
        || stat.st_size < 0
        || stat.st_size as u64 > CLAUDE_ATTESTATION_MAX_BYTES as u64
    {
        return Err(Error::InvalidNativeSession);
    }
    Ok(())
}

fn same_file(before: &Stat, after: &Stat) -> bool {
    before.st_dev == after.st_dev
        && before.st_ino == after.st_ino
        && before.st_size == after.st_size
        && before.st_mode == after.st_mode
        && before.st_uid == after.st_uid
        && before.st_nlink == after.st_nlink
        && before.st_mtime == after.st_mtime
        && before.st_mtime_nsec == after.st_mtime_nsec
        && before.st_ctime == after.st_ctime
        && before.st_ctime_nsec == after.st_ctime_nsec
}
