use std::fs;
use std::os::unix::fs::{FileTypeExt, MetadataExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::sync::Arc;
use tect_application::WorkspaceService;
use tect_domain::Error;
use tect_postgres::PgStore;
use tokio::net::UnixListener;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}", error.code());
        std::process::exit(1);
    }
}

async fn run() -> tect_domain::Result<()> {
    let socket = required_absolute_socket()?;
    let database_url = required_env("TECT_DATABASE_URL")?;
    validate_socket_parent(&socket)?;
    reject_existing_path(&socket)?;

    let store = Arc::new(PgStore::connect(&database_url, 16).await?);
    let service = Arc::new(WorkspaceService::new(
        store,
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let listener = UnixListener::bind(&socket).map_err(|_| Error::InvalidConfiguration)?;
    let guard = SocketGuard::capture(socket)?;
    guard.set_private()?;

    let mut server = tokio::spawn(tect_host::serve(listener, service));
    tokio::select! {
        result = &mut server => {
            result.map_err(|_| Error::TransportUnavailable)?
        }
        signal = tokio::signal::ctrl_c() => {
            signal.map_err(|_| Error::TransportUnavailable)?;
            server.abort();
            let _ = server.await;
            Ok(())
        }
    }
}

fn required_env(name: &str) -> tect_domain::Result<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(Error::InvalidConfiguration),
    }
}

fn required_absolute_socket() -> tect_domain::Result<PathBuf> {
    let path = PathBuf::from(required_env("TECT_SOCKET")?);
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path)
}

fn validate_socket_parent(socket: &Path) -> tect_domain::Result<()> {
    let parent = socket.parent().ok_or(Error::InvalidConfiguration)?;
    let mut current = PathBuf::new();
    for component in parent.components() {
        current.push(component.as_os_str());
        let metadata = fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidConfiguration);
        }
    }
    let metadata = fs::symlink_metadata(parent).map_err(|_| Error::InvalidConfiguration)?;
    if metadata.file_type().is_symlink() || !metadata.is_dir() || metadata.mode() & 0o7777 != 0o700
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn reject_existing_path(socket: &Path) -> tect_domain::Result<()> {
    match fs::symlink_metadata(socket) {
        Ok(_) => Err(Error::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        Err(_) => Err(Error::InvalidConfiguration),
    }
}

struct SocketGuard {
    path: PathBuf,
    device: u64,
    inode: u64,
}

impl SocketGuard {
    fn capture(path: PathBuf) -> tect_domain::Result<Self> {
        let metadata = fs::symlink_metadata(&path).map_err(|_| Error::InvalidConfiguration)?;
        if !metadata.file_type().is_socket() {
            return Err(Error::InvalidConfiguration);
        }
        Ok(Self {
            path,
            device: metadata.dev(),
            inode: metadata.ino(),
        })
    }

    fn set_private(&self) -> tect_domain::Result<()> {
        fs::set_permissions(&self.path, fs::Permissions::from_mode(0o600))
            .map_err(|_| Error::InvalidConfiguration)?;
        let metadata = fs::symlink_metadata(&self.path).map_err(|_| Error::InvalidConfiguration)?;
        if !metadata.file_type().is_socket()
            || metadata.dev() != self.device
            || metadata.ino() != self.inode
            || metadata.mode() & 0o7777 != 0o600
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }

    fn remove_if_owned(&self) {
        let Ok(metadata) = fs::symlink_metadata(&self.path) else {
            return;
        };
        if metadata.file_type().is_socket()
            && metadata.dev() == self.device
            && metadata.ino() == self.inode
        {
            let _ = fs::remove_file(&self.path);
        }
    }
}

impl Drop for SocketGuard {
    fn drop(&mut self) {
        self.remove_if_owned();
    }
}
