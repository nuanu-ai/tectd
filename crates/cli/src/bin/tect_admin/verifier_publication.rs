use super::preflight_output;
use std::fs::File;
use std::io::Write;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::{Component, Path};
use tect_domain::{Error, HostAuth, Result};
use uuid::Uuid;

/// Holds the private parent directory open so publication and cleanup never
/// resolve a replaced path or follow a directory symlink.
pub(super) struct PublishedAuthFile {
    directory: File,
    name: std::ffi::OsString,
    device: u64,
    inode: u64,
    armed: bool,
}

impl PublishedAuthFile {
    fn new(directory: File, name: std::ffi::OsString, file: &File) -> Result<Self> {
        let metadata = file.metadata().map_err(|_| Error::InvalidConfiguration)?;
        Ok(Self {
            directory,
            name,
            device: metadata.dev(),
            inode: metadata.ino(),
            armed: true,
        })
    }

    fn verify_owned(&self) -> Result<()> {
        let metadata = rustix::fs::statat(
            &self.directory,
            &self.name,
            rustix::fs::AtFlags::SYMLINK_NOFOLLOW,
        )
        .map_err(|_| Error::InvalidConfiguration)?;
        if metadata.st_dev as u64 != self.device || metadata.st_ino != self.inode {
            return Err(Error::InvalidConfiguration);
        }
        Ok(())
    }

    fn remove(&mut self) -> Result<()> {
        self.verify_owned()?;
        rustix::fs::unlinkat(&self.directory, &self.name, rustix::fs::AtFlags::empty())
            .map_err(|_| Error::InvalidConfiguration)?;
        self.armed = false;
        self.directory
            .sync_all()
            .map_err(|_| Error::InvalidConfiguration)
    }

    pub(super) fn retain(&mut self) {
        self.armed = false;
    }

    pub(super) fn resolve_commit(
        &mut self,
        decision: tect_postgres::admin::VerifierCommitDecision,
    ) -> Result<bool> {
        use tect_postgres::admin::VerifierCommitDecision;
        match decision {
            VerifierCommitDecision::RecoverSuccess => {
                self.retain();
                Ok(true)
            }
            VerifierCommitDecision::RemoveCredential => {
                self.remove()?;
                Ok(false)
            }
            VerifierCommitDecision::PreserveCredential => {
                self.retain();
                Ok(false)
            }
        }
    }
}

impl Drop for PublishedAuthFile {
    fn drop(&mut self) {
        if self.armed {
            let _ = self.remove();
        }
    }
}

fn private_output_directory(path: &Path) -> Result<File> {
    let mut directory = File::open("/").map_err(|_| Error::InvalidConfiguration)?;
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(name) => {
                directory = rustix::fs::openat(
                    &directory,
                    name,
                    rustix::fs::OFlags::RDONLY
                        | rustix::fs::OFlags::DIRECTORY
                        | rustix::fs::OFlags::NOFOLLOW
                        | rustix::fs::OFlags::CLOEXEC,
                    rustix::fs::Mode::empty(),
                )
                .map(File::from)
                .map_err(|_| Error::InvalidConfiguration)?;
            }
            _ => return Err(Error::InvalidArguments),
        }
    }
    let metadata = directory
        .metadata()
        .map_err(|_| Error::InvalidConfiguration)?;
    if metadata.permissions().mode() & 0o777 != 0o700
        || metadata.uid() != rustix::process::geteuid().as_raw()
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(directory)
}

