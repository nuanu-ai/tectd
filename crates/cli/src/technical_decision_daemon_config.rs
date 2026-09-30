//! Optional operator-owned technical approvals, separate from artifact bytes.
use rustix::fs::{self as fd_fs, Mode, OFlags};
use std::fs::{self, File};
use std::io::Read;
use std::os::unix::fs::MetadataExt;
use std::path::{Component, Path, PathBuf};
use tect_domain::{Error, Result};
use tect_postgres::{ApprovedTechnicalDecisionEvidence, parse_technical_decision_approvals};

const MAX_APPROVAL_BYTES: u64 = 1024 * 1024;

pub(super) fn from_env() -> Result<Option<Vec<ApprovedTechnicalDecisionEvidence>>> {
    match std::env::var("TECT_TECHNICAL_DECISION_APPROVAL_FILE") {
        Ok(path) => read(Path::new(&path)).map(Some),
        Err(std::env::VarError::NotPresent) => Ok(None),
        Err(_) => Err(Error::InvalidConfiguration),
    }
}

fn metadata_valid(m: &fs::Metadata, owner_uid: u32) -> Result<()> {
    if !m.is_file()
        || m.len() > MAX_APPROVAL_BYTES
        || m.mode() & 0o7777 != 0o600
        || m.uid() != owner_uid
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn read(path: &Path) -> Result<Vec<ApprovedTechnicalDecisionEvidence>> {
    read_after_open(path, || {})
}

const DIRECTORY_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::DIRECTORY)
    .union(OFlags::NOFOLLOW)
    .union(OFlags::CLOEXEC);
const FILE_FLAGS: OFlags = OFlags::RDONLY
    .union(OFlags::NOFOLLOW)
    .union(OFlags::NONBLOCK)
    .union(OFlags::CLOEXEC);

/// Every component is resolved against the preceding open directory. No
/// path-based precheck is used as permission to follow a later symlink.
fn open_directory(path: &Path) -> Result<File> {
    let mut fd = fd_fs::openat(fd_fs::ABS, "/", DIRECTORY_FLAGS, Mode::empty())
        .map_err(|_| Error::InvalidConfiguration)?;
    for component in path.components().skip(1) {
        let Component::Normal(name) = component else {
            return Err(Error::InvalidConfiguration);
        };
        fd = fd_fs::openat(&fd, name, DIRECTORY_FLAGS, Mode::empty())
            .map_err(|_| Error::InvalidConfiguration)?;
    }
    Ok(File::from(fd))
}

