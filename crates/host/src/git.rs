use async_trait::async_trait;
use std::env;
use std::ffi::OsStr;
use std::fs;
use std::os::unix::ffi::OsStrExt;
use std::path::{Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tect_application::SourceInspector;
use tect_domain::{Error, MAX_SOURCE_PATH_BYTES, Result, SourceLocation};
use tokio::io::{AsyncRead, AsyncReadExt};
use tokio::process::Command;
use tokio::time::timeout;

const GIT_TIMEOUT: Duration = Duration::from_secs(5);
const MAX_GIT_OUTPUT_BYTES: usize = 16 * 1024;

#[derive(Debug, Default, Clone, Copy)]
pub struct GitSourceInspector;

#[async_trait]
impl SourceInspector for GitSourceInspector {
    async fn inspect(&self, path: &str, allowed_roots: &[String]) -> Result<SourceLocation> {
        if path.is_empty()
            || path.len() > MAX_SOURCE_PATH_BYTES
            || path.contains('\0')
            || !Path::new(path).is_absolute()
        {
            return Err(Error::InvalidSource);
        }
        let roots = canonical_allowed_roots(allowed_roots)?;
        let requested = canonical_directory(Path::new(path))?;
        require_allowed(&requested, &roots)?;

        let worktree = canonical_directory(Path::new(
            &git_rev_parse(&requested, "--show-toplevel").await?,
        ))?;
        let common_dir = canonical_directory(Path::new(
            &git_rev_parse(&requested, "--git-common-dir").await?,
        ))?;
        require_allowed(&worktree, &roots)?;
        require_allowed(&common_dir, &roots)?;

        Ok(SourceLocation {
            common_dir: bounded_utf8_path(&common_dir)?,
            worktree_path: bounded_utf8_path(&worktree)?,
        })
    }
}

fn canonical_allowed_roots(roots: &[String]) -> Result<Vec<PathBuf>> {
    if roots.is_empty() {
        return Err(Error::InvalidSource);
    }
    roots
        .iter()
        .map(|root| {
            if root.is_empty() || root.len() > MAX_SOURCE_PATH_BYTES {
                return Err(Error::InvalidSource);
            }
            let declared = Path::new(root);
            if !declared.is_absolute() {
                return Err(Error::InvalidSource);
            }
            let canonical = canonical_directory(declared)?;
            if canonical != declared {
                return Err(Error::InvalidSource);
            }
            Ok(canonical)
        })
        .collect()
}

fn canonical_directory(path: &Path) -> Result<PathBuf> {
    let canonical = fs::canonicalize(path).map_err(|_| Error::InvalidSource)?;
    if !canonical.is_absolute()
        || !fs::metadata(&canonical)
            .map_err(|_| Error::InvalidSource)?
            .is_dir()
    {
        return Err(Error::InvalidSource);
    }
    bounded_utf8_path(&canonical)?;
    Ok(canonical)
}

fn require_allowed(path: &Path, roots: &[PathBuf]) -> Result<()> {
    if roots.iter().any(|root| path.starts_with(root)) {
        Ok(())
    } else {
        Err(Error::InvalidSource)
    }
}

fn bounded_utf8_path(path: &Path) -> Result<String> {
    let value = path.to_str().ok_or(Error::InvalidSource)?;
    if value.is_empty() || value.len() > MAX_SOURCE_PATH_BYTES {
        return Err(Error::InvalidSource);
    }
    Ok(value.to_owned())
}

async fn git_rev_parse(directory: &Path, query: &str) -> Result<String> {
    let mut command = Command::new("git");
    command
        .arg("--no-optional-locks")
        .arg("-C")
        .arg(directory)
        .arg("rev-parse")
        .arg("--path-format=absolute")
        .arg(query)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true)
        .env("LC_ALL", "C");
    clear_git_environment(&mut command);

    let mut child = command.spawn().map_err(|_| Error::InvalidSource)?;
    let stdout = child.stdout.take().ok_or(Error::InvalidSource)?;
    let stderr = child.stderr.take().ok_or(Error::InvalidSource)?;
    let operation = async {
        let (stdout, stderr) = tokio::join!(read_bounded(stdout), read_bounded(stderr));
        let stdout = stdout?;
        let stderr = stderr?;
        if stdout.len() > MAX_GIT_OUTPUT_BYTES || stderr.len() > MAX_GIT_OUTPUT_BYTES {
            let _ = child.kill().await;
            let _ = child.wait().await;
            return Err(Error::InvalidSource);
        }
        let status = child.wait().await.map_err(|_| Error::InvalidSource)?;
        if !status.success() {
            return Err(Error::InvalidSource);
        }
        parse_git_path(stdout)
    };

    match timeout(GIT_TIMEOUT, operation).await {
        Ok(result) => result,
        Err(_) => {
            let _ = child.kill().await;
            let _ = child.wait().await;
            Err(Error::InvalidSource)
        }
    }
}

fn clear_git_environment(command: &mut Command) {
    for (name, _) in env::vars_os() {
        if os_bytes(&name).starts_with(b"GIT_") {
            command.env_remove(name);
        }
    }
}

fn os_bytes(value: &OsStr) -> &[u8] {
    value.as_bytes()
}

async fn read_bounded<R: AsyncRead + Unpin>(reader: R) -> Result<Vec<u8>> {
    let mut bytes = Vec::new();
    reader
        .take((MAX_GIT_OUTPUT_BYTES + 1) as u64)
        .read_to_end(&mut bytes)
        .await
        .map_err(|_| Error::InvalidSource)?;
    Ok(bytes)
}

fn parse_git_path(mut bytes: Vec<u8>) -> Result<String> {
    if bytes.last() == Some(&b'\n') {
        bytes.pop();
        if bytes.last() == Some(&b'\r') {
            bytes.pop();
        }
    }
    if bytes.is_empty() || bytes.contains(&0) || bytes.len() > MAX_SOURCE_PATH_BYTES {
        return Err(Error::InvalidSource);
    }
    String::from_utf8(bytes).map_err(|_| Error::InvalidSource)
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::process::Command as StdCommand;

    fn init_repository(path: &Path) {
        let status = StdCommand::new("git")
            .arg("init")
            .arg("--quiet")
            .arg(path)
            .status()
            .unwrap();
        assert!(status.success());
    }

    #[tokio::test]
    async fn inspector_returns_canonical_repository_identity() {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let repository = root.join("source");
        fs::create_dir(&repository).unwrap();
        init_repository(&repository);

        let location = GitSourceInspector
            .inspect(
                repository.to_str().unwrap(),
                &[root.to_str().unwrap().to_owned()],
            )
            .await
            .unwrap();
        assert_eq!(location.worktree_path, repository.to_str().unwrap());
        assert_eq!(
            location.common_dir,
            repository.join(".git").to_str().unwrap()
        );
    }

    #[tokio::test]
    async fn inspector_rejects_repository_outside_allowed_roots() {
        let repository_root = tempfile::tempdir().unwrap();
        let allowed_root = tempfile::tempdir().unwrap();
        let repository = repository_root.path().canonicalize().unwrap();
        init_repository(&repository);
        let allowed = allowed_root.path().canonicalize().unwrap();

        assert_eq!(
            GitSourceInspector
                .inspect(
                    repository.to_str().unwrap(),
                    &[allowed.to_str().unwrap().to_owned()],
                )
                .await,
            Err(Error::InvalidSource)
        );
    }
}
