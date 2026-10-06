use super::*;

pub(super) fn validate_socket(path: &Path) -> Result<SocketIdentity> {
    if !path.is_absolute() {
        return Err(Error::InvalidConfiguration);
    }
    let mut current = PathBuf::new();
    for component in path.components() {
        match component {
            Component::RootDir | Component::Normal(_) => current.push(component.as_os_str()),
            _ => return Err(Error::InvalidConfiguration),
        }
        let metadata = fs::symlink_metadata(&current).map_err(|error| {
            if error.kind() == std::io::ErrorKind::NotFound {
                Error::TransportUnavailable
            } else {
                Error::InvalidConfiguration
            }
        })?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidConfiguration);
        }
    }

    let parent = path.parent().ok_or(Error::InvalidConfiguration)?;
    let parent_metadata = fs::symlink_metadata(parent).map_err(|_| Error::InvalidConfiguration)?;
    if !parent_metadata.is_dir() || parent_metadata.mode() & 0o7777 != 0o700 {
        return Err(Error::InvalidConfiguration);
    }
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::TransportUnavailable)?;
    if !metadata.file_type().is_socket() || metadata.mode() & 0o7777 != 0o600 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(SocketIdentity {
        device: metadata.dev(),
        inode: metadata.ino(),
    })
}