fn directory_valid(m: &fs::Metadata, owner_uid: u32) -> Result<()> {
    if !m.is_dir() || m.uid() != owner_uid || m.mode() & 0o7777 != 0o700 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn same_identity(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    a.dev() == b.dev() && a.ino() == b.ino() && a.uid() == b.uid() && a.mode() == b.mode()
}

fn stable_file(a: &fs::Metadata, b: &fs::Metadata) -> bool {
    same_identity(a, b)
        && a.len() == b.len()
        && a.mtime() == b.mtime()
        && a.mtime_nsec() == b.mtime_nsec()
        && a.ctime() == b.ctime()
        && a.ctime_nsec() == b.ctime_nsec()
}

fn read_after_open(
    path: &Path,
    after_open: impl FnOnce(),
) -> Result<Vec<ApprovedTechnicalDecisionEvidence>> {
    if !path.is_absolute() {
        return Err(Error::InvalidConfiguration);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => return Err(Error::InvalidConfiguration),
        }
    }
    if current.as_os_str() != path.as_os_str() {
        return Err(Error::InvalidConfiguration);
    }
    let owner_uid = rustix::process::geteuid().as_raw();
    let parent_path = path.parent().ok_or(Error::InvalidConfiguration)?;
    let name = path.file_name().ok_or(Error::InvalidConfiguration)?;
    let parent = open_directory(parent_path)?;
    let parent_opened = parent.metadata().map_err(|_| Error::InvalidConfiguration)?;
    directory_valid(&parent_opened, owner_uid)?;
    let mut file = File::from(
        fd_fs::openat(&parent, name, FILE_FLAGS, Mode::empty())
            .map_err(|_| Error::InvalidConfiguration)?,
    );
    let opened = file.metadata().map_err(|_| Error::InvalidConfiguration)?;
    metadata_valid(&opened, owner_uid)?;
    after_open();
    let mut bytes = Vec::with_capacity(opened.len() as usize);
    file.by_ref()
        .take(MAX_APPROVAL_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::InvalidConfiguration)?;
    if bytes.len() as u64 > MAX_APPROVAL_BYTES || bytes.len() as u64 != opened.len() {
        return Err(Error::InvalidConfiguration);
    }
    let finished = file.metadata().map_err(|_| Error::InvalidConfiguration)?;
    metadata_valid(&finished, owner_uid)?;
    if !stable_file(&opened, &finished) {
        return Err(Error::InvalidConfiguration);
    }
    // Rewalk from root to detect a moved/replaced containing directory, then
    // re-open the final entry without following links and compare the pin.
    let current_parent = open_directory(parent_path)?;
    let parent_current = current_parent
        .metadata()
        .map_err(|_| Error::InvalidConfiguration)?;
    directory_valid(&parent_current, owner_uid)?;
    let current_file = File::from(
        fd_fs::openat(&current_parent, name, FILE_FLAGS, Mode::empty())
            .map_err(|_| Error::InvalidConfiguration)?,
    );
    let current = current_file
        .metadata()
        .map_err(|_| Error::InvalidConfiguration)?;
    metadata_valid(&current, owner_uid)?;
    if !same_identity(&parent_opened, &parent_current) || !stable_file(&opened, &current) {
        return Err(Error::InvalidConfiguration);
    }
    let body = std::str::from_utf8(&bytes).map_err(|_| Error::InvalidConfiguration)?;
    parse_technical_decision_approvals(body)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::{PermissionsExt, symlink};

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let directory = tempfile::tempdir().unwrap();
        let parent = directory.path().canonicalize().unwrap();
        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
        let path = parent.join("synthetic-approvals.json");
        fs::write(&path, "[]").unwrap();
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        (directory, path)
    }

    #[test]
    fn descriptor_traversal_rejects_parent_symlink_nonprivate_parent_and_wrong_uid() {
        let (_directory, path) = fixture();
        let parent = path.parent().unwrap();
        let linked_parent = parent.join("parent-link");
        symlink(parent, &linked_parent).unwrap();
        assert!(read(&linked_parent.join("synthetic-approvals.json")).is_err());
        let actual_uid = rustix::process::geteuid().as_raw();
        let file_metadata = File::open(&path).unwrap().metadata().unwrap();
        let parent_metadata = File::open(parent).unwrap().metadata().unwrap();
        assert_eq!(file_metadata.uid(), actual_uid);
        assert_eq!(parent_metadata.uid(), actual_uid);
        assert!(metadata_valid(&file_metadata, actual_uid).is_ok());
        assert!(directory_valid(&parent_metadata, actual_uid).is_ok());
        assert!(metadata_valid(&file_metadata, actual_uid.wrapping_add(1)).is_err());
        assert!(directory_valid(&parent_metadata, actual_uid.wrapping_add(1)).is_err());
        fs::set_permissions(parent, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(read(&path).is_err());
        fs::set_permissions(parent, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(read(&path).unwrap().is_empty());
        let non_normal = PathBuf::from(format!("{}/./synthetic-approvals.json", parent.display()));
        assert!(read(&non_normal).is_err());
    }

    #[test]
    fn stable_read_rejects_size_timestamp_mode_and_final_entry_replacement() {
        for mutation in 0..4 {
            let (_directory, path) = fixture();
            let result = read_after_open(&path, || match mutation {
                0 => fs::write(&path, "[ ]").unwrap(),
                1 => {
                    let file = File::open(&path).unwrap();
                    let changed = file.metadata().unwrap().modified().unwrap()
                        + std::time::Duration::from_secs(1);
                    file.set_modified(changed).unwrap();
                }
                2 => fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap(),
                _ => {
                    fs::rename(&path, path.with_extension("old")).unwrap();
                    fs::write(&path, "[]").unwrap();
                    fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                }
            });
            assert!(result.is_err(), "accepted mutation {mutation}");
        }
    }

    #[test]
    fn stable_read_rejects_replaced_or_symlinked_ancestor_after_open() {
        for substitute_symlink in [false, true] {
            let outer = tempfile::tempdir().unwrap();
            let outer = outer.path().canonicalize().unwrap();
            let parent = outer.join("private");
            fs::create_dir(&parent).unwrap();
            fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
            let path = parent.join("synthetic-approvals.json");
            fs::write(&path, "[]").unwrap();
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
            assert!(
                read_after_open(&path, || {
                    let moved = outer.join("moved");
                    fs::rename(&parent, &moved).unwrap();
                    if substitute_symlink {
                        symlink(&moved, &parent).unwrap();
                    } else {
                        fs::create_dir(&parent).unwrap();
                        fs::set_permissions(&parent, fs::Permissions::from_mode(0o700)).unwrap();
                        fs::write(&path, "[]").unwrap();
                        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
                    }
                })
                .is_err()
            );
        }
    }

    #[test]
    fn synthetic_empty_whitelist_is_private_bounded_and_fail_closed() {
        let (_directory, path) = fixture();
        let directory = path.parent().unwrap();
        assert!(read(&path).unwrap().is_empty());
        assert!(read(Path::new("relative.json")).is_err());
        let link = directory.join("link.json");
        symlink(&path, &link).unwrap();
        assert!(read(&link).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o644)).unwrap();
        assert!(read(&path).is_err());
        fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        fs::write(&path, "[{\"untrusted\":true}]").unwrap();
        assert!(read(&path).is_err());
        fs::write(&path, vec![b' '; MAX_APPROVAL_BYTES as usize + 1]).unwrap();
        assert!(read(&path).is_err());
    }
}
