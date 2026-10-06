//! An explicitly pinned pipeline artifact is the only trusted Matrix source.
//! Pipeline provenance/target are caller-supplied metadata and confer no trust.
use crate::storage_error;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::BTreeMap;
use tect_application::MatrixEvidenceValidator;
use tect_domain::{
    CommitmentEvidence, Error, EvidenceValidationOutcome, FactProvenance, MatrixEvidenceBinding,
    MatrixFact, OperationalFacts, ProtectedGuarantee, RequiredMatrixFact, Result,
};
use uuid::Uuid;

const SCHEMA: &str = "tect.matrix-operating-evidence/1";
const FORMAT: &str = "application/vnd.tect.matrix-operating-evidence+json;version=1";

/// Startup/operator supplied approval. Its SHA approves the exact payload bytes,
/// independently of the artifact's caller-supplied metadata and declared digest.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ApprovedMatrixEvidenceArtifact {
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub task_id: Uuid,
    pub task_revision: i64,
    pub artifact_id: Uuid,
    pub artifact_revision: i64,
    pub sha256: String,
    pub policy_version: String,
    pub max_age_seconds: i64,
}

impl ApprovedMatrixEvidenceArtifact {
    pub fn from_json(value: &str) -> Result<Self> {
        let approval: Self =
            serde_json::from_str(value).map_err(|_| Error::InvalidConfiguration)?;
        if approval.tenant_id.is_nil()
            || approval.workspace_id.is_nil()
            || approval.task_id.is_nil()
            || approval.artifact_id.is_nil()
            || approval.task_revision < 1
            || approval.artifact_revision < 1
            || approval.max_age_seconds < 1
            || approval.sha256.len() != 64
            || !approval.sha256.bytes().all(|b| b.is_ascii_hexdigit())
            || !valid_text(&approval.policy_version)
        {
            return Err(Error::InvalidConfiguration);
        }
        Ok(approval)
    }

    fn evidence_ref(&self) -> String {
        format!(
            "pipeline-evidence:{}@{}",
            self.artifact_id, self.artifact_revision
        )
    }
}

#[derive(Clone)]
pub struct PgMatrixEvidenceValidator {
    pool: PgPool,
    approval: ApprovedMatrixEvidenceArtifact,
}

impl PgMatrixEvidenceValidator {
    pub fn new(pool: PgPool, approval: ApprovedMatrixEvidenceArtifact) -> Self {
        Self { pool, approval }
    }

