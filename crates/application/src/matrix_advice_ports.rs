use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{
    AdvisoryDispatch, AdvisoryModelConfiguration, AdvisoryOpportunity, AdvisoryProviderProfileRef,
    Error, MatrixAdviceEligibility, MatrixRanking, Result,
};
use uuid::Uuid;

use crate::{
    AdvisoryLifecycleCapability, MatrixProviderBinding, MatrixProviderRequest,
    MatrixProviderResponse,
};

const ADVICE_DIGEST_DOMAIN: &[u8] = b"tect.guarded-matrix-advice/1\0";

/// Durable evidence of one guarded Matrix dispatch. A ranking is advisory only;
/// selection belongs to the later, separate disposition record.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct GuardedMatrixAdviceRecord {
    pub opportunity_id: Uuid,
    pub dispatch_id: Uuid,
    pub opportunity_material_digest: String,
    pub binding: MatrixProviderBinding,
    pub provider_profile_ref: AdvisoryProviderProfileRef,
    pub model_configuration: AdvisoryModelConfiguration,
    /// Exact opaque bytes returned by the provider; never reserialize parsed
    /// response data as evidence of what was received.
    pub raw_response_payload: Vec<u8>,
    pub response_payload_sha256: String,
    /// Canonical digest of schema version, full binding, and typed outcome.
    /// It deliberately excludes occurrence IDs and raw transport bytes.
    pub advice_digest: String,
    pub outcome: GuardedMatrixAdviceOutcome,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub enum GuardedMatrixAdviceOutcome {
    Ranked {
        ranked_choice_ids: Vec<String>,
    },
    Abstained {
        reason: Option<String>,
    },
    /// A rejected response yields no usable advice and no ranking.
    Rejected {
        reason: String,
    },
}

impl GuardedMatrixAdviceRecord {
    /// Construct a usable outcome only after the typed response passes the
    /// request's binding, provider identity, raw-byte hash and choice-set guard.
    pub fn from_provider_response(
        opportunity_id: Uuid,
        dispatch_id: Uuid,
        request: &MatrixProviderRequest,
        response: MatrixProviderResponse,
    ) -> Result<Self> {
        response.validate_for(request)?;
        let outcome = match response.ranking {
            MatrixRanking::Ranked {
                ranked_candidate_ids,
                ..
            } => GuardedMatrixAdviceOutcome::Ranked {
                ranked_choice_ids: ranked_candidate_ids,
            },
            MatrixRanking::Abstained { .. } => {
                GuardedMatrixAdviceOutcome::Abstained { reason: None }
            }
        };
        let binding = request.binding().clone();
        let advice_digest = canonical_matrix_advice_digest(&binding, &outcome)?;
        let record = Self {
            opportunity_id,
            dispatch_id,
            opportunity_material_digest: binding.evaluation_digest.clone(),
            binding,
            provider_profile_ref: response.provider_profile_ref,
            model_configuration: response.model_configuration,
            raw_response_payload: response.raw_response_payload,
            response_payload_sha256: response.response_payload_sha256,
            advice_digest,
            outcome,
        };
        record.validate_for(
            opportunity_id,
            dispatch_id,
            request.binding(),
            request.provider_profile_ref(),
            request.model_configuration(),
            request.eligibility(),
        )?;
        Ok(record)
    }

