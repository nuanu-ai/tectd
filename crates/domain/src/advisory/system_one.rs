use super::scope_source::{canonical_digest, valid_digest};
use crate::{
    Error, Result, ScopeAlternativeId, ScopeConstructorManifest, ScopeDecompositionKind,
    ScopeDigest,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;

#[derive(Debug, Clone, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ScopeAdviceId(pub String);

impl ScopeAdviceId {
    pub fn validate(&self) -> Result<()> {
        if valid_digest(&self.0) {
            Ok(())
        } else {
            Err(Error::InvalidArguments)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeAdviceAlternative {
    pub id: ScopeAlternativeId,
    pub kind: ScopeDecompositionKind,
    pub material_digest: String,
    pub covered_obligation_ids: Vec<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeAdviceQuestion {
    pub alternative_id: ScopeAlternativeId,
    pub require_choice: bool,
    pub require_score: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct ScopeAdviceRequest {
    pub contract: String,
    pub source_digest: String,
    pub manifest_digest: String,
    pub eligible_set_digest: String,
    pub baseline_id: ScopeAlternativeId,
    pub alternatives: Vec<ScopeAdviceAlternative>,
    pub questions: Vec<ScopeAdviceQuestion>,
    pub digest: String,
}

impl ScopeAdviceRequest {
    pub fn from_manifest(
        digest: &impl ScopeDigest,
        manifest: &ScopeConstructorManifest,
    ) -> Result<Self> {
        manifest.validate(digest)?;
        let alternatives = manifest
            .emitted
            .iter()
            .map(|value| ScopeAdviceAlternative {
                id: value.id.clone(),
                kind: value.kind,
                material_digest: value.material_digest.clone(),
                covered_obligation_ids: {
                    let mut ids = value
                        .coverage
                        .iter()
                        .map(|coverage| coverage.obligation_id.clone())
                        .collect::<Vec<_>>();
                    ids.sort();
                    ids
                },
            })
            .collect::<Vec<_>>();
        let questions = manifest
            .emitted
            .iter()
            .map(|value| ScopeAdviceQuestion {
                alternative_id: value.id.clone(),
                require_choice: true,
                require_score: true,
            })
            .collect();
        let mut request = Self {
            contract: "tect.scope-decomposition-advice/1".into(),
            source_digest: manifest.source.digest.clone(),
            manifest_digest: manifest.whole_set_digest.clone(),
            eligible_set_digest: manifest.eligible_set_digest.clone(),
            baseline_id: manifest.baseline_id.clone(),
            alternatives,
            questions,
            digest: String::new(),
        };
        request.digest = request.canonical_digest(digest)?;
        Ok(request)
    }

    pub fn validate(
        &self,
        digest: &impl ScopeDigest,
        manifest: &ScopeConstructorManifest,
    ) -> Result<()> {
        manifest.validate(digest)?;
        if self != &Self::from_manifest(digest, manifest)?
            || self.digest != self.canonical_digest(digest)?
        {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    fn canonical_digest(&self, digest: &impl ScopeDigest) -> Result<String> {
        canonical_digest(
            digest,
            "tect.scope-advice-request/1",
            &(
                &self.contract,
                &self.source_digest,
                &self.manifest_digest,
                &self.eligible_set_digest,
                &self.baseline_id,
                &self.alternatives,
                &self.questions,
            ),
        )
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeAdviceChoice {
    Preferred,
    NonPreferred,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ScopeAdviceScoreBand {
    Conflict,
    WeakFit,
    Fit,
    StrongFit,
}

impl ScopeAdviceScoreBand {
    pub const fn ordinal(self) -> u8 {
        match self {
            Self::Conflict => 0,
            Self::WeakFit => 1,
            Self::Fit => 2,
            Self::StrongFit => 3,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(transparent)]
pub struct ConfidenceBasisPoints(pub u16);

impl ConfidenceBasisPoints {
    fn validate(self) -> Result<()> {
        if self.0 <= 10_000 {
            Ok(())
        } else {
            Err(Error::InvalidArguments)
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedScopeAdviceAnswer {
    pub alternative_id: ScopeAlternativeId,
    pub choice: ScopeAdviceChoice,
    pub score: ScopeAdviceScoreBand,
    pub choice_confidence: ConfidenceBasisPoints,
    pub score_confidence: ConfidenceBasisPoints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct NormalizedScopeAdviceAnswers {
    pub answers: Vec<NormalizedScopeAdviceAnswer>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardedScopeAdviceItem {
    pub alternative_id: ScopeAlternativeId,
    pub choice: ScopeAdviceChoice,
    pub score: ScopeAdviceScoreBand,
    pub choice_confidence: ConfidenceBasisPoints,
    pub score_confidence: ConfidenceBasisPoints,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct GuardedScopeAdvice {
    pub id: ScopeAdviceId,
    pub request_digest: String,
    pub source_digest: String,
    pub manifest_digest: String,
    pub eligible_set_digest: String,
    pub normalized_answers_digest: String,
    pub items: Vec<GuardedScopeAdviceItem>,
    pub ranked_ids: Vec<ScopeAlternativeId>,
}

pub fn guard_scope_advice(
    digest: &impl ScopeDigest,
    manifest: &ScopeConstructorManifest,
    request: &ScopeAdviceRequest,
    normalized: &NormalizedScopeAdviceAnswers,
) -> Result<GuardedScopeAdvice> {
    request.validate(digest, manifest)?;
    let expected = manifest
        .emitted
        .iter()
        .map(|value| &value.id)
        .collect::<BTreeSet<_>>();
    let actual = normalized
        .answers
        .iter()
        .map(|value| &value.alternative_id)
        .collect::<BTreeSet<_>>();
    if normalized.answers.len() != expected.len() || actual != expected {
        return Err(Error::InvalidArguments);
    }
    for answer in &normalized.answers {
        answer.alternative_id.validate()?;
        answer.choice_confidence.validate()?;
        answer.score_confidence.validate()?;
    }
    let mut canonical_answers = normalized.answers.clone();
    canonical_answers.sort_by(|left, right| left.alternative_id.cmp(&right.alternative_id));
    let normalized_answers_digest = canonical_digest(
        digest,
        "tect.normalized-scope-advice-answers/1",
        &canonical_answers,
    )?;
    let id = ScopeAdviceId(canonical_digest(
        digest,
        "tect.guarded-scope-advice/1",
        &(
            &request.digest,
            &manifest.whole_set_digest,
            &manifest.eligible_set_digest,
            &normalized_answers_digest,
        ),
    )?);
    let mut items = canonical_answers
        .into_iter()
        .map(|answer| GuardedScopeAdviceItem {
            alternative_id: answer.alternative_id,
            choice: answer.choice,
            score: answer.score,
            choice_confidence: answer.choice_confidence,
            score_confidence: answer.score_confidence,
        })
        .collect::<Vec<_>>();
    items.sort_by(|left, right| {
        right
            .score
            .ordinal()
            .cmp(&left.score.ordinal())
            .then_with(|| left.alternative_id.cmp(&right.alternative_id))
    });
    let ranked_ids = items
        .iter()
        .map(|value| value.alternative_id.clone())
        .collect();
    Ok(GuardedScopeAdvice {
        id,
        request_digest: request.digest.clone(),
        source_digest: manifest.source.digest.clone(),
        manifest_digest: manifest.whole_set_digest.clone(),
        eligible_set_digest: manifest.eligible_set_digest.clone(),
        normalized_answers_digest,
        items,
        ranked_ids,
    })
}

pub fn validate_guarded_advice_binding(
    digest: &impl ScopeDigest,
    manifest: &ScopeConstructorManifest,
    advice: &GuardedScopeAdvice,
) -> Result<()> {
    let request = ScopeAdviceRequest::from_manifest(digest, manifest)?;
    let normalized = NormalizedScopeAdviceAnswers {
        answers: advice
            .items
            .iter()
            .map(|item| NormalizedScopeAdviceAnswer {
                alternative_id: item.alternative_id.clone(),
                choice: item.choice,
                score: item.score,
                choice_confidence: item.choice_confidence,
                score_confidence: item.score_confidence,
            })
            .collect(),
    };
    if advice != &guard_scope_advice(digest, manifest, &request, &normalized)? {
        return Err(Error::InputConflict);
    }
    Ok(())
}
