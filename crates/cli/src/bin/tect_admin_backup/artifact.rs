use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::{Read, Write};
use std::os::unix::fs::{DirBuilderExt, OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use tect_domain::{Error, Result};

pub const FORMAT: &str = "tectd-logical-backup";
pub const VERSION: u32 = 1;
const MANIFEST: &str = "manifest.json";
const MAX_MANIFEST: u64 = 1_048_576;

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct FileRecord {
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct GraphRecord {
    pub iri: String,
    pub native_digest: String,
    pub file: String,
    pub size: u64,
    pub sha256: String,
}

#[derive(Debug, Deserialize, Serialize)]
#[serde(deny_unknown_fields)]
pub struct Manifest {
    pub format: String,
    pub version: u32,
    pub source_database: String,
    pub runtime_role: String,
    pub postgres_version_num: i32,
    pub pgrdf_version: String,
    pub pgrdf_build_id: String,
    pub schema_version: i64,
    pub application_dump: FileRecord,
    pub graphs: Vec<GraphRecord>,
}

pub struct LoadedBundle {
    pub dump: PathBuf,
    pub graphs: Vec<tect_postgres::admin::RestoreGraph>,
}

pub fn create_bundle(path: &Path) -> Result<()> {
    validate_new_absolute(path)?;
    let parent = path.parent().ok_or(Error::InvalidArguments)?;
    let metadata = verify_directory_chain(parent)?;
    if metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(Error::InvalidConfiguration);
    }
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(path)
        .map_err(|_| Error::InvalidConfiguration)?;
    sync_directory(parent)
}

pub fn create_graph_directory(bundle: &Path) -> Result<PathBuf> {
    let path = bundle.join("graphs");
    let mut builder = std::fs::DirBuilder::new();
    builder.mode(0o700);
    builder
        .create(&path)
        .map_err(|_| Error::InvalidConfiguration)?;
    sync_directory(bundle)?;
    Ok(path)
}

pub fn sync_graph_directory(path: &Path) -> Result<()> {
    verify_directory_chain(path)?;
    sync_directory(path)
}

pub fn validate_native_digest(value: &str) -> Result<()> {
    if value.len() != 64
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

pub fn write_private(path: &Path, contents: &[u8]) -> Result<FileRecord> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .map_err(|_| Error::InvalidConfiguration)?;
    file.write_all(contents)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::StorageUnavailable)?;
    Ok(FileRecord {
        file: relative_name(path)?,
        size: contents.len() as u64,
        sha256: hex(&Sha256::digest(contents)),
    })
}

pub fn seal_external_file(bundle: &Path, path: &Path) -> Result<FileRecord> {
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::StorageUnavailable)?;
    if !metadata.file_type().is_file() {
        return Err(Error::InvalidConfiguration);
    }
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::StorageUnavailable)?;
    File::open(path)
        .and_then(|file| file.sync_all())
        .map_err(|_| Error::StorageUnavailable)?;
    sync_directory(bundle)?;
    let (size, sha256) = hash_file(path)?;
    Ok(FileRecord {
        file: relative_name(path)?,
        size,
        sha256,
    })
}

pub fn publish_manifest(bundle: &Path, manifest: &Manifest) -> Result<()> {
    let mut contents = serde_json::to_vec_pretty(manifest).map_err(|_| Error::InternalInvariant)?;
    contents.push(b'\n');
    let temporary = bundle.join(".manifest.tmp");
    let destination = bundle.join(MANIFEST);
    write_private(&temporary, &contents)?;
    std::fs::hard_link(&temporary, &destination).map_err(|_| Error::InvalidConfiguration)?;
    File::open(&destination)
        .and_then(|file| file.sync_all())
        .map_err(|_| Error::StorageUnavailable)?;
    let _ = std::fs::remove_file(&temporary);
    sync_directory(bundle)
}

