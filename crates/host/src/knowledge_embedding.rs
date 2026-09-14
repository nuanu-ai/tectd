use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::env;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::sync::Arc;
use std::time::Duration;
use tect_application::KnowledgeEmbeddingProvider;
use tect_domain::{
    Error, KnowledgeEmbeddingModelIdentity, KnowledgeEmbeddingPurpose, KnowledgeEmbeddingRequest,
    Result,
};
use tokio::io::{AsyncBufReadExt, AsyncReadExt, AsyncWriteExt, BufReader};
use tokio::process::{Child, ChildStdin, Command};
use tokio::sync::Mutex;
use tokio::time::timeout;
use uuid::Uuid;

pub const EMBEDDING_MODEL: &str = "intfloat/multilingual-e5-small";
pub const EMBEDDING_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
pub const EMBEDDING_RECIPE: &str = "title_v1";
pub const EMBEDDING_DIMENSIONS: usize = 384;
const PROTOCOL: &str = "tect-knowledge-embedding-v1";
const MAX_TEXT_BYTES: usize = 4 * 1024;
const MAX_REQUEST_BYTES: usize = 8 * 1024;
const MAX_RESPONSE_BYTES: usize = 64 * 1024;
const MAX_STDERR_BYTES: u64 = 64 * 1024;
const STARTUP_TIMEOUT: Duration = Duration::from_secs(30);
const INFERENCE_TIMEOUT: Duration = Duration::from_secs(20);
const PROVIDER_SLOT_TIMEOUT: Duration = Duration::from_secs(1);
const STDERR_DRAIN_TIMEOUT: Duration = Duration::from_secs(60);
const WORKER_SCRIPT: &str = include_str!("../python/knowledge_embedding_worker.py");

#[derive(Clone, Debug)]
pub struct LocalEmbeddingConfig {
    python_executable: PathBuf,
    model_dir: PathBuf,
    worker_script: Option<PathBuf>,
}

impl LocalEmbeddingConfig {
    pub fn from_env() -> Result<Option<Self>> {
        let python = env::var_os("TECT_KNOWLEDGE_EMBEDDING_PYTHON");
        let model = env::var_os("TECT_KNOWLEDGE_EMBEDDING_MODEL_DIR");
        match (python, model) {
            (None, None) => Ok(None),
            (Some(python), Some(model)) => {
                Self::new(PathBuf::from(python), PathBuf::from(model), None).map(Some)
            }
            _ => Err(Error::InvalidConfiguration),
        }
    }

    fn new(
        python_executable: PathBuf,
        model_dir: PathBuf,
        worker_script: Option<PathBuf>,
    ) -> Result<Self> {
        let python_executable = validate_executable(&python_executable)?;
        validate_path(&model_dir, PathKind::Directory)?;
        if let Some(worker_script) = &worker_script {
            validate_path(worker_script, PathKind::File)?;
        }
        Ok(Self {
            python_executable,
            model_dir,
            worker_script,
        })
    }

    #[cfg(test)]
    pub(crate) fn for_test(
        python_executable: PathBuf,
        model_dir: PathBuf,
        worker_script: PathBuf,
    ) -> Result<Self> {
        Self::new(python_executable, model_dir, Some(worker_script))
    }
}

enum PathKind {
    Directory,
    File,
}

fn validate_executable(path: &Path) -> Result<PathBuf> {
    validate_absolute(path)?;
    let target = fs::canonicalize(path).map_err(|_| Error::InvalidConfiguration)?;
    let metadata = fs::metadata(&target).map_err(|_| Error::InvalidConfiguration)?;
    if !metadata.is_file() || metadata.permissions().mode() & 0o111 == 0 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path.to_path_buf())
}