    async fn current_snapshot(&self) -> Result<OperatingSnapshot> {
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(self.approval.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let row: Option<(String, i64, String, String, String)> = sqlx::query_as(
            "SELECT digest,size,format,readiness,body FROM pipeline_evidence_artifacts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4",
        )
        .bind(self.approval.tenant_id)
        .bind(self.approval.workspace_id)
        .bind(self.approval.artifact_id)
        .bind(self.approval.artifact_revision)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        checked_snapshot(row, &self.approval)
    }

    async fn binding(
        &self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        if workspace_id != self.approval.workspace_id
            || task_id != self.approval.task_id
            || revision != self.approval.task_revision
            || evidence_ref != self.approval.evidence_ref()
        {
            return Err(Error::Forbidden);
        }
        let snapshot = self.current_snapshot().await?;
        snapshot.binding(&self.approval, fact, evidence_ref, now)
    }
}

fn checked_snapshot(
    row: Option<(String, i64, String, String, String)>,
    approval: &ApprovedMatrixEvidenceArtifact,
) -> Result<OperatingSnapshot> {
    let (digest, size, format, readiness, body) = row.ok_or(Error::Forbidden)?;
    if readiness != "ready"
        || format != FORMAT
        || size != body.len() as i64
        || !digest.eq_ignore_ascii_case(&approval.sha256)
        || format!("{:x}", Sha256::digest(body.as_bytes())) != approval.sha256.to_lowercase()
    {
        return Err(Error::Forbidden);
    }
    parse_snapshot(&body, approval)
}

impl OperatingSnapshot {
    fn binding(
        &self,
        approval: &ApprovedMatrixEvidenceArtifact,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        let observed = self.fact(fact)?;
        if observed.observed_at > now
            || observed.expires_at <= now
            || observed.expires_at <= observed.observed_at
            || now
                .checked_sub(observed.observed_at)
                .is_none_or(|age| age > approval.max_age_seconds)
        {
            return Err(Error::Forbidden);
        }
        Ok(MatrixEvidenceBinding {
            fact_path: fact.path.clone(),
            value_digest: fact.value_digest.clone(),
            evidence_ref: evidence_ref.into(),
            content_digest: approval.sha256.to_lowercase(),
            source: observed.source_ref.clone(),
            subject: format!("{}@{}", approval.task_id, approval.task_revision),
            observed_at: observed.observed_at,
            expires_at: observed.expires_at,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
    }
}

#[async_trait]
impl MatrixEvidenceValidator for PgMatrixEvidenceValidator {
    fn policy_version(&self) -> &str {
        &self.approval.policy_version
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
        self.binding(workspace_id, task_id, revision, fact, evidence_ref, now)
            .await
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
        let current = self
            .binding(
                workspace_id,
                task_id,
                revision,
                fact,
                &binding.evidence_ref,
                now,
            )
            .await?;
        if current != *binding {
            return Err(Error::Forbidden);
        }
        Ok(())
    }
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct Sourced<T> {
    fact: T,
    source_ref: String,
    observed_at: i64,
    expires_at: i64,
}

#[derive(Debug, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct OperatingSnapshot {
    schema: String,
    workspace_id: Uuid,
    task_id: Uuid,
    task_revision: i64,
    scale: Sourced<MatrixFact<String>>,
    criticality: Sourced<MatrixFact<String>>,
    affected_guarantees: Sourced<MatrixFact<Vec<ProtectedGuarantee>>>,
    actual_exposure: Sourced<MatrixFact<bool>>,
    urgent_repair: Sourced<MatrixFact<bool>>,
    operational_facts: Sourced<OperationalFacts>,
    #[serde(skip_serializing_if = "Option::is_none")]
    demand_commitment: Option<Sourced<MatrixFact<CommitmentEvidence>>>,
    #[serde(skip_serializing_if = "Option::is_none")]
    latency_commitment: Option<Sourced<MatrixFact<CommitmentEvidence>>>,
}

fn parse_snapshot(
    body: &str,
    approval: &ApprovedMatrixEvidenceArtifact,
) -> Result<OperatingSnapshot> {
    let original: serde_json::Value = serde_json::from_str(body).map_err(|_| Error::Forbidden)?;
    let snapshot: OperatingSnapshot =
        serde_json::from_value(original.clone()).map_err(|_| Error::Forbidden)?;
    if serde_json::to_value(&snapshot).map_err(|_| Error::Forbidden)? != original
        || snapshot.schema != SCHEMA
        || snapshot.workspace_id != approval.workspace_id
        || snapshot.task_id != approval.task_id
        || snapshot.task_revision != approval.task_revision
    {
        return Err(Error::Forbidden);
    }
    snapshot.all_facts()?;
    Ok(snapshot)
}

impl OperatingSnapshot {
    fn all_facts(&self) -> Result<BTreeMap<String, ObservedFact>> {
        let mut facts = BTreeMap::new();
        insert(&mut facts, "/envelope/scale", &self.scale.fact, &self.scale)?;
        insert(
            &mut facts,
            "/criticality",
            &self.criticality.fact,
            &self.criticality,
        )?;
        let mut guarantees = self.affected_guarantees.fact.clone();
        if let MatrixFact::Known { value, .. } = &mut guarantees {
            value.sort();
        }
        insert(
            &mut facts,
            "/affected_guarantees",
            &guarantees,
            &self.affected_guarantees,
        )?;
        insert(
            &mut facts,
            "/actual_exposure",
            &self.actual_exposure.fact,
            &self.actual_exposure,
        )?;
        insert(
            &mut facts,
            "/urgent_repair",
            &self.urgent_repair.fact,
            &self.urgent_repair,
        )?;
        match &self.operational_facts.fact {
            OperationalFacts::KnownEmpty { provenance } => {
                check_provenance(provenance)?;
                insert(
                    &mut facts,
                    "/envelope/operational_facts",
                    &self.operational_facts.fact,
                    &self.operational_facts,
                )?;
            }
            OperationalFacts::Reported { entries } if !entries.is_empty() => {
                for entry in entries {
                    if entry.name.trim().is_empty() || entry.name.len() > 256 {
                        return Err(Error::Forbidden);
                    }
                    let path = format!(
                        "/envelope/operational_facts/{}",
                        entry.name.replace('~', "~0").replace('/', "~1")
                    );
                    insert(&mut facts, &path, &entry.fact, &self.operational_facts)?;
                }
            }
            _ => return Err(Error::Forbidden),
        }
        if let Some(value) = &self.demand_commitment {
            insert(&mut facts, "/demand_commitment", &value.fact, value)?;
        }
        if let Some(value) = &self.latency_commitment {
            insert(&mut facts, "/latency_commitment", &value.fact, value)?;
        }
        Ok(facts)
    }

    fn fact(&self, fact: &RequiredMatrixFact) -> Result<ObservedFact> {
        let observed = self
            .all_facts()?
            .remove(&fact.path)
            .ok_or(Error::Forbidden)?;
        if observed.value_digest != fact.value_digest {
            return Err(Error::Forbidden);
        }
        Ok(observed)
    }
}

#[derive(Clone)]
struct ObservedFact {
    value_digest: String,
    source_ref: String,
    observed_at: i64,
    expires_at: i64,
}

fn insert<T: Serialize, S>(
    facts: &mut BTreeMap<String, ObservedFact>,
    path: &str,
    value: &T,
    source: &Sourced<S>,
) -> Result<()> {
    if !valid_text(&source.source_ref) || source.expires_at <= source.observed_at {
        return Err(Error::Forbidden);
    }
    let value_json = serde_json::to_value(value).map_err(|_| Error::Forbidden)?;
    // Unknown/conflicting values cannot be turned into evidence by pinning bytes.
    if value_json
        .get("state")
        .and_then(|v| v.as_str())
        .is_some_and(|state| state != "known" && state != "known_empty" && state != "reported")
    {
        return Err(Error::Forbidden);
    }
    if let Some(provenance) = value_json.get("provenance").and_then(|v| v.as_str()) {
        check_provenance(&FactProvenance(provenance.into()))?;
    }
    // Match required_matrix_facts: hash the typed serialization directly.
    // Round-tripping through Value can reorder object keys in tagged facts.
    let bytes = serde_json::to_vec(value).map_err(|_| Error::Forbidden)?;
    if facts
        .insert(
            path.into(),
            ObservedFact {
                value_digest: format!("{:x}", Sha256::digest(bytes)),
                source_ref: source.source_ref.clone(),
                observed_at: source.observed_at,
                expires_at: source.expires_at,
            },
        )
        .is_some()
    {
        return Err(Error::Forbidden);
    }
    Ok(())
}

fn check_provenance(provenance: &FactProvenance) -> Result<()> {
    provenance.validate().map_err(|_| Error::Forbidden)
}

fn valid_text(value: &str) -> bool {
    !value.trim().is_empty() && value.len() <= 256 && !value.chars().any(char::is_control)
}

#[cfg(test)]
#[path = "matrix_evidence_artifact_tests.rs"]
mod tests;