    /// Validate against the fresh opportunity, dispatch and saved Matrix
    /// revision/evaluation binding that the store must lock and read.
    pub fn validate_for(
        &self,
        opportunity_id: Uuid,
        dispatch_id: Uuid,
        binding: &MatrixProviderBinding,
        provider_profile_ref: &AdvisoryProviderProfileRef,
        model_configuration: &AdvisoryModelConfiguration,
        eligibility: &MatrixAdviceEligibility,
    ) -> Result<()> {
        if self.opportunity_id.is_nil()
            || self.dispatch_id.is_nil()
            || self.opportunity_id != opportunity_id
            || self.dispatch_id != dispatch_id
            || self.binding != *binding
            || self.provider_profile_ref != *provider_profile_ref
            || self.model_configuration != *model_configuration
            || self.provider_profile_ref.validate().is_err()
            || self.model_configuration.validate().is_err()
            || self.binding.task_id.is_nil()
            || self.binding.task_revision < 1
            || self.opportunity_material_digest != binding.evaluation_digest
            || !is_digest(&binding.input_digest)
            || !is_digest(&binding.choice_set_digest)
            || !is_digest(&binding.evaluation_digest)
            || binding
                .verification_digest
                .as_deref()
                .is_some_and(|digest| !is_digest(digest))
            || self.response_payload_sha256
                != format!("{:x}", Sha256::digest(&self.raw_response_payload))
            || self.advice_digest != canonical_matrix_advice_digest(binding, &self.outcome)?
        {
            return Err(Error::InvalidArguments);
        }
        match &self.outcome {
            GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
                let ranking = MatrixRanking::Ranked {
                    ranked_candidate_ids: ranked_choice_ids.clone(),
                    recommended_candidate_id: ranked_choice_ids
                        .first()
                        .cloned()
                        .ok_or(Error::InvalidArguments)?,
                };
                ranking.validate(eligibility)
            }
            GuardedMatrixAdviceOutcome::Abstained { reason } => {
                valid_reason(reason.as_deref())?;
                MatrixRanking::Abstained {
                    ranked_candidate_ids: Vec::new(),
                    recommended_candidate_id: None,
                }
                .validate(eligibility)
            }
            GuardedMatrixAdviceOutcome::Rejected { reason } => {
                valid_reason(Some(reason))?;
                if !matches!(
                    eligibility,
                    MatrixAdviceEligibility::EligibleForAdvice { .. }
                ) {
                    return Err(Error::InvalidArguments);
                }
                Ok(())
            }
        }
    }
}

/// Stable, domain-separated SHA-256. JSON object keys are serialized in sorted
/// order by serde_json; array order (including rank order) is retained.
pub fn canonical_matrix_advice_digest(
    binding: &MatrixProviderBinding,
    outcome: &GuardedMatrixAdviceOutcome,
) -> Result<String> {
    let outcome = match outcome {
        GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
            serde_json::json!({"status": "ranked", "ranked_choice_ids": ranked_choice_ids})
        }
        GuardedMatrixAdviceOutcome::Abstained { reason } => {
            serde_json::json!({"status": "abstained", "reason": reason})
        }
        GuardedMatrixAdviceOutcome::Rejected { reason } => {
            serde_json::json!({"status": "rejected", "reason": reason})
        }
    };
    let mut material = serde_json::json!({
        "schema_version": if binding.verification_digest.is_some() { 2 } else { 1 },
        "binding": {
            "task_id": binding.task_id,
            "task_revision": binding.task_revision,
            "input_digest": binding.input_digest,
            "choice_set_id": binding.choice_set_id,
            "choice_set_version": binding.choice_set_version,
            "choice_set_digest": binding.choice_set_digest,
            "evaluation_digest": binding.evaluation_digest,
        },
        "outcome": outcome,
    });
    if let Some(digest) = &binding.verification_digest {
        if !is_digest(digest) {
            return Err(Error::InvalidArguments);
        }
        material["binding"]["verification_digest"] = serde_json::json!(digest);
    }
    let encoded = serde_json::to_vec(&material).map_err(|_| Error::InternalInvariant)?;
    let mut hasher = Sha256::new();
    hasher.update(ADVICE_DIGEST_DOMAIN);
    hasher.update(encoded);
    Ok(format!("{:x}", hasher.finalize()))
}

fn is_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
}

