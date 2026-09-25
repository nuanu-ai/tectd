//! Immutable disposition of one saved pre-open recommendation. This is a
//! planning decision only; it cannot open a Slice or attest verification.

use crate::{
    Error, PipelineKind, PipelineRecommendationDisposition, PipelineRecommendationManifest,
    PipelineRecommendationRanking, Result, SliceCandidateNode,
};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineDispositionRequest {
    pub request_id: Uuid,
    pub opportunity_id: Uuid,
    pub expected_work_revision: i64,
    pub manifest_digest: String,
    pub action: PipelineRecommendationDisposition,
    pub rationale: String,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(tag = "status", rename_all = "snake_case", deny_unknown_fields)]
pub enum PipelineDispositionAdvice {
    NoCall,
    Ranked {
        dispatch_id: Uuid,
        ranked_ids: Vec<String>,
    },
    Abstained {
        dispatch_id: Uuid,
    },
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineDispositionResult {
    pub id: Uuid,
    pub request: PipelineDispositionRequest,
    pub work_id: Uuid,
    pub advice: PipelineDispositionAdvice,
    pub selected_kind: Option<PipelineKind>,
    pub selected_option_id: Option<String>,
}

impl PipelineDispositionRequest {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.opportunity_id.is_nil()
            || self.expected_work_revision < 1
            || !sha256(&self.manifest_digest)
            || self.rationale.trim().is_empty()
            || self.rationale.len() > 4096
            || self.rationale.chars().any(|c| c == '\0')
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }

    pub fn resolve(
        &self,
        id: Uuid,
        manifest: &PipelineRecommendationManifest,
        saved_work: &SliceCandidateNode,
        advice: &PipelineDispositionAdvice,
    ) -> Result<PipelineDispositionResult> {
        self.validate()?;
        manifest.validate_digest()?;
        let SliceCandidateNode::Work {
            id: work_id,
            revision,
            pipeline,
            ..
        } = saved_work
        else {
            return Err(Error::InputConflict);
        };
        if id.is_nil()
            || *work_id != manifest.work_id
            || *revision != manifest.work_revision
            || *pipeline != manifest.deterministic_kind
            || self.expected_work_revision != *revision
            || self.manifest_digest != manifest.digest
        {
            return Err(Error::InputConflict);
        }
        let selected_option_id = match advice {
            PipelineDispositionAdvice::NoCall => {
                if self.action != PipelineRecommendationDisposition::UseDeterministicChoice {
                    return Err(Error::InvalidArguments);
                }
                Some(require_eligible_baseline(manifest, *pipeline)?)
            }
            PipelineDispositionAdvice::Abstained { dispatch_id } => {
                if dispatch_id.is_nil()
                    || self.action != PipelineRecommendationDisposition::UseDeterministicChoice
                {
                    return Err(Error::InvalidArguments);
                }
                Some(require_eligible_baseline(manifest, *pipeline)?)
            }
            PipelineDispositionAdvice::Ranked {
                dispatch_id,
                ranked_ids,
            } => {
                if dispatch_id.is_nil() {
                    return Err(Error::InvalidArguments);
                }
                PipelineRecommendationRanking::Ranked {
                    ranked_ids: ranked_ids.clone(),
                }
                .validate(manifest)?;
                match self.action {
                    PipelineRecommendationDisposition::AcceptRecommendation => {
                        let top = &ranked_ids[0];
                        Some(top.clone())
                    }
                    PipelineRecommendationDisposition::RejectRecommendation => None,
                    PipelineRecommendationDisposition::UseDeterministicChoice => {
                        Some(require_eligible_baseline(manifest, *pipeline)?)
                    }
                }
            }
        };
        let selected_kind = selected_option_id.as_ref().map(|id| {
            manifest
                .options
                .iter()
                .find(|option| &option.id == id)
                .expect("validated eligible pair")
                .kind
        });
        Ok(PipelineDispositionResult {
            id,
            request: self.clone(),
            work_id: *work_id,
            advice: advice.clone(),
            selected_kind,
            selected_option_id,
        })
    }
}