pub fn load_bundle(path: &Path, runtime_role: &str, schema_version: i64) -> Result<LoadedBundle> {
    validate_existing_bundle(path)?;
    let manifest_path = path.join(MANIFEST);
    let metadata = private_regular_file(&manifest_path)?;
    if metadata.len() > MAX_MANIFEST {
        return Err(Error::InvalidConfiguration);
    }
    let mut bytes = Vec::with_capacity(metadata.len() as usize);
    File::open(&manifest_path)
        .and_then(|mut file| file.read_to_end(&mut bytes))
        .map_err(|_| Error::StorageUnavailable)?;
    let manifest: Manifest =
        serde_json::from_slice(&bytes).map_err(|_| Error::InvalidConfiguration)?;
    if manifest.format != FORMAT
        || manifest.version != VERSION
        || manifest.runtime_role != runtime_role
        || manifest.postgres_version_num / 10_000 != 18
        || manifest.pgrdf_version != "0.6.34"
        || manifest.pgrdf_build_id != "v0.6.34"
        || manifest.schema_version != schema_version
        || manifest.source_database.is_empty()
        || manifest.application_dump.file != "application.dump"
    {
        return Err(Error::InvalidConfiguration);
    }
    let mut files = HashSet::new();
    files.insert(manifest.application_dump.file.clone());
    let dump = verified_file(path, &manifest.application_dump)?;
    let mut iris = HashSet::new();
    let mut graphs = Vec::with_capacity(manifest.graphs.len());
    for graph in &manifest.graphs {
        if graph.iri.is_empty()
            || graph.iri.starts_with("urn:tect:dk:scratch:")
            || !iris.insert(graph.iri.clone())
            || !files.insert(graph.file.clone())
        {
            return Err(Error::InvalidConfiguration);
        }
        validate_native_digest(&graph.native_digest)?;
        let record = FileRecord {
            file: graph.file.clone(),
            size: graph.size,
            sha256: graph.sha256.clone(),
        };
        let graph_path = verified_file(path, &record)?;
        let payload =
            std::fs::read_to_string(graph_path).map_err(|_| Error::InvalidConfiguration)?;
        graphs.push(tect_postgres::admin::RestoreGraph {
            iri: graph.iri.clone(),
            native_digest: graph.native_digest.clone(),
            payload,
        });
    }
    Ok(LoadedBundle { dump, graphs })
}

fn verified_file(root: &Path, record: &FileRecord) -> Result<PathBuf> {
    if record.sha256.len() != 64 || !record.sha256.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::InvalidConfiguration);
    }
    let relative = safe_relative(&record.file)?;
    let path = root.join(relative);
    let metadata = private_regular_file(&path)?;
    let (size, digest) = hash_file(&path)?;
    if metadata.len() != record.size || size != record.size || digest != record.sha256 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path)
}

fn validate_new_absolute(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidArguments);
    }
    match std::fs::symlink_metadata(path) {
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => Ok(()),
        _ => Err(Error::InvalidConfiguration),
    }
}

fn validate_existing_bundle(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidArguments);
    }
    let metadata = verify_directory_chain(path)?;
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
            _ => return Err(Error::InvalidArguments),
        }
        final_metadata =
            std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if final_metadata.file_type().is_symlink() || !final_metadata.is_dir() {
            return Err(Error::InvalidConfiguration);
        }
    }
    Ok(final_metadata)
}

fn private_regular_file(path: &Path) -> Result<std::fs::Metadata> {
    let parent = path.parent().ok_or(Error::InvalidConfiguration)?;
    verify_directory_chain(parent)?;
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::InvalidConfiguration)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(metadata)
}

fn safe_relative(value: &str) -> Result<PathBuf> {
    let path = Path::new(value);
    if value.is_empty()
        || path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::Normal(_)))
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path.to_path_buf())
}

fn relative_name(path: &Path) -> Result<String> {
    let name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::InvalidConfiguration)?;
    Ok(name.to_owned())
}

fn hash_file(path: &Path) -> Result<(u64, String)> {
    let mut file = File::open(path).map_err(|_| Error::StorageUnavailable)?;
    let mut digest = Sha256::new();
    let mut size = 0_u64;
    let mut buffer = [0_u8; 64 * 1024];
    loop {
        let count = file
            .read(&mut buffer)
            .map_err(|_| Error::StorageUnavailable)?;
        if count == 0 {
            break;
        }
        size = size
            .checked_add(count as u64)
            .ok_or(Error::CapacityExceeded)?;
        digest.update(&buffer[..count]);
    }
    Ok((size, hex(&digest.finalize())))
}

fn sync_directory(path: &Path) -> Result<()> {
    File::open(path)
        .and_then(|directory| directory.sync_all())
        .map_err(|_| Error::StorageUnavailable)
}

