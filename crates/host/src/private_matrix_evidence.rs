//! Operator-pinned immutable evidence for an explicitly opted-in private DEV.
//! The pin is a trust decision, not an inference from a caller's claim.
use async_trait::async_trait;
use ring::signature::{ED25519, UnparsedPublicKey};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::{
    fs::{self, File, Metadata},
    io::Read,
    os::unix::fs::MetadataExt,
    path::{Component, Path, PathBuf},
    sync::Arc,
};
use tect_application::{MatrixEvidenceValidator, Store, TransactionMode};
use tect_domain::{
    EngineeringMatrixInput, Error, EvidenceValidationOutcome, HostAuth, MatrixEvidenceBinding,
    PrincipalRole, RequiredMatrixFact, Result, required_matrix_facts,
};
use uuid::Uuid;

const POLICY_SCHEMA: &str = "tect.dev-matrix-evidence-policy/1";
const EVIDENCE_SCHEMA: &str = "tect.dev-matrix-evidence/1";
const MAX_BYTES: u64 = 262_144;

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Policy {
    schema: String,
    policy_version: String,
    tenant_id: Uuid,
    workspace_id: Uuid,
    verifier_principal: Uuid,
    verifier_auth: HostAuth,
    verifier_public_key_hex: String,
    artifact_root: PathBuf,
    entries: Vec<Entry>,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Entry {
    file_name: String,
    task_id: Uuid,
    revision: i64,
    binding: MatrixEvidenceBinding,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Payload {
    schema: String,
    policy_version: String,
    workspace_id: Uuid,
    task_id: Uuid,
    revision: i64,
    verifier_principal: Uuid,
    source: String,
    subject: String,
    observed_at: i64,
    expires_at: i64,
    input: EngineeringMatrixInput,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Envelope {
    payload: Payload,
    signature_hex: String,
}

pub struct PrivateMatrixEvidenceValidator {
    policy: Policy,
    policy_path: PathBuf,
    policy_digest: String,
    store: Arc<dyn Store>,
}

impl PrivateMatrixEvidenceValidator {
    pub fn from_env(store: Arc<dyn Store>) -> Result<Option<Self>> {
        match std::env::var("TECT_DEV_MATRIX_EVIDENCE_POLICY_FILE") {
            Err(std::env::VarError::NotPresent) => Ok(None),
            Err(_) => Err(Error::InvalidConfiguration),
            Ok(value) => Self::from_file(PathBuf::from(value), store).map(Some),
        }
    }

    fn from_file(policy_path: PathBuf, store: Arc<dyn Store>) -> Result<Self> {
        let bytes = secure_read(&policy_path, 0o600)?;
        let policy: Policy =
            serde_json::from_slice(&bytes).map_err(|_| Error::InvalidConfiguration)?;
        if policy.schema != POLICY_SCHEMA
            || policy.policy_version.trim().is_empty()
            || policy.policy_version.len() > 256
            || policy.tenant_id.is_nil()
            || policy.workspace_id.is_nil()
            || policy.verifier_principal.is_nil()
            || policy.verifier_auth.host_id.is_nil()
            || decode_hex(&policy.verifier_auth.credential, 32).is_err()
            || policy.entries.is_empty()
            || policy.entries.len() > 100
            || decode_hex(&policy.verifier_public_key_hex, 32).is_err()
        {
            return Err(Error::InvalidConfiguration);
        }
        private_root(&policy.artifact_root)?;
        let mut refs = std::collections::BTreeSet::new();
        for entry in &policy.entries {
            if !basename(&entry.file_name)
                || entry.task_id.is_nil()
                || entry.revision < 1
                || entry.binding.evidence_ref.trim().is_empty()
                || entry.binding.evidence_ref.len() > 4096
                || !refs.insert(&entry.binding.evidence_ref)
                || entry.binding.validation_outcome != EvidenceValidationOutcome::Accepted
            {
                return Err(Error::InvalidConfiguration);
            }
        }
        Ok(Self {
            policy,
            policy_path,
            policy_digest: hash(&bytes),
            store,
        })
    }

    fn resolve_existing(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> Result<()> {
        let current = self.resolve(
            workspace_id,
            task_id,
            revision,
            fact,
            &binding.evidence_ref,
            now,
        )?;
        if current != *binding {
            return Err(Error::Forbidden);
        }
        Ok(())
    }

    async fn check_identity(&self) -> Result<()> {
        let mut tx = self.store.begin(TransactionMode::ReadOnly).await?;
        let identity = tx.authenticate(&self.policy.verifier_auth).await?;
        if identity.host_id != self.policy.verifier_auth.host_id
            || identity.tenant_id != self.policy.tenant_id
            || identity.principal_id != self.policy.verifier_principal
            || identity.role != PrincipalRole::Verifier
        {
            return Err(Error::Forbidden);
        }
        tx.set_tenant(identity.tenant_id).await?;
        if !tx
            .is_member(self.policy.workspace_id, identity.principal_id)
            .await?
        {
            return Err(Error::Forbidden);
        }
        tx.commit().await
    }

    fn resolve(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        if hash(&secure_read(&self.policy_path, 0o600)?) != self.policy_digest {
            return Err(Error::Forbidden);
        }
        private_root(&self.policy.artifact_root)?;
        let entry = self
            .policy
            .entries
            .iter()
            .find(|entry| entry.binding.evidence_ref == evidence_ref)
            .ok_or(Error::Forbidden)?;
        if workspace_id != self.policy.workspace_id
            || task_id != entry.task_id
            || revision != entry.revision
            || fact.path != entry.binding.fact_path
            || fact.value_digest != entry.binding.value_digest
        {
            return Err(Error::Forbidden);
        }
        let bytes = secure_read(&self.policy.artifact_root.join(&entry.file_name), 0o400)?;
        if hash(&bytes) != entry.binding.content_digest {
            return Err(Error::Forbidden);
        }
        let envelope: Envelope = serde_json::from_slice(&bytes).map_err(|_| Error::Forbidden)?;
        // Reject any fields ignored by nested enum deserializers.
        let original: serde_json::Value =
            serde_json::from_slice(&bytes).map_err(|_| Error::Forbidden)?;
        if serde_json::to_value(&envelope).map_err(|_| Error::Forbidden)? != original {
            return Err(Error::Forbidden);
        }
        let payload = &envelope.payload;
        let canonical = serde_json::to_value(payload).map_err(|_| Error::Forbidden)?;
        let signed = serde_json::to_vec(&canonical).map_err(|_| Error::Forbidden)?;
        UnparsedPublicKey::new(
            &ED25519,
            decode_hex(&self.policy.verifier_public_key_hex, 32)?,
        )
        .verify(&signed, &decode_hex(&envelope.signature_hex, 64)?)
        .map_err(|_| Error::Forbidden)?;
        if payload.schema != EVIDENCE_SCHEMA
            || payload.policy_version != self.policy.policy_version
            || payload.workspace_id != workspace_id
            || payload.task_id != task_id
            || payload.revision != revision
            || payload.verifier_principal != self.policy.verifier_principal
            || payload.source != entry.binding.source
            || payload.source.trim().is_empty()
            || payload.subject != entry.binding.subject
            || payload.subject.trim().is_empty()
            || payload.observed_at != entry.binding.observed_at
            || payload.expires_at != entry.binding.expires_at
            || payload.observed_at < 0
            || payload.observed_at > now
            || payload.expires_at <= now
            || payload.expires_at <= payload.observed_at
            || payload
                .expires_at
                .checked_sub(payload.observed_at)
                .is_none_or(|age| age > 3600)
        {
            return Err(Error::Forbidden);
        }
        if !required_matrix_facts(&payload.input)?
            .iter()
            .any(|required| required == fact)
        {
            return Err(Error::Forbidden);
        }
        Ok(entry.binding.clone())
    }
}

#[async_trait]
impl MatrixEvidenceValidator for PrivateMatrixEvidenceValidator {
    fn policy_version(&self) -> &str {
        &self.policy.policy_version
    }
    async fn validate(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        self.check_identity().await?;
        self.resolve(workspace_id, task_id, revision, fact, evidence_ref, now)
    }
    async fn revalidate(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> Result<()> {
        self.check_identity().await?;
        self.resolve_existing(workspace_id, task_id, revision, fact, binding, now)
    }
}

fn hash(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn decode_hex(value: &str, size: usize) -> Result<Vec<u8>> {
    if value.len() != size * 2 || !value.bytes().all(|b| b.is_ascii_hexdigit()) {
        return Err(Error::Forbidden);
    }
    (0..size)
        .map(|n| u8::from_str_radix(&value[n * 2..n * 2 + 2], 16).map_err(|_| Error::Forbidden))
        .collect()
}
fn basename(value: &str) -> bool {
    !value.is_empty()
        && value.len() <= 255
        && Path::new(value).components().count() == 1
        && matches!(
            Path::new(value).components().next(),
            Some(Component::Normal(_))
        )
}
fn uid() -> u32 {
    rustix::process::getuid().as_raw()
}
fn identity(meta: &Metadata) -> (u64, u64, u64, i64, i64, i64, i64, u32, u32, u64) {
    (
        meta.dev(),
        meta.ino(),
        meta.size(),
        meta.mtime(),
        meta.mtime_nsec(),
        meta.ctime(),
        meta.ctime_nsec(),
        meta.mode(),
        meta.uid(),
        meta.nlink(),
    )
}
// Track every ancestor identity, including the approved root, across file reads.
// Root-owned sticky /private/tmp is permitted above an owner-only private root.
fn ancestors(path: &Path) -> Result<Vec<(PathBuf, Metadata)>> {
    if !path.is_absolute()
        || path
            .components()
            .any(|c| !matches!(c, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::Forbidden);
    }
    let mut result = Vec::new();
    let mut current = PathBuf::from("/");
    for component in path.parent().ok_or(Error::Forbidden)?.components() {
        if let Component::Normal(name) = component {
            current.push(name);
        }
        let meta = fs::symlink_metadata(&current).map_err(|_| Error::Forbidden)?;
        let trusted_sticky = meta.uid() == 0 && meta.mode() & 0o1000 != 0;
        if !meta.is_dir()
            || (meta.uid() != uid() && meta.uid() != 0)
            || (meta.mode() & 0o022 != 0 && !trusted_sticky)
        {
            return Err(Error::Forbidden);
        }
        result.push((current.clone(), meta));
    }
    Ok(result)
}
fn private_root(path: &Path) -> Result<()> {
    ancestors(&path.join("probe"))?;
    let meta = fs::symlink_metadata(path).map_err(|_| Error::Forbidden)?;
    if !meta.is_dir() || meta.uid() != uid() || meta.mode() & 0o777 != 0o700 {
        return Err(Error::Forbidden);
    }
    Ok(())
}
fn secure_read(path: &Path, mode: u32) -> Result<Vec<u8>> {
    let parents = ancestors(path)?;
    let before = fs::symlink_metadata(path).map_err(|_| Error::Forbidden)?;
    if !before.is_file()
        || before.uid() != uid()
        || before.nlink() != 1
        || before.mode() & 0o777 != mode
        || before.size() > MAX_BYTES
    {
        return Err(Error::Forbidden);
    }
    // Resolve all parents through directory handles: O_NOFOLLOW on the leaf
    // alone would leave parent-component symlink races.
    let flags =
        rustix::fs::OFlags::RDONLY | rustix::fs::OFlags::CLOEXEC | rustix::fs::OFlags::NOFOLLOW;
    let mut dir = rustix::fs::open(
        "/",
        flags | rustix::fs::OFlags::DIRECTORY,
        rustix::fs::Mode::empty(),
    )
    .map_err(|_| Error::Forbidden)?;
    for component in path.parent().ok_or(Error::Forbidden)?.components() {
        if let Component::Normal(name) = component {
            dir = rustix::fs::openat(
                &dir,
                name,
                flags | rustix::fs::OFlags::DIRECTORY,
                rustix::fs::Mode::empty(),
            )
            .map_err(|_| Error::Forbidden)?;
        }
    }
    let mut file = File::from(
        rustix::fs::openat(
            &dir,
            path.file_name().ok_or(Error::Forbidden)?,
            flags,
            rustix::fs::Mode::empty(),
        )
        .map_err(|_| Error::Forbidden)?,
    );
    if identity(&file.metadata().map_err(|_| Error::Forbidden)?) != identity(&before) {
        return Err(Error::Forbidden);
    }
    let mut bytes = Vec::new();
    (&mut file)
        .take(MAX_BYTES + 1)
        .read_to_end(&mut bytes)
        .map_err(|_| Error::Forbidden)?;
    if bytes.len() as u64 != before.size()
        || identity(&file.metadata().map_err(|_| Error::Forbidden)?) != identity(&before)
        || identity(&fs::symlink_metadata(path).map_err(|_| Error::Forbidden)?) != identity(&before)
    {
        return Err(Error::Forbidden);
    }
    for (parent, before) in parents {
        let after = fs::symlink_metadata(parent).map_err(|_| Error::Forbidden)?;
        if (after.dev(), after.ino(), after.uid(), after.mode())
            != (before.dev(), before.ino(), before.uid(), before.mode())
        {
            return Err(Error::Forbidden);
        }
    }
    Ok(bytes)
}

#[cfg(test)]
#[path = "private_matrix_evidence_tests.rs"]
mod tests;