fn validate_path(path: &Path, kind: PathKind) -> Result<()> {
    validate_absolute(path)?;
    let metadata = fs::symlink_metadata(path).map_err(|_| Error::InvalidConfiguration)?;
    if metadata.file_type().is_symlink()
        || match kind {
            PathKind::Directory => !metadata.is_dir(),
            PathKind::File => !metadata.is_file(),
        }
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn validate_absolute(path: &Path) -> Result<()> {
    if path.is_absolute()
        && path
            .components()
            .all(|part| matches!(part, Component::RootDir | Component::Normal(_)))
    {
        Ok(())
    } else {
        Err(Error::InvalidConfiguration)
    }
}

pub struct LocalKnowledgeEmbeddingWorker {
    config: LocalEmbeddingConfig,
    process: Arc<Mutex<Option<WorkerProcess>>>,
}

impl LocalKnowledgeEmbeddingWorker {
    pub fn new(config: LocalEmbeddingConfig) -> Self {
        Self {
            config,
            process: Arc::new(Mutex::new(None)),
        }
    }

    pub async fn embed_query(&self, text: &str) -> Result<Vec<f32>> {
        self.embed(EmbeddingKind::Query, text, Uuid::new_v4()).await
    }

    pub async fn embed_title(&self, title: &str) -> Result<Vec<f32>> {
        self.embed(EmbeddingKind::Passage, title, Uuid::new_v4())
            .await
    }

    async fn embed(&self, kind: EmbeddingKind, text: &str, request_id: Uuid) -> Result<Vec<f32>> {
        validate_text(text)?;
        let mut process = timeout(PROVIDER_SLOT_TIMEOUT, self.process.lock())
            .await
            .map_err(|_| Error::TransportUnavailable)?;
        if process.is_none() {
            *process = Some(WorkerProcess::start(&self.config).await?);
        }
        let result = process
            .as_mut()
            .expect("worker initialized")
            .embed(kind, text, request_id)
            .await;
        if result.is_err()
            && let Some(mut failed) = process.take()
        {
            failed.stop().await;
        }
        result
    }
}

#[async_trait]
impl KnowledgeEmbeddingProvider for LocalKnowledgeEmbeddingWorker {
    fn model(&self) -> Option<KnowledgeEmbeddingModelIdentity> {
        Some(KnowledgeEmbeddingModelIdentity::pinned())
    }

    async fn embed(&self, request: &KnowledgeEmbeddingRequest) -> Result<Vec<f32>> {
        request.model.validate()?;
        let expected_prefix = match request.purpose {
            KnowledgeEmbeddingPurpose::Query => "query: ",
            KnowledgeEmbeddingPurpose::PassageTitle => "passage: ",
        };
        if !request.text.starts_with(expected_prefix)
            || format!("{:x}", Sha256::digest(request.text.as_bytes())) != request.input_digest
        {
            return Err(Error::KnowledgeUnavailable);
        }
        let kind = match request.purpose {
            KnowledgeEmbeddingPurpose::Query => EmbeddingKind::Query,
            KnowledgeEmbeddingPurpose::PassageTitle => EmbeddingKind::Passage,
        };
        self.embed(kind, &request.text, request.request_id).await
    }
}

fn validate_text(text: &str) -> Result<()> {
    if text.trim().is_empty() || text.contains('\0') || text.len() > MAX_TEXT_BYTES {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

#[derive(Clone, Copy, Serialize)]
#[serde(rename_all = "snake_case")]
enum EmbeddingKind {
    Query,
    Passage,
}

#[derive(Serialize)]
struct WorkerRequest<'a> {
    protocol: &'static str,
    request_id: String,
    kind: EmbeddingKind,
    text: &'a str,
}

#[derive(Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
enum WorkerResponse {
    Ready {
        protocol: String,
        model: String,
        revision: String,
        recipe: String,
        dimensions: usize,
    },
    Ok {
        protocol: String,
        request_id: String,
        model: String,
        revision: String,
        recipe: String,
        dimensions: usize,
        embedding: Vec<f32>,
    },
    Error {
        protocol: String,
        code: String,
    },
}

struct WorkerProcess {
    child: Child,
    stdin: ChildStdin,
    stdout: BufReader<tokio::process::ChildStdout>,
}

impl WorkerProcess {
    async fn start(config: &LocalEmbeddingConfig) -> Result<Self> {
        let mut command = Command::new(&config.python_executable);
        command.args(["-I", "-X", "utf8"]);
        if let Some(worker_script) = &config.worker_script {
            command.arg(worker_script);
        } else {
            command.arg("-c").arg(WORKER_SCRIPT);
        }
        command
            .arg("--model-dir")
            .arg(&config.model_dir)
            .env_clear()
            .env("PYTHONUNBUFFERED", "1")
            .env("PYTHONDONTWRITEBYTECODE", "1")
            .env("PYTHONUTF8", "1")
            .env("PYTHONNOUSERSITE", "1")
            .env("HF_HUB_OFFLINE", "1")
            .env("TRANSFORMERS_OFFLINE", "1")
            .env("TOKENIZERS_PARALLELISM", "false")
            .env("OMP_NUM_THREADS", "4")
            .env("MKL_NUM_THREADS", "4")
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .stderr(Stdio::piped())
            .kill_on_drop(true);
        command.current_dir("/");
        let mut child = command.spawn().map_err(|_| Error::InvalidConfiguration)?;
        let stdin = child.stdin.take().ok_or(Error::InvalidConfiguration)?;
        let stdout = child.stdout.take().ok_or(Error::InvalidConfiguration)?;
        let stderr = child.stderr.take().ok_or(Error::InvalidConfiguration)?;
        tokio::spawn(async move {
            let mut stderr = BufReader::new(stderr).take(MAX_STDERR_BYTES);
            let _ = timeout(
                STDERR_DRAIN_TIMEOUT,
                tokio::io::copy(&mut stderr, &mut tokio::io::sink()),
            )
            .await;
        });
        let mut process = Self {
            child,
            stdin,
            stdout: BufReader::new(stdout),
        };
        let ready = timeout(STARTUP_TIMEOUT, process.read_response())
            .await
            .map_err(|_| Error::TransportUnavailable)??;
        match ready {
            WorkerResponse::Ready {
                protocol,
                model,
                revision,
                recipe,
                dimensions,
            } if valid_identity(&protocol, &model, &revision, &recipe, dimensions) => Ok(process),
            _ => {
                process.stop().await;
                Err(Error::InvalidConfiguration)
            }
        }
    }

    async fn embed(
        &mut self,
        kind: EmbeddingKind,
        text: &str,
        request_id: Uuid,
    ) -> Result<Vec<f32>> {
        timeout(
            INFERENCE_TIMEOUT,
            self.embed_with_timeout(kind, text, request_id),
        )
        .await
        .map_err(|_| Error::TransportUnavailable)?
    }

    async fn embed_with_timeout(
        &mut self,
        kind: EmbeddingKind,
        text: &str,
        request_id: Uuid,
    ) -> Result<Vec<f32>> {
        let request_id = request_id.to_string();
        let request = WorkerRequest {
            protocol: PROTOCOL,
            request_id: request_id.clone(),
            kind,
            text,
        };
        let mut bytes = serde_json::to_vec(&request).map_err(|_| Error::TransportUnavailable)?;
        if bytes.len() >= MAX_REQUEST_BYTES {
            return Err(Error::InvalidArguments);
        }
        bytes.push(b'\n');
        self.stdin
            .write_all(&bytes)
            .await
            .map_err(|_| Error::TransportUnavailable)?;
        let response = self.read_response().await?;
        match response {
            WorkerResponse::Ok {
                protocol,
                request_id: response_id,
                model,
                revision,
                recipe,
                dimensions,
                embedding,
            } if response_id == request_id
                && valid_identity(&protocol, &model, &revision, &recipe, dimensions)
                && valid_embedding(&embedding) =>
            {
                Ok(embedding)
            }
            WorkerResponse::Error { protocol, code }
                if protocol == PROTOCOL && !code.is_empty() =>
            {
                Err(Error::TransportUnavailable)
            }
            _ => Err(Error::TransportUnavailable),
        }
    }

    async fn read_response(&mut self) -> Result<WorkerResponse> {
        let mut bytes = Vec::with_capacity(8 * 1024);
        loop {
            let available = self
                .stdout
                .fill_buf()
                .await
                .map_err(|_| Error::TransportUnavailable)?;
            if available.is_empty() {
                return Err(Error::TransportUnavailable);
            }
            let consumed = available
                .iter()
                .position(|byte| *byte == b'\n')
                .map_or(available.len(), |index| index + 1);
            if bytes.len() + consumed > MAX_RESPONSE_BYTES {
                return Err(Error::TransportUnavailable);
            }
            bytes.extend_from_slice(&available[..consumed]);
            self.stdout.consume(consumed);
            if bytes.last() == Some(&b'\n') {
                bytes.pop();
                return serde_json::from_slice(&bytes).map_err(|_| Error::TransportUnavailable);
            }
        }
    }

    async fn stop(&mut self) {
        let _ = self.child.kill().await;
        let _ = self.child.wait().await;
    }
}

fn valid_identity(
    protocol: &str,
    model: &str,
    revision: &str,
    recipe: &str,
    dimensions: usize,
) -> bool {
    protocol == PROTOCOL
        && model == EMBEDDING_MODEL
        && revision == EMBEDDING_REVISION
        && recipe == EMBEDDING_RECIPE
        && dimensions == EMBEDDING_DIMENSIONS
}

fn valid_embedding(embedding: &[f32]) -> bool {
    if embedding.len() != EMBEDDING_DIMENSIONS || !embedding.iter().all(|value| value.is_finite()) {
        return false;
    }
    let norm = embedding
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    (norm - 1.0).abs() <= 1e-3
}

#[cfg(test)]
mod tests;
