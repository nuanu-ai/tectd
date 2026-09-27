//! Explicit declarations are promises, never proof of operating facts. The
//! application authenticates owners and validates real Program/Scope/Slice ancestry.
use crate::{
    CommitmentEvidence, EngineeringIntent, EngineeringMatrixInput, EngineeringMode, Error,
    FactProvenance, MatrixFact, RequiredMatrixFact, Result, required_matrix_facts,
};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use uuid::Uuid;

pub const MATRIX_REQUIREMENTS_SCHEMA: &str = "tect.matrix-requirements/1";

/// Slice uses its stable logical work identity before and after opening. The
/// application resolves opened native Slice locators and verifies database ancestry.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "level", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequirementsAnchor {
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
    },
}
impl RequirementsAnchor {
    pub fn program_id(self) -> Uuid {
        match self {
            Self::Program { program_id }
            | Self::Scope { program_id, .. }
            | Self::Slice { program_id, .. } => program_id,
        }
    }
    fn level(self) -> u8 {
        match self {
            Self::Program { .. } => 0,
            Self::Scope { .. } => 1,
            Self::Slice { .. } => 2,
        }
    }
    fn validate(self) -> Result<()> {
        let id = match self {
            Self::Program { program_id } => program_id,
            Self::Scope { scope_id, .. } => scope_id,
            Self::Slice {
                scope_id,
                candidate_set_id,
                work_candidate_id,
                ..
            } => {
                if scope_id.is_nil() || candidate_set_id.is_nil() {
                    return Err(Error::InvalidArguments);
                }
                work_candidate_id
            }
        };
        if id.is_nil() || self.program_id().is_nil() {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum DeclaredRequirementPath {
    Mode,
    Intent,
    Urgency,
    PromisedBehavior,
    PromisedProof,
    DemandCommitment,
    LatencyCommitment,
}
impl DeclaredRequirementPath {
    pub fn fact_path(self) -> &'static str {
        match self {
            Self::Mode => "/mode",
            Self::Intent => "/intent",
            Self::Urgency => "/urgency",
            Self::PromisedBehavior => "/promised_behavior",
            Self::PromisedProof => "/promised_proof",
            Self::DemandCommitment => "/demand_commitment",
            Self::LatencyCommitment => "/latency_commitment",
        }
    }
}

/// NoCommitment declares absence of a promise. Verified limits are deliberately
/// impossible to place in a declaration patch.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(
    tag = "kind",
    content = "value",
    rename_all = "snake_case",
    deny_unknown_fields
)]
pub enum DeclaredRequirementValue {
    Mode(EngineeringMode),
    Intent(EngineeringIntent),
    Urgency(String),
    PromisedBehavior(String),
    PromisedProof(String),
    NoDemandCommitment,
    NoLatencyCommitment,
}
impl DeclaredRequirementValue {
    pub fn path(&self) -> DeclaredRequirementPath {
        match self {
            Self::Mode(_) => DeclaredRequirementPath::Mode,
            Self::Intent(_) => DeclaredRequirementPath::Intent,
            Self::Urgency(_) => DeclaredRequirementPath::Urgency,
            Self::PromisedBehavior(_) => DeclaredRequirementPath::PromisedBehavior,
            Self::PromisedProof(_) => DeclaredRequirementPath::PromisedProof,
            Self::NoDemandCommitment => DeclaredRequirementPath::DemandCommitment,
            Self::NoLatencyCommitment => DeclaredRequirementPath::LatencyCommitment,
        }
    }
    fn validate(&self) -> Result<()> {
        match self {
            Self::Intent(EngineeringIntent::Other(s))
            | Self::Urgency(s)
            | Self::PromisedBehavior(s)
            | Self::PromisedProof(s) => text(s),
            _ => Ok(()),
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "operation", rename_all = "snake_case", deny_unknown_fields)]
pub enum RequirementDeclarationPatch {
    Set { value: DeclaredRequirementValue },
    Remove { path: DeclaredRequirementPath },
}
impl RequirementDeclarationPatch {
    fn path(&self) -> DeclaredRequirementPath {
        match self {
            Self::Set { value } => value.path(),
            Self::Remove { path } => *path,
        }
    }
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct DeclarationRecorder {
    pub principal: String,
    pub session: String,
}
impl DeclarationRecorder {
    fn validate(&self) -> Result<()> {
        text(&self.principal)?;
        text(&self.session)
    }
}

/// Append-only revision; constructors do not assert that recorder is the human.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixRequirementsProposal {
    anchor: RequirementsAnchor,
    revision: u64,
    patches: Vec<RequirementDeclarationPatch>,
    recorder: DeclarationRecorder,
    digest: String,
}
impl MatrixRequirementsProposal {
    pub fn new(
        anchor: RequirementsAnchor,
        revision: u64,
        mut patches: Vec<RequirementDeclarationPatch>,
        recorder: DeclarationRecorder,
    ) -> Result<Self> {
        patches.sort_by_key(RequirementDeclarationPatch::path);
        let mut proposal = Self {
            anchor,
            revision,
            patches,
            recorder,
            digest: String::new(),
        };
        proposal.validate_shape()?;
        proposal.digest = proposal.canonical_digest()?;
        Ok(proposal)
    }
    fn validate_shape(&self) -> Result<()> {
        self.anchor.validate()?;
        self.recorder.validate()?;
        if self.revision == 0 || self.patches.is_empty() {
            return Err(Error::InvalidArguments);
        }
        let mut paths = std::collections::BTreeSet::new();
        for patch in &self.patches {
            if !paths.insert(patch.path()) {
                return Err(Error::InvalidArguments);
            }
            if let RequirementDeclarationPatch::Set { value } = patch {
                value.validate()?;
            }
        }
        Ok(())
    }
    fn validate(&self) -> Result<()> {
        self.validate_shape()?;
        if self.digest != self.canonical_digest()? {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
    pub fn canonical_digest(&self) -> Result<String> {
        let mut copy = self.clone();
        copy.digest.clear();
        copy.patches.sort_by_key(RequirementDeclarationPatch::path);
        digest(&copy)
    }
    pub fn anchor(&self) -> RequirementsAnchor {
        self.anchor
    }
    pub fn revision(&self) -> u64 {
        self.revision
    }
    pub fn digest(&self) -> &str {
        &self.digest
    }
    pub fn patches(&self) -> &[RequirementDeclarationPatch] {
        &self.patches
    }
    pub fn recorder(&self) -> &DeclarationRecorder {
        &self.recorder
    }
}

/// Structural binding only: application must authorize the owner and verify
/// that response_ref identifies their explicit response to this exact proposal.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixRequirementsConfirmation {
    anchor: RequirementsAnchor,
    proposal_revision: u64,
    proposal_digest: String,
    owner_principal: String,
    owner_response_ref: String,
    recorder: DeclarationRecorder,
}
impl MatrixRequirementsConfirmation {
    pub fn new(
        proposal: &MatrixRequirementsProposal,
        proposal_revision: u64,
        proposal_digest: String,
        owner_principal: String,
        owner_response_ref: String,
        recorder: DeclarationRecorder,
    ) -> Result<Self> {
        let value = Self {
            anchor: proposal.anchor,
            proposal_revision,
            proposal_digest,
            owner_principal,
            owner_response_ref,
            recorder,
        };
        value.validate_for(proposal)?;
        Ok(value)
    }
    pub fn validate_for(&self, proposal: &MatrixRequirementsProposal) -> Result<()> {
        proposal.validate()?;
        text(&self.owner_principal)?;
        text(&self.owner_response_ref)?;
        self.recorder.validate()?;
        if self.anchor != proposal.anchor
            || self.proposal_revision != proposal.revision
            || self.proposal_digest != proposal.digest
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
    pub fn owner_principal(&self) -> &str {
        &self.owner_principal
    }
    pub fn owner_response_ref(&self) -> &str {
        &self.owner_response_ref
    }
    pub fn recorder(&self) -> &DeclarationRecorder {
        &self.recorder
    }
}
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixRequirementsRevision {
    pub proposal: MatrixRequirementsProposal,
    pub confirmation: Option<MatrixRequirementsConfirmation>,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedRequirementSource {
    pub anchor: RequirementsAnchor,
    pub proposal_revision: u64,
    pub proposal_digest: String,
    pub owner_principal: String,
    pub owner_response_ref: String,
}
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct ResolvedDeclaredRequirement {
    pub value: DeclaredRequirementValue,
    pub source: ResolvedRequirementSource,
}

/// Immutable snapshot. Its semantic digest excludes recorder/source changes.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct EffectiveMatrixRequirements {
    schema: String,
    program_id: Uuid,
    values: BTreeMap<DeclaredRequirementPath, ResolvedDeclaredRequirement>,
    semantic_digest: String,
}
impl EffectiveMatrixRequirements {
    pub fn schema(&self) -> &str {
        &self.schema
    }
    pub fn program_id(&self) -> Uuid {
        self.program_id
    }
    pub fn values(&self) -> &BTreeMap<DeclaredRequirementPath, ResolvedDeclaredRequirement> {
        &self.values
    }
    pub fn semantic_digest(&self) -> &str {
        &self.semantic_digest
    }
}

/// `lineage` is supplied by the trusted application after database ancestry
/// checks. Unknown anchors, cross-Program records and duplicate revisions fail.
pub fn resolve_matrix_requirements(
    lineage: &[RequirementsAnchor],
    revisions: &[MatrixRequirementsRevision],
    schema: &str,
) -> Result<EffectiveMatrixRequirements> {
    text(schema)?;
    let Some(first @ RequirementsAnchor::Program { .. }) = lineage.first().copied() else {
        return Err(Error::InvalidArguments);
    };
    for (index, anchor) in lineage.iter().enumerate() {
        anchor.validate()?;
        if anchor.program_id() != first.program_id() || usize::from(anchor.level()) != index {
            return Err(Error::InvalidArguments);
        }
        if let RequirementsAnchor::Slice { scope_id, .. } = anchor
            && !matches!(lineage.get(index - 1), Some(RequirementsAnchor::Scope { scope_id: parent, .. }) if parent == scope_id)
        {
            return Err(Error::InvalidArguments);
        }
    }
    let mut ordered = BTreeMap::new();
    for revision in revisions {
        revision.proposal.validate()?;
        let Some(index) = lineage.iter().position(|a| *a == revision.proposal.anchor) else {
            return Err(Error::InvalidArguments);
        };
        if ordered
            .insert((index, revision.proposal.revision), revision)
            .is_some()
        {
            return Err(Error::InvalidArguments);
        }
    }
    let mut values = BTreeMap::new();
    for revision in ordered.values() {
        let Some(confirmation) = &revision.confirmation else {
            continue;
        };
        confirmation.validate_for(&revision.proposal)?;
        let source = ResolvedRequirementSource {
            anchor: confirmation.anchor,
            proposal_revision: confirmation.proposal_revision,
            proposal_digest: confirmation.proposal_digest.clone(),
            owner_principal: confirmation.owner_principal.clone(),
            owner_response_ref: confirmation.owner_response_ref.clone(),
        };
        for patch in &revision.proposal.patches {
            match patch {
                RequirementDeclarationPatch::Set { value } => {
                    values.insert(
                        value.path(),
                        ResolvedDeclaredRequirement {
                            value: value.clone(),
                            source: source.clone(),
                        },
                    );
                }
                RequirementDeclarationPatch::Remove { path } => {
                    values.remove(path);
                }
            }
        }
    }
    let semantic: BTreeMap<_, _> = values
        .iter()
        .map(|(path, value)| (*path, &value.value))
        .collect();
    let semantic_digest = digest(&(schema, semantic))?;
    Ok(EffectiveMatrixRequirements {
        schema: schema.into(),
        program_id: first.program_id(),
        values,
        semantic_digest,
    })
}

pub fn missing_required_matrix_declarations(
    context: &EffectiveMatrixRequirements,
) -> Vec<DeclaredRequirementPath> {
    [
        DeclaredRequirementPath::Mode,
        DeclaredRequirementPath::Intent,
        DeclaredRequirementPath::Urgency,
        DeclaredRequirementPath::PromisedBehavior,
        DeclaredRequirementPath::PromisedProof,
    ]
    .into_iter()
    .filter(|path| !context.values.contains_key(path))
    .collect()
}

#[path = "matrix_declared_requirements_roles.rs"]
mod roles;
pub use roles::{MatrixFactRole, matrix_fact_role};

#[path = "matrix_declared_requirements_binding.rs"]
mod binding;
pub use binding::{
    bind_matrix_requirements_input, compose_declared_requirements_matrix,
    required_matrix_operating_facts,
};
fn text(value: &str) -> Result<()> {
    if value.trim().is_empty() || value.len() > 256 {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
fn digest(value: &impl Serialize) -> Result<String> {
    let value = serde_json::to_value(value).map_err(|_| Error::InvalidArguments)?;
    let bytes = serde_json::to_vec(&value).map_err(|_| Error::InvalidArguments)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

#[cfg(test)]
#[path = "matrix_declared_requirements_tests.rs"]
mod tests;
