use super::scope_source::{canonical_digest, valid_digest, valid_id, validate_obligations};
use crate::{
    Error, FrozenScopeSource, ResolvedCandidateDraft, Result, ScopeDigest, SourceObligation,
};
use serde::{Deserialize, Serialize};
use std::collections::{BTreeMap, BTreeSet};

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeAlternativeId(pub String);

impl ScopeAlternativeId {
    pub fn validate(&self) -> Result<()> {
        if valid_digest(&self.0) {
            Ok(())
        } else {
            Err(Error::InvalidArguments)
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeDecompositionKind {
    Cohesive,
    Partitioned,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ObligationCoverage {
    pub obligation_id: String,
    #[serde(default)]
    pub condition_ids: Vec<String>,
    #[serde(default)]
    pub exception_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeDecompositionAlternative {
    pub id: ScopeAlternativeId,
    pub kind: ScopeDecompositionKind,
    /// Frozen saved draft with authoritative entity IDs; never an unresolved
    /// command draft or an implicitly generated decomposition.
    pub material: ResolvedCandidateDraft,
    pub material_digest: String,
    pub coverage: Vec<ObligationCoverage>,
}

/// A caller-authored full resolved alternative before the domain binds it to
/// the frozen source and assigns its authoritative advisory identity.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct SourceAuthoredScopeAlternative {
    /// Request-local key used only to select the deterministic baseline.
    pub key: String,
    pub kind: ScopeDecompositionKind,
    pub material: ResolvedCandidateDraft,
    pub coverage: Vec<ObligationCoverage>,
}

/// Complete input to the pure source-authored manifest constructor.
///
/// `constructor` is required because this layer has no authority to infer an
/// implementation identity or revision. The source and alternatives must
/// already have been loaded and resolved by their authoritative caller.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct BuildSourceAuthoredScopeManifest {
    pub constructor: ScopeConstructorIdentity,
    pub source: FrozenScopeSource,
    pub obligations: Vec<SourceObligation>,
    pub alternatives: Vec<SourceAuthoredScopeAlternative>,
    pub baseline_key: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct RejectedScopeAlternative {
    pub alternative: ScopeDecompositionAlternative,
    pub reason_codes: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeConstructorIdentity {
    pub id: String,
    pub version: String,
    pub digest: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeConstructorManifest {
    pub constructor: ScopeConstructorIdentity,
    pub source: FrozenScopeSource,
    pub obligations: Vec<SourceObligation>,
    pub emitted: Vec<ScopeDecompositionAlternative>,
    #[serde(default)]
    pub rejected: Vec<RejectedScopeAlternative>,
    pub baseline_id: ScopeAlternativeId,
    pub ordered_ids: Vec<ScopeAlternativeId>,
    pub eligible_set_digest: String,
    pub whole_set_digest: String,
}

impl ScopeConstructorManifest {
    pub fn validate(&self, digest: &impl ScopeDigest) -> Result<()> {
        self.source.validate(digest)?;
        self.constructor.validate()?;
        if self.emitted.is_empty() {
            return Err(Error::InvalidArguments);
        }
        validate_obligations(&self.source, &self.obligations)?;
        if !ordered_alternatives(&self.emitted)
            || !ordered_alternatives(
                &self
                    .rejected
                    .iter()
                    .map(|value| value.alternative.clone())
                    .collect::<Vec<_>>(),
            )
        {
            return Err(Error::InvalidArguments);
        }
        let mut by_id = BTreeMap::new();
        let mut material_digests = BTreeSet::new();
        for alternative in &self.emitted {
            validate_alternative(digest, self, alternative)?;
            if by_id.insert(alternative.id.clone(), true).is_some()
                || !material_digests.insert(&alternative.material_digest)
            {
                return Err(Error::InvalidArguments);
            }
        }
        for rejected in &self.rejected {
            validate_alternative(digest, self, &rejected.alternative)?;
            if rejected.reason_codes.is_empty()
                || rejected.reason_codes.iter().any(|value| !valid_id(value))
                || !material_digests.insert(&rejected.alternative.material_digest)
                || by_id
                    .insert(rejected.alternative.id.clone(), false)
                    .is_some()
            {
                return Err(Error::InvalidArguments);
            }
        }
        if by_id.get(&self.baseline_id) != Some(&true) {
            return Err(Error::InvalidArguments);
        }
        let expected_ids = by_id.keys().cloned().collect::<Vec<_>>();
        if self.ordered_ids != expected_ids
            || self.eligible_set_digest != self.canonical_eligible_set_digest(digest)?
            || self.whole_set_digest != self.canonical_whole_set_digest(digest)?
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    pub fn canonical_eligible_set_digest(&self, digest: &impl ScopeDigest) -> Result<String> {
        let values = self
            .emitted
            .iter()
            .map(|value| (&value.id, &value.material_digest))
            .collect::<Vec<_>>();
        canonical_digest(
            digest,
            "tect.scope-eligible-set/2",
            &(&self.source.digest, &self.constructor, values),
        )
    }

    pub fn canonical_whole_set_digest(&self, digest: &impl ScopeDigest) -> Result<String> {
        let obligations = self
            .obligations
            .iter()
            .cloned()
            .map(|mut value| {
                value
                    .conditions
                    .sort_by(|left, right| left.id.cmp(&right.id));
                value
                    .exceptions
                    .sort_by(|left, right| left.id.cmp(&right.id));
                value
            })
            .collect::<Vec<_>>();
        let emitted = self
            .emitted
            .iter()
            .cloned()
            .map(canonical_alternative)
            .collect::<Vec<_>>();
        let rejected = self
            .rejected
            .iter()
            .cloned()
            .map(|mut value| {
                value.alternative = canonical_alternative(value.alternative);
                value
            })
            .collect::<Vec<_>>();
        canonical_digest(
            digest,
            "tect.scope-constructor-manifest/2",
            &(
                &self.constructor,
                &self.source,
                obligations,
                emitted,
                rejected,
                &self.baseline_id,
                &self.ordered_ids,
                &self.eligible_set_digest,
            ),
        )
    }

    pub fn eligible(&self, id: &ScopeAlternativeId) -> Option<&ScopeDecompositionAlternative> {
        self.emitted.iter().find(|value| &value.id == id)
    }
}

impl ScopeConstructorIdentity {
    pub fn validate(&self) -> Result<()> {
        if !valid_id(&self.id) || !valid_id(&self.version) || !valid_digest(&self.digest) {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

/// Resolve agent-authored alternatives into a deterministic eligible manifest.
/// Candidate content is preserved byte-for-byte at the value level: this
/// function validates and binds supplied material but never constructs or
/// splits candidate content from the source.
pub fn build_source_authored_scope_manifest(
    digest: &impl ScopeDigest,
    input: BuildSourceAuthoredScopeManifest,
) -> Result<ScopeConstructorManifest> {
    input.source.validate(digest)?;
    input.constructor.validate()?;
    validate_obligations(&input.source, &input.obligations)?;
    if !valid_id(&input.baseline_key)
        || input.alternatives.is_empty()
        || input.alternatives.len() > 100
    {
        return Err(Error::InvalidArguments);
    }

    let mut keys = BTreeSet::new();
    let mut by_key = BTreeMap::new();
    let mut emitted = Vec::with_capacity(input.alternatives.len());
    for alternative in input.alternatives {
        if !valid_id(&alternative.key) || !keys.insert(alternative.key.clone()) {
            return Err(Error::InvalidArguments);
        }
        alternative.material.validate()?;
        let coverage = canonical_coverage(&alternative.coverage);
        validate_coverage(&input.obligations, &coverage)?;
        let material_digest = scope_candidate_material_digest(digest, &alternative.material)?;
        let id = stable_scope_alternative_id(
            digest,
            &input.constructor,
            &input.source.digest,
            alternative.kind,
            &material_digest,
            &coverage,
        )?;
        if by_key.insert(alternative.key, id.clone()).is_some() {
            return Err(Error::InvalidArguments);
        }
        emitted.push(ScopeDecompositionAlternative {
            id,
            kind: alternative.kind,
            material: alternative.material,
            material_digest,
            coverage,
        });
    }
    emitted.sort_by(|left, right| left.id.cmp(&right.id));
    let baseline_id = by_key
        .get(&input.baseline_key)
        .cloned()
        .ok_or(Error::InvalidArguments)?;
    let ordered_ids = emitted
        .iter()
        .map(|value| value.id.clone())
        .collect::<Vec<_>>();
    let mut manifest = ScopeConstructorManifest {
        constructor: input.constructor,
        source: input.source,
        obligations: input.obligations,
        emitted,
        rejected: Vec::new(),
        baseline_id,
        ordered_ids,
        eligible_set_digest: String::new(),
        whole_set_digest: String::new(),
    };
    manifest.eligible_set_digest = manifest.canonical_eligible_set_digest(digest)?;
    manifest.whole_set_digest = manifest.canonical_whole_set_digest(digest)?;
    manifest.validate(digest)?;
    Ok(manifest)
}

pub fn scope_candidate_material_digest(
    digest: &impl ScopeDigest,
    material: &ResolvedCandidateDraft,
) -> Result<String> {
    canonical_digest(digest, "tect.scope-candidate-material/2", material)
}

pub fn stable_scope_alternative_id(
    digest: &impl ScopeDigest,
    constructor: &ScopeConstructorIdentity,
    source_digest: &str,
    kind: ScopeDecompositionKind,
    material_digest: &str,
    coverage: &[ObligationCoverage],
) -> Result<ScopeAlternativeId> {
    if !valid_digest(source_digest) || !valid_digest(material_digest) {
        return Err(Error::InvalidArguments);
    }
    let coverage = canonical_coverage(coverage);
    Ok(ScopeAlternativeId(canonical_digest(
        digest,
        "tect.scope-alternative-id/2",
        &(constructor, source_digest, kind, material_digest, coverage),
    )?))
}

fn validate_alternative(
    digest: &impl ScopeDigest,
    manifest: &ScopeConstructorManifest,
    alternative: &ScopeDecompositionAlternative,
) -> Result<()> {
    alternative.id.validate()?;
    alternative.material.validate()?;
    if alternative.material_digest
        != scope_candidate_material_digest(digest, &alternative.material)?
    {
        return Err(Error::InputConflict);
    }
    let expected_id = stable_scope_alternative_id(
        digest,
        &manifest.constructor,
        &manifest.source.digest,
        alternative.kind,
        &alternative.material_digest,
        &alternative.coverage,
    )?;
    if alternative.id != expected_id {
        return Err(Error::InputConflict);
    }
    validate_coverage(&manifest.obligations, &alternative.coverage)
}

fn validate_coverage(
    obligations: &[SourceObligation],
    coverage: &[ObligationCoverage],
) -> Result<()> {
    let mut rows = BTreeMap::new();
    for row in coverage {
        if rows.insert(&row.obligation_id, row).is_some()
            || row.condition_ids.iter().collect::<BTreeSet<_>>().len() != row.condition_ids.len()
            || row.exception_ids.iter().collect::<BTreeSet<_>>().len() != row.exception_ids.len()
        {
            return Err(Error::InvalidArguments);
        }
    }
    if rows.len() != obligations.len() {
        return Err(Error::InvalidSource);
    }
    for obligation in obligations {
        let row = rows.get(&obligation.id).ok_or(Error::InvalidSource)?;
        let conditions = obligation
            .conditions
            .iter()
            .map(|value| &value.id)
            .collect::<BTreeSet<_>>();
        let exceptions = obligation
            .exceptions
            .iter()
            .map(|value| &value.id)
            .collect::<BTreeSet<_>>();
        if row.condition_ids.iter().collect::<BTreeSet<_>>() != conditions
            || row.exception_ids.iter().collect::<BTreeSet<_>>() != exceptions
        {
            return Err(Error::InvalidSource);
        }
    }
    Ok(())
}

fn ordered_alternatives(values: &[ScopeDecompositionAlternative]) -> bool {
    values.windows(2).all(|pair| pair[0].id < pair[1].id)
}

fn canonical_coverage(values: &[ObligationCoverage]) -> Vec<ObligationCoverage> {
    let mut values = values.to_vec();
    for value in &mut values {
        value.condition_ids.sort();
        value.exception_ids.sort();
    }
    values.sort_by(|left, right| left.obligation_id.cmp(&right.obligation_id));
    values
}

fn canonical_alternative(
    mut value: ScopeDecompositionAlternative,
) -> ScopeDecompositionAlternative {
    value.coverage = canonical_coverage(&value.coverage);
    value
}
