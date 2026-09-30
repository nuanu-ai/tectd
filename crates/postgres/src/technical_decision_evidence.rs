//! Read-only resolution of separately approved technical evidence. Artifact
//! readiness and caller provenance never supply approval or owner authorship.
use crate::storage_error;
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::collections::BTreeSet;
use tect_application::{
    MatrixRequirementsLocator, ResolvedTechnicalDecisionEvidence,
    ServerTechnicalDecisionTaskBinding, TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION,
    TechnicalDecisionApprovalRecord, TechnicalDecisionCandidateMapping,
    TechnicalDecisionEvidenceReference, TechnicalDecisionEvidenceResolver,
};
use tect_domain::{
    DELIVERY_MECHANISM_CARD, DeliveryApproach, DeliveryMechanismDecisionCard, EngineeringCandidate,
    Error, EvidenceValidationOutcome, Result, TECHNICAL_DECISION_SCHEMA, TechnicalDecisionFact,
    TechnicalEvidenceBinding, TechnicalFactKind, TechnicalFactObservation, TechnicalFactValue,
};
use uuid::Uuid;

pub const TECHNICAL_EVIDENCE_SCHEMA: &str = "tect.matrix-technical-evidence/1";
pub const TECHNICAL_EVIDENCE_FORMAT: &str =
    "application/vnd.tect.matrix-technical-evidence+json;version=1";