fn hex(bytes: &[u8]) -> String {
    const HEX: &[u8; 16] = b"0123456789abcdef";
    let mut value = String::with_capacity(bytes.len() * 2);
    for byte in bytes {
        value.push(HEX[(byte >> 4) as usize] as char);
        value.push(HEX[(byte & 0x0f) as usize] as char);
    }
    value
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::os::unix::fs::symlink;

    fn fixture() -> (tempfile::TempDir, PathBuf) {
        let temp = tempfile::tempdir().unwrap();
        std::fs::set_permissions(temp.path(), std::fs::Permissions::from_mode(0o700)).unwrap();
        let bundle = temp.path().canonicalize().unwrap().join("backup");
        create_bundle(&bundle).unwrap();
        let graphs = create_graph_directory(&bundle).unwrap();
        let dump = write_private(&bundle.join("application.dump"), b"dump").unwrap();
        let graph =
            write_private(&graphs.join("graph-0.nt"), b"<urn:s> <urn:p> <urn:o> .\n").unwrap();
        publish_manifest(
            &bundle,
            &Manifest {
                format: FORMAT.into(),
                version: VERSION,
                source_database: "source".into(),
                runtime_role: "runtime_role".into(),
                postgres_version_num: 180006,
                pgrdf_version: "0.6.34".into(),
                pgrdf_build_id: "v0.6.34".into(),
                schema_version: 9,
                application_dump: dump,
                graphs: vec![GraphRecord {
                    iri: "urn:graph".into(),
                    native_digest:
                        "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855".into(),
                    file: "graphs/graph-0.nt".into(),
                    size: graph.size,
                    sha256: graph.sha256,
                }],
            },
        )
        .unwrap();
        (temp, bundle)
    }

    #[test]
    fn modified_payload_is_rejected_before_restore() {
        let (_temp, bundle) = fixture();
        assert!(load_bundle(&bundle, "runtime_role", 9).is_ok());
        std::fs::write(bundle.join("graphs/graph-0.nt"), b"changed").unwrap();
        assert_eq!(
            load_bundle(&bundle, "runtime_role", 9).err(),
            Some(Error::InvalidConfiguration)
        );
    }

    #[test]
    fn malformed_native_digest_is_rejected() {
        let (_temp, bundle) = fixture();
        let manifest_path = bundle.join(MANIFEST);
        let mut manifest: serde_json::Value =
            serde_json::from_slice(&std::fs::read(&manifest_path).unwrap()).unwrap();
        manifest["graphs"][0]["native_digest"] = serde_json::json!("ABC123");
        std::fs::write(&manifest_path, serde_json::to_vec(&manifest).unwrap()).unwrap();
        assert_eq!(
            load_bundle(&bundle, "runtime_role", 9).err(),
            Some(Error::InvalidConfiguration)
        );
    }

    #[test]
    fn symlinked_manifest_is_rejected() {
        let (temp, bundle) = fixture();
        let manifest = bundle.join(MANIFEST);
        let outside = temp.path().join("outside.json");
        std::fs::rename(&manifest, &outside).unwrap();
        symlink(&outside, &manifest).unwrap();
        assert_eq!(
            load_bundle(&bundle, "runtime_role", 9).err(),
            Some(Error::InvalidConfiguration)
        );
    }

    #[test]
    fn unknown_duplicate_and_missing_manifest_fields_are_rejected() {
        let (_temp, bundle) = fixture();
        let manifest_path = bundle.join(MANIFEST);
        let original = std::fs::read_to_string(&manifest_path).unwrap();

        let mut unknown: serde_json::Value = serde_json::from_str(&original).unwrap();
        unknown["unexpected"] = serde_json::json!(true);
        std::fs::write(&manifest_path, serde_json::to_vec(&unknown).unwrap()).unwrap();
        assert!(load_bundle(&bundle, "runtime_role", 9).is_err());

        let duplicate = original.replacen('{', "{\n  \"version\": 1,", 1);
        std::fs::write(&manifest_path, duplicate).unwrap();
        assert!(load_bundle(&bundle, "runtime_role", 9).is_err());

        let mut missing: serde_json::Value = serde_json::from_str(&original).unwrap();
        missing.as_object_mut().unwrap().remove("runtime_role");
        std::fs::write(&manifest_path, serde_json::to_vec(&missing).unwrap()).unwrap();
        assert!(load_bundle(&bundle, "runtime_role", 9).is_err());
    }
}