pub(super) fn publish_verifier_auth_file(
    path: &Path,
    auth: &HostAuth,
) -> Result<PublishedAuthFile> {
    preflight_output(path)?;
    let parent = path.parent().ok_or(Error::InvalidConfiguration)?;
    let name = path.file_name().ok_or(Error::InvalidConfiguration)?;
    let directory = private_output_directory(parent)?;
    let temporary_directory = directory
        .try_clone()
        .map_err(|_| Error::InvalidConfiguration)?;
    let temporary_name = std::ffi::OsString::from(format!(".verifier.{}.tmp", Uuid::new_v4()));
    let mut file = rustix::fs::openat(
        &directory,
        &temporary_name,
        rustix::fs::OFlags::WRONLY
            | rustix::fs::OFlags::CREATE
            | rustix::fs::OFlags::EXCL
            | rustix::fs::OFlags::NOFOLLOW
            | rustix::fs::OFlags::CLOEXEC,
        rustix::fs::Mode::from_raw_mode(0o600),
    )
    .map(File::from)
    .map_err(|_| Error::InvalidConfiguration)?;
    let mut temporary_guard = PublishedAuthFile::new(temporary_directory, temporary_name, &file)?;
    file.set_permissions(std::fs::Permissions::from_mode(0o600))
        .map_err(|_| Error::InvalidConfiguration)?;
    let mut contents = serde_json::to_vec_pretty(auth).map_err(|_| Error::InvalidConfiguration)?;
    contents.push(b'\n');
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    temporary_guard.verify_owned()?;
    rustix::fs::linkat(
        &directory,
        &temporary_guard.name,
        &directory,
        name,
        rustix::fs::AtFlags::empty(),
    )
    .map_err(|_| Error::InvalidConfiguration)?;
    let destination_guard = PublishedAuthFile {
        directory,
        name: name.to_owned(),
        device: temporary_guard.device,
        inode: temporary_guard.inode,
        armed: true,
    };
    destination_guard.verify_owned()?;
    destination_guard
        .directory
        .sync_all()
        .map_err(|_| Error::InvalidConfiguration)?;
    temporary_guard.remove()?;
    Ok(destination_guard)
}

#[cfg(test)]
mod tests {
    use super::super::{Arguments, Command};
    use super::*;
    use clap::Parser;
    use std::path::PathBuf;

    fn fake_auth() -> HostAuth {
        HostAuth {
            host_id: Uuid::new_v4(),
            credential: "0".repeat(64),
        }
    }

    fn private_fixture() -> (tempfile::TempDir, PathBuf) {
        let fixture = tempfile::tempdir().unwrap();
        let path = std::fs::canonicalize(fixture.path()).unwrap();
        std::fs::set_permissions(&path, std::fs::Permissions::from_mode(0o700)).unwrap();
        (fixture, path)
    }

    #[test]
    fn verifier_enrollment_requires_explicit_existing_tenant_workspace_and_output() {
        let tenant = Uuid::new_v4().to_string();
        let workspace = Uuid::new_v4().to_string();
        let args = Arguments::try_parse_from([
            "tect-admin",
            "enroll-verifier",
            "--tenant",
            &tenant,
            "--workspace",
            &workspace,
            "--out",
            "/private/example/verifier.json",
        ])
        .unwrap();
        assert!(matches!(args.command, Command::EnrollVerifier { .. }));
        assert!(
            Arguments::try_parse_from([
                "tect-admin",
                "enroll-verifier",
                "--tenant",
                &tenant,
                "--out",
                "/private/example/verifier.json",
            ])
            .is_err()
        );
    }