fn valid_reason(reason: Option<&str>) -> Result<()> {
    if reason.is_some_and(|value| value.trim().is_empty()) {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct StoredGuardedMatrixAdviceRecord {
    pub advice_id: Uuid,
    pub record: GuardedMatrixAdviceRecord,
}

/// The adapter must lock the opportunity and dispatch, then revalidate the
/// sealed persisted dispatch request bytes, digest, provider identity,
/// configuration, lifecycle state and exact saved revision/evaluation. For a
/// ranked or abstained outcome, the dispatch must be sealed and sent, have a
/// `ProviderResponse` outcome and non-null `response_payload`, and its
/// persisted payload must byte-for-byte equal `record.raw_response_payload`;
/// the payload hash is checked as an adjunct, not a substitute. Then validate
/// the response evidence and outcome against the owner choice set.
/// Caller-supplied record fields alone never prove a provider call or a
/// permitted dispatch.
/// Exact replay of the same complete record for the same occurrence returns the
/// stored row; a changed record, duplicate occurrence under another advice ID,
/// or reused advice ID for another occurrence returns `Error::InputConflict`.
/// `Rejected` maps to the failed guard outcome and must never yield usable
/// advice, a ranking or a disposition. The store must not derive a disposition
/// here.
#[async_trait]
pub trait MatrixAdviceStore: Send {
    async fn guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<StoredGuardedMatrixAdviceRecord>>;

    async fn persist_guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
    ) -> Result<StoredGuardedMatrixAdviceRecord>;

    /// Finalize a sealed Matrix dispatch and persist its advice using the
    /// same locked verification decision. A direct persistence call still
    /// checks evidence freshness at the time of that separate call.
    // The application dispatch/recovery callers and PostgreSQL implementation
    // share this stable transaction port; grouping arguments would change its
    // cross-crate contract without changing the guarded operation.
    #[allow(clippy::too_many_arguments)]
    async fn finalize_guarded_matrix_advice(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatch,
        record: Option<&GuardedMatrixAdviceRecord>,
        verification_stale: bool,
    ) -> Result<AdvisoryOpportunity>;
}

#[cfg(test)]
mod tests {
    use super::*;

    fn binding() -> MatrixProviderBinding {
        MatrixProviderBinding {
            task_id: Uuid::new_v4(),
            task_revision: 2,
            input_digest: "a".repeat(64),
            choice_set_id: "set-1".into(),
            choice_set_version: 1,
            choice_set_digest: "b".repeat(64),
            evaluation_digest: "c".repeat(64),
            verification_digest: None,
        }
    }

    fn record(outcome: GuardedMatrixAdviceOutcome) -> GuardedMatrixAdviceRecord {
        let binding = binding();
        let advice_digest = canonical_matrix_advice_digest(&binding, &outcome).unwrap();
        let raw_response_payload = b"opaque provider response".to_vec();
        GuardedMatrixAdviceRecord {
            opportunity_id: Uuid::new_v4(),
            dispatch_id: Uuid::new_v4(),
            opportunity_material_digest: binding.evaluation_digest.clone(),
            binding,
            provider_profile_ref: AdvisoryProviderProfileRef {
                id: "provider".into(),
            },
            model_configuration: AdvisoryModelConfiguration {
                model: "model".into(),
            },
            response_payload_sha256: format!("{:x}", Sha256::digest(&raw_response_payload)),
            raw_response_payload,
            advice_digest,
            outcome,
        }
    }

    fn eligibility() -> MatrixAdviceEligibility {
        MatrixAdviceEligibility::EligibleForAdvice {
            candidate_ids: vec!["a".into(), "b".into()],
        }
    }

    fn validate(record: &GuardedMatrixAdviceRecord) -> Result<()> {
        record.validate_for(
            record.opportunity_id,
            record.dispatch_id,
            &record.binding,
            &record.provider_profile_ref,
            &record.model_configuration,
            &eligibility(),
        )
    }

    #[test]
    fn ranked_requires_exact_permutation() {
        let mut record = record(GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["b".into(), "a".into()],
        });
        assert_eq!(validate(&record), Ok(()));
        record.outcome = GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "a".into()],
        };
        record.advice_digest =
            canonical_matrix_advice_digest(&record.binding, &record.outcome).unwrap();
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }

    #[test]
    fn abstention_is_usable_without_a_rank() {
        let record = record(GuardedMatrixAdviceOutcome::Abstained { reason: None });
        assert_eq!(validate(&record), Ok(()));
    }

    #[test]
    fn rejection_has_no_usable_rank_and_requires_reason() {
        let mut record = record(GuardedMatrixAdviceOutcome::Rejected {
            reason: "invalid provider response".into(),
        });
        assert_eq!(validate(&record), Ok(()));
        record.outcome = GuardedMatrixAdviceOutcome::Rejected { reason: " ".into() };
        record.advice_digest =
            canonical_matrix_advice_digest(&record.binding, &record.outcome).unwrap();
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }

    #[test]
    fn rejects_mismatched_binding_and_opportunity_material() {
        let mut record = record(GuardedMatrixAdviceOutcome::Abstained { reason: None });
        let expected = record.binding.clone();
        record.binding.task_revision += 1;
        assert_eq!(
            record.validate_for(
                record.opportunity_id,
                record.dispatch_id,
                &expected,
                &record.provider_profile_ref,
                &record.model_configuration,
                &eligibility()
            ),
            Err(Error::InvalidArguments)
        );
        record.binding = expected;
        record.opportunity_material_digest = "d".repeat(64);
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }

    #[test]
    fn digest_is_stable_separates_outcomes_and_preserves_rank_order() {
        let binding = binding();
        let ranked = GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "b".into()],
        };
        let same = canonical_matrix_advice_digest(&binding, &ranked).unwrap();
        assert_eq!(
            same,
            canonical_matrix_advice_digest(&binding, &ranked).unwrap()
        );
        assert_ne!(
            same,
            canonical_matrix_advice_digest(
                &binding,
                &GuardedMatrixAdviceOutcome::Ranked {
                    ranked_choice_ids: vec!["b".into(), "a".into()],
                },
            )
            .unwrap()
        );
        assert_ne!(
            same,
            canonical_matrix_advice_digest(
                &binding,
                &GuardedMatrixAdviceOutcome::Abstained { reason: None },
            )
            .unwrap()
        );
        let mut changed_binding = binding.clone();
        changed_binding.choice_set_version += 1;
        assert_ne!(
            same,
            canonical_matrix_advice_digest(&changed_binding, &ranked).unwrap()
        );
        let first = record(ranked.clone());
        let mut second = first.clone();
        second.opportunity_id = Uuid::new_v4();
        second.dispatch_id = Uuid::new_v4();
        assert_eq!(first.advice_digest, second.advice_digest);
        let mut verified_binding = binding.clone();
        verified_binding.verification_digest = Some("d".repeat(64));
        let verified = canonical_matrix_advice_digest(&verified_binding, &ranked).unwrap();
        assert_ne!(same, verified);
        verified_binding.verification_digest = Some("e".repeat(64));
        assert_ne!(
            verified,
            canonical_matrix_advice_digest(&verified_binding, &ranked).unwrap()
        );
        verified_binding.verification_digest = Some("not-a-digest".into());
        assert_eq!(
            canonical_matrix_advice_digest(&verified_binding, &ranked),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn record_rejects_raw_response_hash_and_advice_digest_mismatch() {
        let mut record = record(GuardedMatrixAdviceOutcome::Abstained { reason: None });
        record.raw_response_payload.push(b'!');
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
        record.response_payload_sha256 =
            format!("{:x}", Sha256::digest(&record.raw_response_payload));
        record.advice_digest = "0".repeat(64);
        assert_eq!(validate(&record), Err(Error::InvalidArguments));
    }

    #[test]
    fn record_rejects_wrong_expected_provider_identity() {
        let record = record(GuardedMatrixAdviceOutcome::Rejected {
            reason: "invalid provider response".into(),
        });
        assert_eq!(
            record.validate_for(
                record.opportunity_id,
                record.dispatch_id,
                &record.binding,
                &AdvisoryProviderProfileRef { id: "other".into() },
                &record.model_configuration,
                &eligibility(),
            ),
            Err(Error::InvalidArguments)
        );
    }
}
