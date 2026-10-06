use super::{kill_and_reap, kill_and_reap_with_probe, retain_first};
use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt, PermissionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};

pub struct Daemon {
    pub child: Child,
    pub socket: PathBuf,
    pub(super) inode: (u64, u64),
}
impl Daemon {
    pub async fn start(url: &str, socket: PathBuf) -> Self {
        Self::start_with(Path::new(env!("CARGO_BIN_EXE_tectd")), url, socket).await
    }
    pub async fn start_with(binary: &Path, url: &str, socket: PathBuf) -> Self {
        Self::start_configured_result(binary, url, socket, None)
            .await
            .unwrap()
    }
    #[allow(dead_code)]
    pub async fn start_maintenance(url: &str, socket: PathBuf, contexts: &Path) -> Self {
        Self::start_configured_result(
            Path::new(env!("CARGO_BIN_EXE_tectd")),
            url,
            socket,
            Some(contexts),
        )
        .await
        .unwrap()
    }
    pub async fn start_configured_result(
        binary: &Path,
        url: &str,
        socket: PathBuf,
        contexts: Option<&Path>,
    ) -> Result<Self, String> {
        // Scope: fresh exclusive private_temp roots; no concurrent same-UID
        // writer. Metadata checks do not authenticate hostile same-UID actors.
        let parent = socket.parent().ok_or("daemon socket parent")?;
        let canonical = parent
            .canonicalize()
            .map_err(|_| "daemon parent canonicalization")?;
        let metadata = fs::symlink_metadata(parent).map_err(|_| "daemon parent metadata")?;
        if parent != canonical
            || !metadata.is_dir()
            || metadata.permissions().mode() & 0o777 != 0o700
            || metadata.uid() != rustix::process::geteuid().as_raw()
        {
            return Err("daemon parent must be canonical UID-private directory".into());
        }
        let parent_identity = (metadata.dev(), metadata.ino());
        require_absent(&socket)?;
        let log_path = socket.with_extension("stderr");
        if log_path == socket {
            return Err("daemon log/socket path collision".into());
        }
        // Explicitly uncapped stderr file: B2 remains open.
        let log = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(log_path)
            .map_err(|_| "daemon stderr create_new")?;
        let mut command = Self::command(binary, url, &socket, contexts);
        let mut child = command
            .stderr(Stdio::from(log))
            .spawn()
            .map_err(|_| "daemon spawn")?;
        let inode = Self::startup_owned(
            &mut child,
            &socket,
            parent,
            parent_identity,
            Duration::from_secs(5),
        )
        .await?;
        Ok(Self {
            child,
            socket,
            inode,
        })
    }
    pub(super) async fn startup_owned(
        child: &mut Child,
        socket: &Path,
        parent: &Path,
        parent_identity: (u64, u64),
        deadline: Duration,
    ) -> Result<(u64, u64), String> {
        // Factory validates this parent/socket before spawn. Fixtures retain the
        // owned Child to independently observe exit after failure cleanup.
        let mut known = None;
        let ready = tokio::time::timeout(deadline, async {
            loop {
                if child
                    .try_wait()
                    .map_err(|_| "daemon startup status")?
                    .is_some()
                {
                    return Err("owned daemon exited during startup".to_owned());
                }
                let current = fs::symlink_metadata(parent).map_err(|_| "daemon parent metadata")?;
                if (current.dev(), current.ino()) != parent_identity {
                    return Err("daemon parent replaced".into());
                }
                match fs::symlink_metadata(&socket) {
                    Ok(metadata) => {
                        if !metadata.file_type().is_socket() {
                            return Err("daemon readiness path is not a socket".into());
                        }
                        let identity = (metadata.dev(), metadata.ino());
                        if known.is_some_and(|old| old != identity) {
                            return Err("daemon startup socket replaced".into());
                        }
                        known = Some(identity);
                        if metadata.uid() != rustix::process::geteuid().as_raw() {
                            return Err("daemon readiness socket permissions/owner".into());
                        }
                        // Producer binds before setting mode0600. Retain this
                        // identity while waiting inside the original deadline.
                        if metadata.permissions().mode() & 0o777 == 0o600 {
                            return Ok(identity);
                        }
                    }
                    Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
                    Err(_) => return Err("daemon readiness metadata".into()),
                }
                tokio::time::sleep(Duration::from_millis(10)).await;
            }
        })
        .await
        .unwrap_or_else(|_| Err("daemon readiness deadline".into()));
        match ready {
            Ok(inode) => Ok(inode),
            Err(original) => {
                let cleanup = kill_and_reap(child).await;
                let original = retain_first(original, cleanup);
                let unlink = match known {
                    Some(inode) => unlink_exited(child, socket, inode),
                    None => Ok(()), // Unknown path identity is always retained.
                };
                Err(retain_first(original, unlink))
            }
        }
    }
    pub(super) fn command(
        binary: &Path,
        url: &str,
        socket: &Path,
        contexts: Option<&Path>,
    ) -> Command {
        let mut command = Command::new(binary);
        Self::configure(&mut command, url, socket, contexts);
        command
    }
    pub(super) fn configure(
        command: &mut Command,
        url: &str,
        socket: &Path,
        contexts: Option<&Path>,
    ) {
        command
            .env_clear()
            .env("TECT_DATABASE_URL", url)
            .env("TECT_SOCKET", socket)
            .env("PATH", "/usr/bin:/bin")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .kill_on_drop(true);
        if let Some(contexts) = contexts {
            command
                .env("TECT_KNOWLEDGE_MAINTENANCE", "1")
                .env("TECT_KNOWLEDGE_SEARCH_CONTEXTS", contexts);
        }
    }
    /// Returns true only when the owned daemon had already exited.
    #[allow(dead_code)]
    pub async fn stop(&mut self) -> Result<bool, String> {
        let probe = self.child.try_wait();
        let already_exited = matches!(&probe, Ok(Some(_)));
        kill_and_reap_with_probe(&mut self.child, probe).await?;
        Ok(already_exited)
    }
    #[allow(dead_code)]
    pub fn unlink_after_stop(&mut self) -> Result<(), String> {
        unlink_exited(&mut self.child, &self.socket, self.inode)
    }
    pub async fn crash(&mut self) {
        let cleanup = kill_and_reap(&mut self.child).await;
        cleanup.unwrap();
        assert!(
            self.child
                .try_wait()
                .unwrap()
                .is_some_and(|exit| !exit.success()),
            "owned crash exit"
        );
    }
    pub fn remove_owned_stale_socket(&mut self) {
        self.unlink_after_stop().unwrap();
    }
}
fn require_absent(path: &Path) -> Result<(), String> {
    match fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err("daemon socket must be absent before spawn".into()),
    }
}
fn unlink_exited(child: &mut Child, socket: &Path, inode: (u64, u64)) -> Result<(), String> {
    if child
        .try_wait()
        .map_err(|_| "daemon cleanup status")?
        .is_none()
    {
        return Err("never clean a live child's socket".into());
    }
    let metadata = match fs::symlink_metadata(socket) {
        Ok(metadata) => metadata,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => return Ok(()),
        Err(_) => return Err("socket metadata".into()),
    };
    if !metadata.file_type().is_socket() || (metadata.dev(), metadata.ino()) != inode {
        return Err("owned socket was replaced; left untouched".into());
    }
    fs::remove_file(socket).map_err(|_| "socket unlink".into())
}