    #[test]
    fn verifier_publication_is_private_exclusive_and_precommit_cleanup_is_owned() {
        let (_fixture, directory) = private_fixture();
        let path = directory.join("verifier.json");
        let auth = fake_auth();
        let published = publish_verifier_auth_file(&path, &auth).unwrap();
        let metadata = std::fs::symlink_metadata(&path).unwrap();
        assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
        assert_eq!(metadata.uid(), rustix::process::geteuid().as_raw());
        let decoded: HostAuth = serde_json::from_slice(&std::fs::read(&path).unwrap()).unwrap();
        assert_eq!(decoded.host_id, auth.host_id);
        assert!(publish_verifier_auth_file(&path, &fake_auth()).is_err());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 1);
        drop(published);
        assert!(!path.exists());
        assert_eq!(std::fs::read_dir(&directory).unwrap().count(), 0);
    }

    #[test]
    fn verifier_publication_rejects_symlink_destination_parent_and_public_directory() {
        let (_fixture, directory) = private_fixture();
        let target = directory.join("target.json");
        std::fs::write(&target, b"fake existing content").unwrap();
        let link = directory.join("link.json");
        std::os::unix::fs::symlink(&target, &link).unwrap();
        assert!(publish_verifier_auth_file(&link, &fake_auth()).is_err());
        let alias = directory.join("alias");
        std::os::unix::fs::symlink(&directory, &alias).unwrap();
        assert!(publish_verifier_auth_file(&alias.join("new.json"), &fake_auth()).is_err());
        std::fs::set_permissions(&directory, std::fs::Permissions::from_mode(0o755)).unwrap();
        assert!(publish_verifier_auth_file(&directory.join("new.json"), &fake_auth()).is_err());
        assert_eq!(std::fs::read(&target).unwrap(), b"fake existing content");
    }

    #[test]
    fn verifier_cleanup_preserves_a_replacement_inode() {
        let (_fixture, directory) = private_fixture();
        let path = directory.join("verifier.json");
        let mut published = publish_verifier_auth_file(&path, &fake_auth()).unwrap();
        let replacement = directory.join("replacement.json");
        std::fs::write(&replacement, b"fake replacement").unwrap();
        std::fs::rename(&replacement, &path).unwrap();
        assert_eq!(published.remove(), Err(Error::InvalidConfiguration));
        drop(published);
        assert_eq!(std::fs::read(&path).unwrap(), b"fake replacement");
    }

    #[test]
    fn verifier_cleanup_uses_pinned_parent_after_path_replacement() {
        let (_fixture, directory) = private_fixture();
        let original = directory.join("original");
        std::fs::create_dir(&original).unwrap();
        std::fs::set_permissions(&original, std::fs::Permissions::from_mode(0o700)).unwrap();
        let mut published =
            publish_verifier_auth_file(&original.join("verifier.json"), &fake_auth()).unwrap();
        let moved = directory.join("moved");
        std::fs::rename(&original, &moved).unwrap();
        std::fs::create_dir(&original).unwrap();
        std::fs::write(original.join("verifier.json"), b"fake foreign replacement").unwrap();
        published.remove().unwrap();
        assert!(!moved.join("verifier.json").exists());
        assert_eq!(
            std::fs::read(original.join("verifier.json")).unwrap(),
            b"fake foreign replacement"
        );
    }

    #[test]
    fn verifier_uncertain_commit_and_interrupted_acknowledgement_preserve_credential() {
        use tect_postgres::admin::VerifierEnrollmentState;
        for state in [
            None,
            Some(VerifierEnrollmentState::Absent),
            Some(VerifierEnrollmentState::Inconsistent),
        ] {
            let (_fixture, directory) = private_fixture();
            let path = directory.join("verifier.json");
            let mut published = publish_verifier_auth_file(&path, &fake_auth()).unwrap();
            published.retain();
            let decision = tect_postgres::admin::resolve_verifier_commit(false, state);
            assert!(!published.resolve_commit(decision).unwrap());
            drop(published);
            assert!(path.is_file());
        }
        let (_fixture, directory) = private_fixture();
        let path = directory.join("interrupted.json");
        let mut published = publish_verifier_auth_file(&path, &fake_auth()).unwrap();
        published.retain();
        drop(published);
        assert!(path.is_file());
    }

    #[test]
    fn verifier_fresh_readback_recovers_success_or_removes_only_proven_rollback() {
        use tect_postgres::admin::VerifierEnrollmentState;
        let (_fixture, directory) = private_fixture();
        let recovered = directory.join("recovered.json");
        let mut published = publish_verifier_auth_file(&recovered, &fake_auth()).unwrap();
        published.retain();
        let decision = tect_postgres::admin::resolve_verifier_commit(
            false,
            Some(VerifierEnrollmentState::Committed),
        );
        assert!(published.resolve_commit(decision).unwrap());
        drop(published);
        assert!(recovered.is_file());
        let rollback = directory.join("rollback.json");
        let mut published = publish_verifier_auth_file(&rollback, &fake_auth()).unwrap();
        published.retain();
        let decision = tect_postgres::admin::resolve_verifier_commit(
            true,
            Some(VerifierEnrollmentState::Absent),
        );
        assert!(!published.resolve_commit(decision).unwrap());
        drop(published);
        assert!(!rollback.exists());
    }
}
