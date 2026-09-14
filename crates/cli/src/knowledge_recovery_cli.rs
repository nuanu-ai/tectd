use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use tect_domain::{Error, Result};
use tect_postgres::{
    KnowledgeAdminPool, KnowledgeSuppressionCheckpoint, apply_knowledge_suppression_manifest,
    knowledge_suppression_manifest_bytes, parse_knowledge_suppression_manifest,
    prepare_knowledge_suppression_manifest, record_knowledge_suppression_export,
};
use uuid::Uuid;

pub async fn export(pool: &KnowledgeAdminPool, output: &Path) -> Result<()> {
    validate_absolute_file_path(output)?;
    let manifest = prepare_knowledge_suppression_manifest(pool).await?;
    let bytes = knowledge_suppression_manifest_bytes(&manifest)?;
    if output.exists() {
        let metadata =
            std::fs::symlink_metadata(output).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.file_type().is_symlink()
            || !metadata.is_file()
            || metadata.permissions().mode() & 0o777 != 0o600
        {
            return Err(Error::InvalidConfiguration);
        }
        let existing = std::fs::read(output).map_err(|_| Error::InvalidConfiguration)?;
        let parsed = parse_knowledge_suppression_manifest(&existing)?;
        if existing != bytes || parsed != manifest {
            return Err(Error::InvalidConfiguration);
        }
    } else {
        durable_new_file(output, &bytes)?;
    }
    let checkpoint = record_knowledge_suppression_export(pool, &manifest).await?;
    println!(
        "{}",
        serde_json::to_string(&serde_json::json!({
            "manifest_path": output,
            "checkpoint": checkpoint
        }))
        .map_err(|_| Error::InternalInvariant)?
    );
    Ok(())
}

pub async fn apply(
    pool: &KnowledgeAdminPool,
    input: &Path,
    expected_lineage: Uuid,
    expected_sequence: i64,
    expected_digest: String,
) -> Result<()> {
    validate_absolute_file_path(input)?;
    let metadata = std::fs::symlink_metadata(input).map_err(|_| Error::InvalidConfiguration)?;
    if metadata.file_type().is_symlink() || !metadata.is_file() {
        return Err(Error::InvalidConfiguration);
    }
    let bytes = std::fs::read(input).map_err(|_| Error::InvalidConfiguration)?;
    let manifest = parse_knowledge_suppression_manifest(&bytes)?;
    let expected = KnowledgeSuppressionCheckpoint {
        database_lineage_id: expected_lineage,
        erasure_sequence: expected_sequence,
        manifest_digest: expected_digest,
    };
    let report = apply_knowledge_suppression_manifest(pool, &manifest, &expected).await?;
    println!(
        "{}",
        serde_json::to_string(&report).map_err(|_| Error::InternalInvariant)?
    );
    Ok(())
}

fn durable_new_file(output: &Path, bytes: &[u8]) -> Result<()> {
    let parent = output.parent().ok_or(Error::InvalidArguments)?;
    verify_directory_chain(parent)?;
    let name = output
        .file_name()
        .and_then(|value| value.to_str())
        .ok_or(Error::InvalidArguments)?;
    let temporary = parent.join(format!(".{name}.{}.tmp", Uuid::new_v4()));
    let result = write_link_sync(&temporary, output, parent, bytes);
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn write_link_sync(temporary: &Path, output: &Path, parent: &Path, bytes: &[u8]) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)
        .map_err(|_| Error::InvalidConfiguration)?;
    file.write_all(bytes)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    std::fs::hard_link(temporary, output).map_err(|_| Error::InvalidConfiguration)?;
    File::open(parent)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    Ok(())
}

fn validate_absolute_file_path(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidArguments);
    }
    verify_directory_chain(path.parent().ok_or(Error::InvalidArguments)?)
}

fn verify_directory_chain(path: &Path) -> Result<()> {
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(Error::InvalidArguments);
            }
        }
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::InvalidConfiguration);
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn durable_file_is_new_mode_0600_and_never_overwritten() {
        let directory = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let output = directory.path().join("manifest.json");
        durable_new_file(&output, b"first\n").unwrap();
        let metadata = std::fs::symlink_metadata(&output).unwrap();
        assert!(metadata.is_file());
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(std::fs::read(&output).unwrap(), b"first\n");
        assert_eq!(
            durable_new_file(&output, b"second\n"),
            Err(Error::InvalidConfiguration)
        );
        assert_eq!(std::fs::read(&output).unwrap(), b"first\n");
    }

    #[test]
    fn relative_and_symlink_parent_paths_are_rejected() {
        assert_eq!(
            validate_absolute_file_path(Path::new("manifest.json")),
            Err(Error::InvalidArguments)
        );
        let directory = tempfile::tempdir_in(std::env::current_dir().unwrap()).unwrap();
        let target = directory.path().join("target");
        std::fs::create_dir(&target).unwrap();
        let link = directory.path().join("link");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert_eq!(
            validate_absolute_file_path(&link.join("manifest.json")),
            Err(Error::InvalidConfiguration)
        );
    }
}
