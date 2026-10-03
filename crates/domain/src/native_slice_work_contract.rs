//! Native planning declaration validation; never grants filesystem or external authority.
use crate::*;
use serde::{Deserialize, Deserializer};
use std::collections::BTreeSet;
use uuid::Uuid;

mod paths;
use paths::{beneath, contains, normalized_path, path};

pub const NATIVE_WORK_CONTRACT_VERSION: &str = "0.6.0-native.engineering.3";
pub const NATIVE_WORK_CONTRACT_SUCCESSOR_VERSION: &str = "0.6.0-native.engineering.4";
pub fn native_work_contract_version(version: &str) -> bool {
    matches!(
        version,
        NATIVE_WORK_CONTRACT_VERSION | NATIVE_WORK_CONTRACT_SUCCESSOR_VERSION
    )
}
pub const NATIVE_WORK_CONTRACT_SCHEMA: &str = "tect:native-slice-work-contract-schema";
pub const NATIVE_WORK_CONTRACT_PHASE: &str = "slice-contract-writer";
pub const NATIVE_WORK_CONTRACT_ARTIFACT: &str = "work-order-contract.json";

fn uuid<'de, D: Deserializer<'de>>(d: D) -> std::result::Result<Uuid, D::Error> {
    let text = String::deserialize(d)?;
    let id = Uuid::parse_str(&text).map_err(serde::de::Error::custom)?;
    if id.is_nil() || id.to_string() != text {
        return Err(serde::de::Error::custom("canonical non-nil UUID required"));
    }
    Ok(id)
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSliceWorkContract {
    pub contract_kind: String,
    pub target: NativeWorkTarget,
    #[serde(deserialize_with = "required_option")]
    pub session_declaration: Option<NativeWorkSession>,
    #[serde(deserialize_with = "required_option")]
    pub source_checkpoint: Option<NativeWorkCheckpoint>,
    pub required_reads: Vec<NativeWorkRead>,
    pub write_scope: NativeWriteScope,
    pub authority: NativeWorkAuthority,
    pub proof_requirements: Vec<NativeWorkProof>,
    pub validation_requirements: Vec<NativeWorkValidation>,
    pub result_closure: NativeWorkClosure,
    pub refresh_resume: NativeWorkRefresh,
}
fn required_option<'de, D, T>(d: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: Deserialize<'de>,
{
    Option::deserialize(d)
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkCheckpoint {
    #[serde(deserialize_with = "uuid")]
    pub checkpoint_id: Uuid,
    pub digest: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkTarget {
    #[serde(deserialize_with = "uuid")]
    pub scope_id: Uuid,
    #[serde(deserialize_with = "uuid")]
    pub slice_id: Uuid,
    pub slice_revision: i64,
    #[serde(deserialize_with = "uuid")]
    pub run_id: Uuid,
    pub run_revision: i64,
    pub phase_id: String,
    pub definition_kind: String,
    pub definition_version: String,
    pub definition_digest: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkSession {
    #[serde(deserialize_with = "uuid")]
    pub workspace_id: Uuid,
    #[serde(deserialize_with = "uuid")]
    pub session_id: Uuid,
    #[serde(deserialize_with = "uuid")]
    pub host_id: Uuid,
    pub native_session_id: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkRead {
    pub reference: NativeWorkReference,
    pub required: bool,
}
#[derive(Debug, Deserialize, serde::Serialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum NativeWorkReference {
    NativeOutput {
        phase_id: String,
        output_revision: i64,
        digest: String,
    },
    NativeInput {
        #[serde(deserialize_with = "uuid")]
        input_id: Uuid,
        sequence: i64,
        digest: String,
    },
    NativeEvidenceArtifact {
        #[serde(deserialize_with = "uuid")]
        artifact_id: Uuid,
        revision: i64,
        digest: String,
    },
    SourceFile {
        #[serde(deserialize_with = "uuid")]
        source_id: Uuid,
        path: String,
        content_sha256: String,
    },
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSourcePath {
    #[serde(deserialize_with = "uuid")]
    pub source_id: Uuid,
    pub path: String,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWriteScope {
    pub allowed_roots: Vec<NativeSourcePath>,
    pub allowed_paths: Vec<NativeSourcePath>,
    pub denied_roots: Vec<NativeSourcePath>,
    pub before_hash_required: bool,
}
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeAuthorityStatus {
    Authorized,
    Missing,
    Unknown,
    Expired,
}
#[derive(Debug, Deserialize, PartialEq, Eq)]
#[serde(rename_all = "snake_case")]
pub enum NativeSourceAction {
    SourcePlan,
    SourceEdit,
    SourceTest,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeSourceAuthority {
    pub action: NativeSourceAction,
    pub targets: Vec<NativeSourcePath>,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkAuthority {
    pub status: NativeAuthorityStatus,
    #[serde(deserialize_with = "required_option")]
    pub source: Option<NativeWorkReference>,
    pub scope: Vec<NativeSourceAuthority>,
    pub limitations: String,
}
#[derive(Debug, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum NativeProofKind {
    NativeOutput,
    NativeInput,
    NativeEvidenceArtifact,
    SourceFile,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkProof {
    pub id: String,
    pub obligation: String,
    pub evidence_kind: NativeProofKind,
    pub completion_required: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkValidation {
    pub id: String,
    pub obligation: String,
    pub proof_requirement_ids: Vec<String>,
    pub completion_required: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkClosure {
    pub native_result_required: bool,
    pub summary_required: bool,
    pub evidence_required: bool,
    pub scope_impact_required: bool,
    pub remaining_work_required: bool,
    pub handoff_when_blocked: bool,
}
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NativeWorkRefresh {
    pub refresh_before_write: bool,
    pub recheck_authority: bool,
    pub rehash_required_reads: bool,
    pub verify_before_hash: bool,
    pub reconcile_unknown_outcome: bool,
    pub resume_from_current_native_context: bool,
}
pub fn native_work_contract_refusal(reason: &str) -> Error {
    Error::refused_at(
        RefusalCode::InvalidOutput,
        "WP6-NATIVE-CONTRACT-01",
        "arguments.params.output.artifacts",
        "a current bounded native Slice planning contract",
        reason,
        "correct_native_work_contract",
        "native_slice_work_contract",
    )
}
pub fn native_work_contract_definition(definition: &PipelineDefinitionSnapshot) -> bool {
    definition.kind == PipelineKind::FullDesignToExecution
        && native_work_contract_version(&definition.version)
}
pub fn native_work_contract_phase(
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> bool {
    native_work_contract_definition(definition)
        && phase.id == NATIVE_WORK_CONTRACT_PHASE
        && phase.required_artifacts.iter().any(|r| {
            r.name_pattern == NATIVE_WORK_CONTRACT_ARTIFACT
                && r.schema_resource_id.as_deref() == Some(NATIVE_WORK_CONTRACT_SCHEMA)
        })
}
fn text(s: &str) -> bool {
    !s.trim().is_empty()
}
fn sha(s: &str) -> bool {
    s.len() == 64
        && s.bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
impl NativeWorkReference {
    pub fn validate(&self) -> bool {
        match self {
            Self::NativeOutput {
                phase_id,
                output_revision,
                digest,
            } => text(phase_id) && *output_revision > 0 && sha(digest),
            Self::NativeInput {
                sequence, digest, ..
            } => *sequence > 0 && sha(digest),
            Self::NativeEvidenceArtifact {
                revision, digest, ..
            } => *revision > 0 && sha(digest),
            Self::SourceFile {
                path: p,
                content_sha256,
                ..
            } => path(p, false) && sha(content_sha256),
        }
    }
}
impl NativeSliceWorkContract {
    pub fn parse(body: &str) -> Result<Self> {
        let value: Self = serde_json::from_str(body)
            .map_err(|_| native_work_contract_refusal("invalid closed contract shape"))?;
        value.validate()?;
        Ok(value)
    }
    pub fn validate(&self) -> Result<()> {
        let fail = |s| Err(native_work_contract_refusal(s));
        let t = &self.target;
        if self.contract_kind != "native_slice_work_contract_v1"
            || t.phase_id != NATIVE_WORK_CONTRACT_PHASE
            || t.definition_kind != PipelineKind::FullDesignToExecution.as_str()
            || !native_work_contract_version(&t.definition_version)
            || t.slice_revision < 1
            || t.run_revision < 1
            || !sha(&t.definition_digest)
        {
            return fail("invalid target pins");
        }
        let Some(session) = &self.session_declaration else {
            return fail("authenticated session declaration required");
        };
        if validate_native_id(&session.native_session_id).is_err() {
            return fail("invalid native session identity");
        }
        if self
            .source_checkpoint
            .as_ref()
            .is_some_and(|c| c.checkpoint_id.is_nil() || !sha(&c.digest))
        {
            return fail("invalid source checkpoint");
        }
        if self.required_reads.is_empty()
            || self
                .required_reads
                .iter()
                .any(|r| !r.required || !r.reference.validate())
        {
            return fail("invalid required reads");
        }
        let mut references = BTreeSet::new();
        if self
            .required_reads
            .iter()
            .any(|r| !references.insert(serde_json::to_string(&r.reference).unwrap_or_default()))
        {
            return fail("duplicate required read references");
        }
        for phase in ["slice-full-dev-entry-gate", "slice-design-spec-shaper"] {
            if self.required_reads.iter().filter(|r| matches!(&r.reference,NativeWorkReference::NativeOutput{phase_id,..} if phase_id==phase)).count()!=1 { return fail("exact Phase1 and Phase3 output reads required"); }
        }
        let w = &self.write_scope;
        if !w.before_hash_required
            || w.allowed_roots.is_empty()
            || w.denied_roots.is_empty()
            || w.allowed_roots.iter().any(|p| !path(&p.path, true))
            || w.denied_roots
                .iter()
                .any(|p| !normalized_path(&p.path, true))
            || w.allowed_paths.iter().any(|p| !path(&p.path, false))
        {
            return fail("invalid write scope");
        }
        for p in &w.allowed_paths {
            if !w.allowed_roots.iter().any(|r| contains(p, r))
                || w.denied_roots
                    .iter()
                    .any(|r| contains(p, r) || contains(r, p))
            {
                return fail("write path outside roots or overlaps denial");
            }
            for private in [".git", ".tect", "tect/workspace"] {
                if !w
                    .denied_roots
                    .iter()
                    .any(|r| r.source_id == p.source_id && beneath(private, &r.path))
                {
                    return fail("private runtime roots must be denied");
                }
            }
        }
        let a = &self.authority;
        if a.status != NativeAuthorityStatus::Authorized
            || a.source.as_ref().is_none_or(|r| !r.validate())
            || a.scope.is_empty()
            || !text(&a.limitations)
        {
            return fail("current explicit source authority required");
        }
        let source_context: BTreeSet<Uuid> = self
            .required_reads
            .iter()
            .filter_map(|r| match &r.reference {
                NativeWorkReference::SourceFile { source_id, .. } => Some(*source_id),
                _ => None,
            })
            .chain(w.allowed_roots.iter().map(|p| p.source_id))
            .collect();
        for scope in &a.scope {
            if scope.targets.is_empty()
                || scope.targets.iter().any(|p| {
                    !path(&p.path, false)
                        || !source_context.contains(&p.source_id)
                        || scope.action == NativeSourceAction::SourceEdit
                            && !w.allowed_paths.iter().any(|r| contains(p, r))
                })
            {
                return fail("authority targets must be bounded to declared source context");
            }
        }
        let mut proofs = BTreeSet::new();
        if self.proof_requirements.is_empty()
            || self.proof_requirements.iter().any(|p| {
                !text(&p.id)
                    || !text(&p.obligation)
                    || !p.completion_required
                    || !proofs.insert(p.id.as_str())
            })
        {
            return fail("invalid or duplicate proof obligations");
        }
        let mut validations = BTreeSet::new();
        if self.validation_requirements.is_empty()
            || self.validation_requirements.iter().any(|v| {
                !text(&v.id)
                    || !text(&v.obligation)
                    || !v.completion_required
                    || !validations.insert(v.id.as_str())
                    || v.proof_requirement_ids.is_empty()
                    || v.proof_requirement_ids
                        .iter()
                        .collect::<BTreeSet<_>>()
                        .len()
                        != v.proof_requirement_ids.len()
                    || v.proof_requirement_ids
                        .iter()
                        .any(|id| !proofs.contains(id.as_str()))
            })
        {
            return fail("invalid validation proof links");
        }
        let c = &self.result_closure;
        let r = &self.refresh_resume;
        if ![
            c.native_result_required,
            c.summary_required,
            c.evidence_required,
            c.scope_impact_required,
            c.remaining_work_required,
            c.handoff_when_blocked,
            r.refresh_before_write,
            r.recheck_authority,
            r.rehash_required_reads,
            r.verify_before_hash,
            r.reconcile_unknown_outcome,
            r.resume_from_current_native_context,
        ]
        .into_iter()
        .all(|v| v)
        {
            return fail("closure and refresh obligations cannot be weakened");
        }
        Ok(())
    }
}
pub fn validate_native_work_contract_output(
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    if !native_work_contract_phase(definition, phase) {
        return Ok(());
    }
    let artifact = request
        .output
        .artifacts
        .iter()
        .find(|a| a.name == NATIVE_WORK_CONTRACT_ARTIFACT);
    if request.output.verdict.as_deref() != Some("contract_ready") {
        return if artifact.is_some() {
            Err(native_work_contract_refusal(
                "canonical contract artifact is only allowed for contract_ready",
            ))
        } else {
            Ok(())
        };
    }
    {
        let c = NativeSliceWorkContract::parse(
            &artifact
                .ok_or_else(|| native_work_contract_refusal("required artifact missing"))?
                .body,
        )?;
        for reference in c
            .required_reads
            .iter()
            .map(|r| &r.reference)
            .chain(c.authority.source.iter())
        {
            if let NativeWorkReference::NativeOutput { phase_id, .. } = reference
                && definition
                    .phases
                    .iter()
                    .find(|p| &p.id == phase_id)
                    .is_none_or(|p| p.ordinal >= 4)
            {
                return Err(native_work_contract_refusal(
                    "authoring read cannot depend on current or later phase outputs",
                ));
            }
        }
        if c.target.run_id != request.run_id
            || c.target.run_revision != request.run_revision
            || c.target.definition_version != definition.version
            || c.target.definition_digest != definition.digest
        {
            return Err(native_work_contract_refusal(
                "authoring request pins mismatch",
            ));
        }
    }
    Ok(())
}
#[cfg(test)]
#[path = "native_slice_work_contract_tests.rs"]
mod tests;