/// Untrusted JSON contains observations and bindings only. No approval,
/// principal, authorship or validation-outcome field is accepted here.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalDecisionEvidenceArtifact {
    pub schema: String,
    pub tenant_id: Uuid,
    pub workspace_id: Uuid,
    pub task_id: Uuid,
    pub task_revision: i64,
    pub operating_verification_digest: String,
    pub operating_policy_version: String,
    pub requirements: TechnicalEvidenceRequirements,
    pub choice_set_digest: String,
    pub decision_question: String,
    pub required_outcome: String,
    pub candidate_mapping: Vec<TechnicalEvidenceCandidateMapping>,
    pub facts: Vec<TechnicalEvidenceObservation>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalEvidenceRequirements {
    /// Compared exactly with the server's persisted locator representation.
    pub locator: TechnicalEvidenceLocator,
    pub snapshot_id: Uuid,
    pub semantic_digest: String,
    pub authority_schema: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case", deny_unknown_fields)]
pub enum TechnicalEvidenceLocator {
    Program {
        program_id: Uuid,
    },
    Scope {
        program_id: Uuid,
        scope_id: Uuid,
    },
    Slice {
        program_id: Uuid,
        scope_id: Uuid,
        candidate_set_id: Uuid,
        work_candidate_id: Uuid,
        expected_work_revision: i64,
    },
    OpenedSlice {
        slice_id: Uuid,
    },
}
impl From<&MatrixRequirementsLocator> for TechnicalEvidenceLocator {
    fn from(locator: &MatrixRequirementsLocator) -> Self {
        match *locator {
            MatrixRequirementsLocator::Program { program_id } => Self::Program { program_id },
            MatrixRequirementsLocator::Scope {
                program_id,
                scope_id,
            } => Self::Scope {
                program_id,
                scope_id,
            },
            MatrixRequirementsLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => Self::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            MatrixRequirementsLocator::OpenedSlice { slice_id } => Self::OpenedSlice { slice_id },
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalEvidenceCandidateMapping {
    pub frozen_candidate: EngineeringCandidate,
    pub technical_approach: DeliveryApproach,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TechnicalEvidenceObservation {
    pub kind: TechnicalFactKind,
    pub value: TechnicalFactValue,
    pub source_ref: String,
    pub observed_at: i64,
    pub expires_at: i64,
}

/// Server composition supplies this independent whitelist. Never construct it
/// from artifact JSON, provenance, readiness or requirement confirmation.
#[derive(Debug, Clone)]
pub struct ApprovedTechnicalDecisionEvidence {
    pub binding: ServerTechnicalDecisionTaskBinding,
    pub reference: TechnicalDecisionEvidenceReference,
    pub approval: TechnicalDecisionApprovalRecord,
    pub candidate_mapping: Vec<TechnicalDecisionCandidateMapping>,
    pub validator_policy_version: String,
    pub max_age_seconds: i64,
}

#[derive(Clone)]
pub struct PgTechnicalDecisionEvidenceResolver {
    pool: PgPool,
    approvals: Vec<ApprovedTechnicalDecisionEvidence>,
}

impl PgTechnicalDecisionEvidenceResolver {
    /// An empty whitelist denies before opening a database transaction.
    pub fn new(pool: PgPool, approvals: Vec<ApprovedTechnicalDecisionEvidence>) -> Self {
        Self { pool, approvals }
    }

    fn approved(
        &self,
        binding: &ServerTechnicalDecisionTaskBinding,
        reference: &TechnicalDecisionEvidenceReference,
        now: i64,
    ) -> Option<&ApprovedTechnicalDecisionEvidence> {
        let mut matches = self.approvals.iter().filter(|a| {
            &a.binding == binding
                && &a.reference == reference
                && valid_approval(a)
                && a.approval.claim.approved_at <= now
        });
        let first = matches.next()?;
        // Ambiguous metadata is not a coherent approved snapshot.
        matches.next().is_none().then_some(first)
    }
}

/// Parse only an operator-owned private configuration, never artifact content
/// or tool arguments. Duplicate keys, unknown fields and invalid pins deny.
pub fn parse_technical_decision_approvals(
    body: &str,
) -> Result<Vec<ApprovedTechnicalDecisionEvidence>> {
    let original = strict_json::parse(body).map_err(|_| Error::InvalidConfiguration)?;
    let metadata: Vec<ApprovalConfiguration> =
        serde_json::from_value(original.clone()).map_err(|_| Error::InvalidConfiguration)?;
    if metadata.len() > 256
        || serde_json::to_value(&metadata).map_err(|_| Error::InvalidConfiguration)? != original
    {
        return Err(Error::InvalidConfiguration);
    }
    let mut keys = BTreeSet::new();
    metadata
        .into_iter()
        .map(|m| {
            let approval = m.into_approval();
            if !valid_approval(&approval)
                || !valid_config_binding(&approval)
                || !keys.insert((
                    approval.binding.tenant_id,
                    approval.binding.workspace_id,
                    approval.binding.task_id,
                    approval.binding.task_revision,
                    approval.reference.artifact_id,
                    approval.reference.artifact_version,
                ))
            {
                return Err(Error::InvalidConfiguration);
            }
            Ok(approval)
        })
        .collect()
}

fn valid_config_binding(a: &ApprovedTechnicalDecisionEvidence) -> bool {
    let b = &a.binding;
    let r = &b.requirements_binding;
    let valid_locator = match &r.locator {
        MatrixRequirementsLocator::Program { program_id } => !program_id.is_nil(),
        MatrixRequirementsLocator::Scope {
            program_id,
            scope_id,
        } => !program_id.is_nil() && !scope_id.is_nil(),
        MatrixRequirementsLocator::Slice {
            program_id,
            scope_id,
            candidate_set_id,
            work_candidate_id,
            expected_work_revision,
        } => {
            [program_id, scope_id, candidate_set_id, work_candidate_id]
                .iter()
                .all(|id| !id.is_nil())
                && *expected_work_revision > 0
        }
        MatrixRequirementsLocator::OpenedSlice { slice_id } => !slice_id.is_nil(),
    };
    let mut ids = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    valid_locator
        && !r.snapshot_id.is_nil()
        && sha256(&r.semantic_digest)
        && text(&r.authority_schema, 256)
        && text(&b.operating_policy_version, 256)
        && b.choice_set.schema == tect_domain::MATRIX_CHOICE_SET_SCHEMA
        && b.choice_set.version > 0
        && text(&b.choice_set.choice_set_id, 256)
        && text(&b.choice_set.decision_question, 4096)
        && a.approval.claim.approved_at >= 0
        && a.candidate_mapping.iter().all(|m| {
            let c = &m.frozen_candidate;
            let t = &m.technical_approach;
            ids.insert(c.candidate_id.clone())
                && kinds.insert(t.kind)
                && b.choice_set.candidates.contains(c)
                && text(&c.candidate_id, 256)
                && text(&c.title, 4096)
                && text(&c.approach, 65536)
                && c.candidate_id == t.id
                && c.title == t.title
                && c.approach == t.mechanism
                && !t.operational_consequences.is_empty()
                && t.operational_consequences.iter().all(|s| text(s, 4096))
        })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalConfiguration {
    binding: BindingConfiguration,
    reference: ReferenceConfiguration,
    approval: ApprovalRecordConfiguration,
    candidate_mapping: Vec<TechnicalEvidenceCandidateMapping>,
    validator_policy_version: String,
    max_age_seconds: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingConfiguration {
    tenant_id: Uuid,
    workspace_id: Uuid,
    task_id: Uuid,
    task_revision: i64,
    operating_verification_digest: String,
    operating_policy_version: String,
    requirements_binding: TechnicalEvidenceRequirements,
    choice_set: tect_domain::EngineeringChoiceSet,
    choice_set_digest: String,
    recorded_by_principal_id: Uuid,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceConfiguration {
    artifact_id: Uuid,
    artifact_version: i64,
    content_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRecordConfiguration {
    claim: tect_domain::TechnicalOwnerApprovalClaim,
    card_digest: String,
    candidate_digest: String,
    choice_set_digest: String,
    recorded_by_principal_id: Uuid,
    owner_author_principal_id: Uuid,
    owner_authorship_ref: String,
}
impl ApprovalConfiguration {
    fn into_approval(self) -> ApprovedTechnicalDecisionEvidence {
        let b = self.binding;
        let p = self.approval;
        let r = b.requirements_binding;
        let locator = match r.locator {
            TechnicalEvidenceLocator::Program { program_id } => {
                MatrixRequirementsLocator::Program { program_id }
            }
            TechnicalEvidenceLocator::Scope {
                program_id,
                scope_id,
            } => MatrixRequirementsLocator::Scope {
                program_id,
                scope_id,
            },
            TechnicalEvidenceLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => MatrixRequirementsLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            TechnicalEvidenceLocator::OpenedSlice { slice_id } => {
                MatrixRequirementsLocator::OpenedSlice { slice_id }
            }
        };
        ApprovedTechnicalDecisionEvidence {
            binding: ServerTechnicalDecisionTaskBinding {
                tenant_id: b.tenant_id,
                workspace_id: b.workspace_id,
                task_id: b.task_id,
                task_revision: b.task_revision,
                operating_verification_digest: b.operating_verification_digest,
                operating_policy_version: b.operating_policy_version,
                requirements_binding: tect_application::MatrixTaskRequirementsBinding {
                    locator,
                    snapshot_id: r.snapshot_id,
                    semantic_digest: r.semantic_digest,
                    authority_schema: r.authority_schema,
                },
                choice_set: b.choice_set,
                choice_set_digest: b.choice_set_digest,
                recorded_by_principal_id: b.recorded_by_principal_id,
            },
            reference: TechnicalDecisionEvidenceReference {
                artifact_id: self.reference.artifact_id,
                artifact_version: self.reference.artifact_version,
                content_sha256: self.reference.content_sha256,
            },
            approval: TechnicalDecisionApprovalRecord {
                claim: p.claim,
                card_digest: p.card_digest,
                candidate_digest: p.candidate_digest,
                choice_set_digest: p.choice_set_digest,
                recorded_by_principal_id: p.recorded_by_principal_id,
                owner_author_principal_id: p.owner_author_principal_id,
                owner_authorship_ref: p.owner_authorship_ref,
            },
            candidate_mapping: self
                .candidate_mapping
                .into_iter()
                .map(|m| TechnicalDecisionCandidateMapping {
                    frozen_candidate: m.frozen_candidate,
                    technical_approach: m.technical_approach,
                })
                .collect(),
            validator_policy_version: self.validator_policy_version,
            max_age_seconds: self.max_age_seconds,
        }
    }
}

#[async_trait]
impl TechnicalDecisionEvidenceResolver for PgTechnicalDecisionEvidenceResolver {
    async fn resolve(
        &self,
        binding: &ServerTechnicalDecisionTaskBinding,
        reference: &TechnicalDecisionEvidenceReference,
        now: i64,
    ) -> Result<Option<ResolvedTechnicalDecisionEvidence>> {
        let Some(approved) = self.approved(binding, reference, now) else {
            return Ok(None);
        };
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(binding.tenant_id.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        let row: Option<ArtifactRow> = sqlx::query_as(
            "SELECT digest,size,format,readiness,body FROM pipeline_evidence_artifacts \
             WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4",
        )
        .bind(binding.tenant_id)
        .bind(binding.workspace_id)
        .bind(reference.artifact_id)
        .bind(reference.artifact_version)
        .fetch_optional(&mut *tx)
        .await
        .map_err(storage_error)?;
        tx.commit().await.map_err(storage_error)?;
        checked_technical_snapshot(row, binding, reference, approved, now)
    }
}

type ArtifactRow = (String, i64, String, String, String);

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max
}
fn sha256(value: &str) -> bool {
    value.len() == 64 && value.bytes().all(|b| b.is_ascii_hexdigit())
}
fn current_observation(observed_at: i64, expires_at: i64, now: i64, max_age: i64) -> bool {
    max_age > 0
        && observed_at <= now
        && expires_at > now
        && expires_at > observed_at
        && now
            .checked_sub(observed_at)
            .is_some_and(|age| age <= max_age)
}
fn valid_approval(a: &ApprovedTechnicalDecisionEvidence) -> bool {
    let b = &a.binding;
    let p = &a.approval;
    !b.tenant_id.is_nil()
        && !b.workspace_id.is_nil()
        && !b.task_id.is_nil()
        && b.task_revision > 0
        && !a.reference.artifact_id.is_nil()
        && a.reference.artifact_version > 0
        && sha256(&a.reference.content_sha256)
        && sha256(&b.operating_verification_digest)
        && sha256(&b.choice_set_digest)
        && a.validator_policy_version == TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION
        && a.max_age_seconds > 0
        && p.recorded_by_principal_id == b.recorded_by_principal_id
        && p.owner_author_principal_id == b.recorded_by_principal_id
        && !p.owner_author_principal_id.is_nil()
        && text(&p.owner_authorship_ref, 4096)
        && p.choice_set_digest == b.choice_set_digest
        && sha256(&p.card_digest)
        && sha256(&p.candidate_digest)
        && p.claim.candidate_digest == p.candidate_digest
        && p.claim.task_id == b.task_id.to_string()
        && p.claim.task_revision == b.task_revision.to_string()
        && text(&p.claim.approval_ref, 4096)
        && text(&p.claim.approving_principal, 256)
        && b.choice_set.task_id == b.task_id.to_string()
        && b.choice_set.task_revision == b.task_revision.to_string()
        && b.choice_set.candidates.len() == 2
        && a.candidate_mapping.len() == 2
}

fn checked_technical_snapshot(
    row: Option<ArtifactRow>,
    binding: &ServerTechnicalDecisionTaskBinding,
    reference: &TechnicalDecisionEvidenceReference,
    approved: &ApprovedTechnicalDecisionEvidence,
    now: i64,
) -> Result<Option<ResolvedTechnicalDecisionEvidence>> {
    if &approved.binding != binding
        || &approved.reference != reference
        || !valid_approval(approved)
        || approved.approval.claim.approved_at > now
    {
        return Ok(None);
    }
    let Some((digest, size, format, readiness, body)) = row else {
        return Ok(None);
    };
    if readiness != "ready"
        || format != TECHNICAL_EVIDENCE_FORMAT
        || size != body.len() as i64
        || body.len() > 262_144
        || digest != reference.content_sha256
        || format!("{:x}", Sha256::digest(body.as_bytes())) != reference.content_sha256
    {
        return Ok(None);
    }
    let artifact = parse_technical_artifact(&body)?;
    let requirements = &binding.requirements_binding;
    if artifact.schema != TECHNICAL_EVIDENCE_SCHEMA
        || artifact.tenant_id != binding.tenant_id
        || artifact.workspace_id != binding.workspace_id
        || artifact.task_id != binding.task_id
        || artifact.task_revision != binding.task_revision
        || artifact.operating_verification_digest != binding.operating_verification_digest
        || artifact.operating_policy_version != binding.operating_policy_version
        || artifact.requirements.locator != TechnicalEvidenceLocator::from(&requirements.locator)
        || artifact.requirements.snapshot_id != requirements.snapshot_id
        || artifact.requirements.semantic_digest != requirements.semantic_digest
        || artifact.requirements.authority_schema != requirements.authority_schema
        || artifact.choice_set_digest != binding.choice_set_digest
        || artifact.decision_question != binding.choice_set.decision_question
        || !text(&artifact.decision_question, 512)
        || !text(&artifact.required_outcome, 512)
        || artifact.candidate_mapping.len() != 2
        || artifact.facts.len() != 7
    {
        return Ok(None);
    }
    let mut ids = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    let mut mappings = Vec::new();
    for mapping in artifact.candidate_mapping {
        let saved = &mapping.frozen_candidate;
        let approach = &mapping.technical_approach;
        let resolved = TechnicalDecisionCandidateMapping {
            frozen_candidate: saved.clone(),
            technical_approach: approach.clone(),
        };
        if !ids.insert(saved.candidate_id.clone())
            || !kinds.insert(approach.kind)
            || !binding.choice_set.candidates.contains(saved)
            || !approved.candidate_mapping.contains(&resolved)
            || saved.candidate_id != approach.id
            || saved.title != approach.title
            || saved.approach != approach.mechanism
        {
            return Ok(None);
        }
        mappings.push(resolved);
    }
    let mut fact_kinds = BTreeSet::new();
    let mut facts = Vec::new();
    for observed in artifact.facts {
        let source = matches!(
            observed.kind,
            TechnicalFactKind::ReuseSourceSupport | TechnicalFactKind::SeparateSourceSupport
        );
        if !fact_kinds.insert(observed.kind)
            || source != matches!(observed.value, TechnicalFactValue::SourceSupport(_))
            || !text(&observed.source_ref, 4096)
            || !current_observation(
                observed.observed_at,
                observed.expires_at,
                now,
                approved.max_age_seconds,
            )
        {
            return Ok(None);
        }
        facts.push(TechnicalDecisionFact {
            kind: observed.kind,
            observation: TechnicalFactObservation::Verified {
                value: observed.value,
                binding: TechnicalEvidenceBinding {
                    task_id: binding.task_id.to_string(),
                    task_revision: binding.task_revision.to_string(),
                    matrix_verification_digest: binding.operating_verification_digest.clone(),
                    evidence_ref: format!(
                        "pipeline-evidence:{}@{}",
                        reference.artifact_id, reference.artifact_version
                    ),
                    content_digest: reference.content_sha256.clone(),
                    validator_policy_version: approved.validator_policy_version.clone(),
                    observed_at: observed.observed_at,
                    expires_at: observed.expires_at,
                    validation_outcome: EvidenceValidationOutcome::Accepted,
                },
            },
        });
    }
    let card = DeliveryMechanismDecisionCard {
        schema: TECHNICAL_DECISION_SCHEMA.into(),
        card_id: DELIVERY_MECHANISM_CARD.into(),
        task_id: binding.task_id.to_string(),
        task_revision: binding.task_revision.to_string(),
        matrix_verification_digest: binding.operating_verification_digest.clone(),
        decision_question: artifact.decision_question,
        required_outcome: artifact.required_outcome,
        approaches: mappings
            .iter()
            .map(|m| m.technical_approach.clone())
            .collect(),
        owner_approval: approved.approval.claim.clone(),
        facts: facts.clone(),
    };
    if card.candidate_digest()? != approved.approval.candidate_digest
        || card.canonical_digest()? != approved.approval.card_digest
    {
        return Ok(None);
    }
    // Validate full domain shape/relations before publishing a trusted snapshot.
    tect_domain::compare_delivery_mechanisms_with_trust(
        &card,
        &card.task_id,
        &card.task_revision,
        &card.matrix_verification_digest,
        now,
        &SnapshotTrust,
    )?;
    Ok(Some(ResolvedTechnicalDecisionEvidence {
        binding: binding.clone(),
        reference: reference.clone(),
        card,
        facts,
        approval: approved.approval.clone(),
        candidate_mapping: mappings,
        validator_policy_version: approved.validator_policy_version.clone(),
        max_age_seconds: approved.max_age_seconds,
    }))
}

// Used only after independent approval, exact raw bytes and all metadata checks.
struct SnapshotTrust;
impl tect_domain::TechnicalDecisionTrust for SnapshotTrust {
    fn validate_fact(
        &self,
        _: &DeliveryMechanismDecisionCard,
        _: &TechnicalDecisionFact,
        _: &TechnicalEvidenceBinding,
        _: i64,
    ) -> Result<bool> {
        Ok(true)
    }
    fn validate_owner_approval(
        &self,
        _: &DeliveryMechanismDecisionCard,
        _: &str,
        _: i64,
    ) -> Result<bool> {
        Ok(true)
    }
}

#[path = "technical_decision_evidence_json.rs"]
mod strict_json;
fn parse_technical_artifact(body: &str) -> Result<TechnicalDecisionEvidenceArtifact> {
    let original = strict_json::parse(body)?;
    let artifact: TechnicalDecisionEvidenceArtifact =
        serde_json::from_value(original.clone()).map_err(|_| Error::Forbidden)?;
    if serde_json::to_value(&artifact).map_err(|_| Error::Forbidden)? != original {
        return Err(Error::Forbidden);
    }
    Ok(artifact)
}

#[cfg(test)]
#[path = "technical_decision_evidence_tests.rs"]
mod tests;