fn require_eligible_baseline(
    manifest: &PipelineRecommendationManifest,
    baseline: PipelineKind,
) -> Result<String> {
    let id = manifest
        .deterministic_option_id
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let option = manifest
        .options
        .iter()
        .find(|option| &option.id == id)
        .ok_or(Error::InvalidArguments)?;
    if option.kind != baseline {
        return Err(Error::InvalidArguments);
    }
    Ok(id.clone())
}

fn sha256(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        PIPELINE_RECOMMENDATION_SCHEMA, PIPELINE_VERIFICATION_PLAN_SCHEMA, PipelineExcludedKind,
        PipelineExclusionReason, PipelineRecommendationOption, PipelineVerificationObligation,
        PipelineVerificationPlan,
    };
    use sha2::{Digest, Sha256};

    fn fixture() -> (
        PipelineRecommendationManifest,
        SliceCandidateNode,
        PipelineDispositionRequest,
    ) {
        let work_id = Uuid::new_v4();
        let kind = PipelineKind::LightweightTddDevelopment;
        let other = PipelineKind::FullDesignToExecution;
        let mut manifest = PipelineRecommendationManifest {
            schema: PIPELINE_RECOMMENDATION_SCHEMA.into(),
            work_id,
            work_revision: 2,
            matrix_task_id: "task".into(),
            matrix_task_revision: "3".into(),
            selected_choice_id: "choice".into(),
            matrix_choice_set_digest: "a".repeat(64),
            matrix_verification_digest: "b".repeat(64),
            matrix_input_digest: "c".repeat(64),
            selected_candidate_digest: "d".repeat(64),
            compatibility_policy_digest: "e".repeat(64),
            mandatory_card_ids: vec!["card".into()],
            deterministic_kind: kind,
            deterministic_option_id: None,
            catalogue_revision: "4".into(),
            catalogue_digest: "catalogue".into(),
            options: [kind, other]
                .into_iter()
                .map(|kind| {
                    let mut plan = PipelineVerificationPlan {
                        schema: PIPELINE_VERIFICATION_PLAN_SCHEMA.into(),
                        id: String::new(),
                        digest: String::new(),
                        source_kind: kind,
                        source_definition_version: "1".into(),
                        source_definition_digest: "definition".into(),
                        obligations: vec![PipelineVerificationObligation {
                            phase_id: "proof".into(),
                            required_fields: vec!["proof".into()],
                            required_artifacts: vec![],
                            validator_contracts: vec![],
                            output_constraints: vec![],
                            allowed_verdicts: vec![],
                            verdict_routes: vec![],
                            disposition_required: false,
                            required_dispositions: vec![],
                            fresh_reviewer_input: false,
                            output_contract: "proof".into(),
                        }],
                    };
                    plan.digest = plan.content_digest().unwrap();
                    plan.id = format!("verification-plan:{}", plan.digest);
                    PipelineRecommendationOption {
                        id: PipelineRecommendationOption::pair_id(kind, &plan.id),
                        kind,
                        definition_version: "1".into(),
                        definition_digest: "definition".into(),
                        completion_contract: "proof".into(),
                        forbidden_claims: vec![],
                        verification_plan: plan,
                    }
                })
                .collect(),
            excluded: PipelineKind::CURRENT_SLICE_RUN_KINDS
                .into_iter()
                .filter(|candidate| *candidate != kind && *candidate != other)
                .map(|kind| PipelineExcludedKind {
                    kind,
                    reason: PipelineExclusionReason::MissingRule,
                })
                .collect(),
            evidence_refs: vec![],
            digest: String::new(),
        };
        manifest.deterministic_option_id = Some(manifest.options[0].id.clone());
        manifest.digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&manifest).unwrap())
        );
        let work = SliceCandidateNode::Work {
            model_route_facts: None,
            id: work_id,
            revision: 2,
            title: "work".into(),
            outcome: "outcome".into(),
            includes: vec![],
            excludes: vec![],
            dependencies: vec![],
            proof: vec![],
            pipeline: kind,
            pipeline_reason: "saved".into(),
            why_lightweight_insufficient: None,
            why_further_vertical_split_not_viable: None,
            source_result_ids: vec![],
            source_checkpoint: None,
        };
        let request = PipelineDispositionRequest {
            request_id: Uuid::new_v4(),
            opportunity_id: Uuid::new_v4(),
            expected_work_revision: 2,
            manifest_digest: manifest.digest.clone(),
            action: PipelineRecommendationDisposition::AcceptRecommendation,
            rationale: "reviewed".into(),
        };
        (manifest, work, request)
    }

    #[test]
    fn accept_uses_saved_top_rank_and_reject_has_no_selection() {
        let (manifest, work, mut request) = fixture();
        let ranked = PipelineDispositionAdvice::Ranked {
            dispatch_id: Uuid::new_v4(),
            ranked_ids: manifest
                .options
                .iter()
                .rev()
                .map(|option| option.id.clone())
                .collect(),
        };
        let accepted = request
            .resolve(Uuid::new_v4(), &manifest, &work, &ranked)
            .unwrap();
        assert_eq!(accepted.selected_kind, Some(manifest.options[1].kind));
        request.action = PipelineRecommendationDisposition::RejectRecommendation;
        assert_eq!(
            request
                .resolve(Uuid::new_v4(), &manifest, &work, &ranked)
                .unwrap()
                .selected_kind,
            None
        );
        let mut unknown = ranked;
        if let PipelineDispositionAdvice::Ranked { ranked_ids, .. } = &mut unknown {
            ranked_ids[0] = "unknown".into();
        }
        assert_eq!(
            request.resolve(Uuid::new_v4(), &manifest, &work, &unknown),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn explicit_baseline_works_for_no_call_and_abstention_only_while_bound() {
        let (manifest, work, mut request) = fixture();
        request.action = PipelineRecommendationDisposition::UseDeterministicChoice;
        for advice in [
            PipelineDispositionAdvice::NoCall,
            PipelineDispositionAdvice::Abstained {
                dispatch_id: Uuid::new_v4(),
            },
        ] {
            assert_eq!(
                request
                    .resolve(Uuid::new_v4(), &manifest, &work, &advice)
                    .unwrap()
                    .selected_kind,
                Some(PipelineKind::LightweightTddDevelopment)
            );
        }
        request.expected_work_revision += 1;
        assert_eq!(
            request.resolve(
                Uuid::new_v4(),
                &manifest,
                &work,
                &PipelineDispositionAdvice::NoCall
            ),
            Err(Error::InputConflict)
        );
        request.expected_work_revision -= 1;
        request.action = PipelineRecommendationDisposition::AcceptRecommendation;
        assert_eq!(
            request.resolve(
                Uuid::new_v4(),
                &manifest,
                &work,
                &PipelineDispositionAdvice::NoCall
            ),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn excluded_baseline_cannot_be_used_even_without_provider_call() {
        let (mut manifest, work, mut request) = fixture();
        manifest.options.clear();
        manifest.deterministic_option_id = None;
        manifest.excluded = PipelineKind::CURRENT_SLICE_RUN_KINDS
            .into_iter()
            .map(|kind| PipelineExcludedKind {
                kind,
                reason: PipelineExclusionReason::MissingRule,
            })
            .collect();
        manifest.digest.clear();
        manifest.digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&manifest).unwrap())
        );
        request.manifest_digest = manifest.digest.clone();
        request.action = PipelineRecommendationDisposition::UseDeterministicChoice;
        assert!(!manifest.should_call());
        assert_eq!(
            request.resolve(
                Uuid::new_v4(),
                &manifest,
                &work,
                &PipelineDispositionAdvice::NoCall
            ),
            Err(Error::InvalidArguments)
        );
    }
}
