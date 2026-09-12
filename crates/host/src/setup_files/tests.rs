use super::*;
use sha2::{Digest, Sha256};
use std::fs;
use std::path::{Path, PathBuf};
use tect_application::SetupFiles;
use tect_domain::SetupDirectory;

mod inspection;
mod publication;

struct Fixture {
    _temporary: tempfile::TempDir,
    root: PathBuf,
    task: PathBuf,
    directory: SetupDirectory,
}

impl Fixture {
    fn new() -> Self {
        let temporary = tempfile::tempdir().unwrap();
        let root = temporary.path().canonicalize().unwrap();
        let task = root.join("task");
        fs::create_dir(&task).unwrap();
        let directory = adapter()
            .resolve_directory(path(&task), &[path(&root).to_owned()])
            .unwrap();
        Self {
            _temporary: temporary,
            root,
            task,
            directory,
        }
    }

    fn target(&self) -> PathBuf {
        self.task.join("AGENTS.md")
    }
}

fn adapter() -> LocalSetupFiles {
    LocalSetupFiles
}

fn path(value: &Path) -> &str {
    value.to_str().unwrap()
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn stage_names(directory: &Path) -> Vec<String> {
    let mut names: Vec<_> = fs::read_dir(directory)
        .unwrap()
        .map(|entry| entry.unwrap().file_name().to_string_lossy().into_owned())
        .filter(|name| name.starts_with(".tectd-agents-") && name.ends_with(".tmp"))
        .collect();
    names.sort();
    names
}
