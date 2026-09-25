fn canonical_source_roots(paths: Vec<PathBuf>) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut roots = Vec::with_capacity(paths.len());
    for path in paths {
        let canonical = std::fs::canonicalize(path).map_err(|_| Error::InvalidSource)?;
        if !canonical.is_dir() {
            return Err(Error::InvalidSource);
        }
        let root = canonical
            .into_os_string()
            .into_string()
            .map_err(|_| Error::InvalidSource)?;
        if seen.insert(root.clone()) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn canonical_setup_roots(paths: Vec<PathBuf>) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut roots = Vec::with_capacity(paths.len());
    for path in paths {
        let root = canonical_setup_root(path)?;
        if seen.insert(root.clone()) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn canonical_setup_root(path: PathBuf) -> Result<String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidArguments);
    }
    let selected = path.to_str().ok_or(Error::InvalidArguments)?;
    validate_setup_path(selected)?;

    let canonical = std::fs::canonicalize(&path).map_err(|_| Error::SetupUnavailable)?;
    verify_setup_directory_chain(&canonical)?;
    let root = canonical
        .into_os_string()
        .into_string()
        .map_err(|_| Error::InvalidArguments)?;
    validate_setup_path(&root)?;
    Ok(root)
}

fn verify_setup_directory_chain(path: &Path) -> Result<()> {
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(Error::InvalidArguments);
            }
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| Error::SetupUnavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::SetupUnavailable);
        }
    }
    Ok(())
}

fn preflight_output(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::InvalidArguments);
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(Error::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Error::InvalidConfiguration),
    }
    let parent = path.parent().ok_or(Error::InvalidArguments)?;
    let metadata = verify_directory_chain(parent)?;
    if metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn verify_directory_chain(path: &Path) -> Result<std::fs::Metadata> {
    let mut current = PathBuf::from("/");
    let mut final_metadata =
        std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(Error::InvalidArguments);
            }
        }
        final_metadata =
            std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if final_metadata.file_type().is_symlink() || !final_metadata.is_dir() {
            return Err(Error::InvalidConfiguration);
        }
    }
    Ok(final_metadata)
}

fn write_auth_file(path: &Path, auth: &HostAuth) -> Result<()> {
    preflight_output(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::InvalidConfiguration)?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let result = write_and_link(&temporary, path, auth);
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

/// Removes only the inode created by this invocation if an error occurs.
struct PublishedAuthFile {
    path: PathBuf,
    device: u64,
    inode: u64,
    armed: bool,
}

impl PublishedAuthFile {
    fn new(path: PathBuf, file: &File) -> Result<Self> {
        let metadata = file.metadata().map_err(|_| Error::InvalidConfiguration)?;
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
            armed: true,
        })
    }

    fn same_inode(path: PathBuf, existing: &Self) -> Self {
        Self {
            path,
            device: existing.device,
            inode: existing.inode,
            armed: true,
        }
    }

    fn remove(&mut self) -> Result<()> {
        if !self.armed {
            return Ok(());
        }
        let metadata =
            std::fs::symlink_metadata(&self.path).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.dev() != self.device || metadata.ino() != self.inode {
            return Err(Error::InvalidConfiguration);
        }
        std::fs::remove_file(&self.path).map_err(|_| Error::InvalidConfiguration)?;
        self.armed = false;
        Ok(())
    }

    fn retain(mut self) {
        self.armed = false;
    }
}

impl Drop for PublishedAuthFile {
    fn drop(&mut self) {
        let _ = self.remove();
    }
}

fn publish_verifier_auth_file(path: &Path, auth: &HostAuth) -> Result<PublishedAuthFile> {
    preflight_output(path)?;
    let parent = path.parent().ok_or(Error::InvalidConfiguration)?;
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::InvalidConfiguration)?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(&temporary)
        .map_err(|_| Error::InvalidConfiguration)?;
    let mut temporary_guard = match PublishedAuthFile::new(temporary.clone(), &file) {
        Ok(guard) => guard,
        Err(error) => {
            let _ = std::fs::remove_file(temporary);
            return Err(error);
        }
    };
    let mut contents = serde_json::to_vec_pretty(auth).map_err(|_| Error::InvalidConfiguration)?;
    contents.push(b'\n');
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    std::fs::hard_link(&temporary_guard.path, path).map_err(|_| Error::InvalidConfiguration)?;
    let destination_guard = PublishedAuthFile::same_inode(path.to_path_buf(), &temporary_guard);
    File::open(path)
        .and_then(|published| published.sync_all())
        .and_then(|()| File::open(parent)?.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    temporary_guard.remove()?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    Ok(destination_guard)
}

fn write_and_link(temporary: &Path, destination: &Path, auth: &HostAuth) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)
        .map_err(|_| Error::InvalidConfiguration)?;
    let mut contents = serde_json::to_vec_pretty(auth).map_err(|_| Error::InvalidConfiguration)?;
    contents.push(b'\n');
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    std::fs::hard_link(temporary, destination).map_err(|_| Error::InvalidConfiguration)?;
    File::open(destination)
        .and_then(|created| created.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    Ok(())
}
