use std::{
    fs,
    os::unix::fs::{FileTypeExt, MetadataExt, OpenOptionsExt},
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};
use tokio::process::{Child, Command};

pub struct EmbeddingDaemon {
    child: Child,
    socket: PathBuf,
    inode: (u64, u64),
}

impl EmbeddingDaemon {
    pub async fn start(
        database_url: &str,
        socket: PathBuf,
        python: &Path,
        model: &Path,
        contexts: Option<&Path>,
    ) -> Self {
        let log = fs::OpenOptions::new()
            .create_new(true)
            .write(true)
            .mode(0o600)
            .open(socket.with_extension("stderr"))
            .unwrap();
        let mut child = Command::new(env!("CARGO_BIN_EXE_tectd"));
        child
            .env("TECT_DATABASE_URL", database_url)
            .env("TECT_SOCKET", &socket)
            .env("TECT_KNOWLEDGE_EMBEDDING_PYTHON", python)
            .env("TECT_KNOWLEDGE_EMBEDDING_MODEL_DIR", model)
            .env("TECT_DK3_TEST_EMBEDDING", "1")
            .stdin(Stdio::null())
            .stdout(Stdio::null())
            .stderr(Stdio::from(log))
            .kill_on_drop(true);
        if let Some(contexts) = contexts {
            child.env("TECT_KNOWLEDGE_SEARCH_CONTEXTS", contexts);
        }
        let mut child = child.spawn().unwrap();
        tokio::time::timeout(Duration::from_secs(35), async {
            loop {
                assert!(
                    child.try_wait().unwrap().is_none(),
                    "owned embedding daemon exited during startup"
                );
                if fs::symlink_metadata(&socket).is_ok_and(|metadata| {
                    metadata.file_type().is_socket() && metadata.mode() & 0o777 == 0o600
                }) {
                    break;
                }
                tokio::time::sleep(Duration::from_millis(20)).await;
            }
        })
        .await
        .unwrap();
        let metadata = fs::symlink_metadata(&socket).unwrap();
        Self {
            child,
            socket,
            inode: (metadata.dev(), metadata.ino()),
        }
    }

    pub async fn stop(mut self) {
        self.child.start_kill().unwrap();
        let _ = self.child.wait().await.unwrap();
        let metadata = fs::symlink_metadata(&self.socket).unwrap();
        assert_eq!((metadata.dev(), metadata.ino()), self.inode);
        fs::remove_file(&self.socket).unwrap();
    }
}

pub fn write_worker_probe(path: &Path, real_python: &Path, counter: &Path) {
    use std::io::Write;
    let real = serde_json::to_string(&real_python.to_string_lossy()).unwrap();
    let count = serde_json::to_string(&counter.to_string_lossy()).unwrap();
    assert!(!real_python.to_string_lossy().contains('\n'));
    assert!(!real_python.to_string_lossy().contains('\r'));
    let body = format!(
        r#"#!{}
import json, os, subprocess, sys
REAL = {real}
COUNTER = {count}
child = subprocess.Popen([REAL] + sys.argv[1:], stdin=subprocess.PIPE,
                         stdout=subprocess.PIPE, stderr=subprocess.DEVNULL,
                         text=True, bufsize=1)
ready = child.stdout.readline()
if not ready:
    raise SystemExit(1)
sys.stdout.write(ready)
sys.stdout.flush()
counts = {{"query": 0, "passage": 0,
           "proxy_pid": os.getpid(), "worker_pid": child.pid}}
for line in sys.stdin:
    request = json.loads(line)
    kind = request.get("kind")
    if kind in counts:
        counts[kind] += 1
    temporary = COUNTER + ".tmp"
    with open(temporary, "w", encoding="utf-8") as handle:
        json.dump(counts, handle, sort_keys=True)
    os.chmod(temporary, 0o600)
    os.replace(temporary, COUNTER)
    child.stdin.write(line)
    child.stdin.flush()
    response = child.stdout.readline()
    if not response:
        raise SystemExit(1)
    sys.stdout.write(response)
    sys.stdout.flush()
child.terminate()
child.wait(timeout=5)
"#,
        real_python.display()
    );
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o700)
        .open(path)
        .unwrap();
    file.write_all(body.as_bytes()).unwrap();
}
