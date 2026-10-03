//! A single bounded worker owns directory validation, serialization and persistence.
use super::*;
use rustix::fd::OwnedFd;
use rustix::fs::{self as unix, Mode, OFlags};
use std::fs::File;
use std::io::Write;
use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::mpsc::{self, Receiver, SyncSender};

const OUTPUT_CAP: usize = 65536;
const QUEUE_CAP: usize = 16;
pub(super) struct Job {
    pub trace: Arc<RequestTrace>,
    pub outcome: &'static str,
}
#[derive(Default)]
pub(super) struct Losses {
    pub queue: AtomicUsize,
}
pub(super) struct Worker {
    pub method: &'static str,
    pub sender: SyncSender<Job>,
    pub losses: Arc<Losses>,
}

pub(super) fn start(method: &'static str, directory: PathBuf) -> Option<Worker> {
    let (sender, receiver) = mpsc::sync_channel(QUEUE_CAP);
    let losses = Arc::new(Losses::default());
    let worker_losses = losses.clone();
    std::thread::Builder::new()
        .name("request-diagnostics".into())
        .spawn(move || {
            let Some(directory) = open_directory(&directory) else {
                eprintln!("request_diagnostics_sink_disabled");
                return;
            };
            eprintln!("request_diagnostics_sink_ready");
            run(directory, receiver, worker_losses);
        })
        .ok()?;
    Some(Worker {
        method,
        sender,
        losses,
    })
}

pub(super) fn open_directory(path: &Path) -> Option<OwnedFd> {
    if !path.is_absolute() {
        return None;
    }
    let flags = OFlags::RDONLY | OFlags::DIRECTORY | OFlags::NOFOLLOW | OFlags::CLOEXEC;
    let mut directory = unix::openat(unix::ABS, "/", flags, Mode::empty()).ok()?;
    for component in path.components() {
        match component {
            Component::RootDir => {}
            Component::Normal(name) => {
                directory = unix::openat(&directory, name, flags, Mode::empty()).ok()?;
            }
            _ => return None,
        }
    }
    let stat = unix::fstat(&directory).ok()?;
    if stat.st_mode & 0o7777 != 0o700 || stat.st_uid != rustix::process::geteuid().as_raw() {
        return None;
    }
    Some(directory)
}

fn run(directory: OwnedFd, receiver: Receiver<Job>, losses: Arc<Losses>) {
    loop {
        match receiver.recv_timeout(Duration::from_secs(1)) {
            Ok(job) => {
                if save(&directory, &job).is_none() {
                    eprintln!("request_diagnostics_sink_write_failed");
                }
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
        let lost = losses.queue.swap(0, Ordering::Relaxed);
        if lost != 0 {
            eprintln!("request_diagnostics_queue_dropped count={lost}");
        }
    }
}

pub(super) fn submit(worker: &Worker, job: Job) {
    if worker.sender.try_send(job).is_err() {
        worker.losses.queue.fetch_add(1, Ordering::Relaxed);
    }
}

pub(super) fn save(directory: &OwnedFd, job: &Job) -> Option<()> {
    let bytes = serde_json::to_vec(&job.trace.snapshot(job.outcome)).ok()?;
    if bytes.len() > OUTPUT_CAP {
        return None;
    }
    let name = format!("request-{}.json", job.trace.trace_id());
    let fd = unix::openat(
        directory,
        name.as_str(),
        OFlags::WRONLY | OFlags::CREATE | OFlags::EXCL | OFlags::NOFOLLOW | OFlags::CLOEXEC,
        Mode::RUSR | Mode::WUSR,
    )
    .ok()?;
    File::from(fd).write_all(&bytes).ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::PermissionsExt;

    #[test]
    fn retained_directory_cannot_redirect_after_path_replacement() {
        let root = tempfile::tempdir().unwrap();
        let original = root.path().join("original");
        fs::create_dir(&original).unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).unwrap();
        let handle = open_directory(&original.canonicalize().unwrap()).unwrap();
        let retained = root.path().join("retained");
        fs::rename(&original, &retained).unwrap();
        fs::create_dir(&original).unwrap();
        fs::set_permissions(&original, fs::Permissions::from_mode(0o700)).unwrap();
        let trace = RequestTrace::new("slice_pipeline_context");
        trace.finish();
        let name = format!("request-{}.json", trace.trace_id());
        save(
            &handle,
            &Job {
                trace,
                outcome: "ok",
            },
        )
        .unwrap();
        assert!(retained.join(&name).exists());
        assert!(!original.join(&name).exists());
        assert_eq!(
            fs::metadata(retained.join(name)).unwrap().mode() & 0o777,
            0o600
        );
    }

    #[test]
    fn private_directory_and_symlink_traversal_checks_remain_strict() {
        let directory = tempfile::tempdir().unwrap();
        let canonical = directory.path().canonicalize().unwrap();
        fs::set_permissions(&canonical, fs::Permissions::from_mode(0o700)).unwrap();
        assert!(open_directory(&canonical).is_some());
        fs::set_permissions(&canonical, fs::Permissions::from_mode(0o755)).unwrap();
        assert!(open_directory(&canonical).is_none());
        let link = directory.path().join("symlink");
        std::os::unix::fs::symlink(&canonical, &link).unwrap();
        assert!(open_directory(&link).is_none());
    }
}
