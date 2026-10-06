use super::*;
use tect_application::MatrixTaskRevision;

impl PgUnitOfWork {
    pub(super) async fn validate_disposition_advice(
        &mut self,
        workspace_id: Uuid,
        request: &RecordMatrixDisposition,
        current_advice: Option<&CurrentMatrixAdvice>,
        current_verification: Option<&MatrixDispositionVerification>,
        opportunity: &PgRow,
        current: &MatrixTaskRevision,
    ) -> Result<()> {
        let tenant = self.tenant_id()?;
        let revision = request.expected_task_revision;
        if request.basis == MatrixDispositionBasis::AfterAdvice {
            let token = current_advice.ok_or(Error::StaleContext)?;
            let advice = sqlx::query(
                "SELECT a.advice_id,a.dispatch_id,a.kind,a.ranked_choice_ids,a.reason, \
                        a.advice_digest,a.provider_profile_ref,a.ranking_policy_version,a.trial_uncertainty, \
                        a.model_configuration,a.response_payload_sha256,a.matrix_choice_set_digest, \
                        d.opportunity_id,d.provider,d.model,d.configuration_snapshot,d.configuration_digest, \
                        d.material_digest,d.state,d.send_certainty,d.outcome,d.response_payload,d.request_payload, \
                        c.revision AS current_config_revision,c.mode,c.provider_profile_ref AS current_profile, \
                        c.model_configuration AS current_model \
                 FROM advisory_matrix_advice a \
                 JOIN advisory_dispatch d ON (d.tenant_id,d.workspace_id,d.id)= \
                   (a.tenant_id,a.workspace_id,a.dispatch_id) \
                 JOIN advisory_workspace_config c ON (c.tenant_id,c.workspace_id)= \
                   (a.tenant_id,a.workspace_id) \
                 WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.opportunity_id=$3 \
                 FOR SHARE OF d,c",
            )
            .bind(tenant).bind(workspace_id).bind(request.opportunity_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::StaleContext)?;
            let response: Option<Vec<u8>> =
                advice.try_get("response_payload").map_err(storage_error)?;
            let snapshot: serde_json::Value = advice
                .try_get("configuration_snapshot")
                .map_err(storage_error)?;
            let config_sha = format!(
                "{:x}",
                Sha256::digest(serde_json::to_vec(&snapshot).map_err(storage_error)?)
            );
            let stored_outcome = match (
                advice
                    .try_get::<String, _>("kind")
                    .map_err(storage_error)?
                    .as_str(),
                advice
                    .try_get::<Option<serde_json::Value>, _>("ranked_choice_ids")
                    .map_err(storage_error)?,
                advice
                    .try_get::<Option<String>, _>("reason")
                    .map_err(storage_error)?,
            ) {
                ("ranked", Some(ranks), None) => GuardedMatrixAdviceOutcome::Ranked {
                    ranked_choice_ids: serde_json::from_value(ranks)
                        .map_err(|_| Error::StaleContext)?,
                },
                ("abstained", None, reason) => GuardedMatrixAdviceOutcome::Abstained { reason },
                _ => return Err(Error::StaleContext),
            };
            let policy: Option<String> = advice
                .try_get("ranking_policy_version")
                .map_err(storage_error)?;
            let trial_json: Option<serde_json::Value> =
                advice.try_get("trial_uncertainty").map_err(storage_error)?;
            let trial = crate::matrix_advice_store::decode_trial_uncertainty(
                policy.as_deref(),
                trial_json,
                &snapshot,
                &stored_outcome,
            )
            .map_err(|_| Error::StaleContext)?;
            // Reuse the complete saved-row decoder, canonical strict/trial
            // digest and raw-payload binding, not merely the supplied digest.
            let stored = self
                .guarded_matrix_advice(workspace_id, request.opportunity_id)
                .await?
                .ok_or(Error::StaleContext)?;
            let request_payload: Vec<u8> =
                advice.try_get("request_payload").map_err(storage_error)?;
            if trial != token.trial_evidence
                || trial != stored.record.trial_evidence
                || (trial.is_some()
                    && !crate::matrix_advice_store::disposition_trial_snapshot_matches(
                        &snapshot,
                        &request_payload,
                        &stored.record,
                    ))
                || stored.advice_id != token.advice_id
                || stored.record.advice_digest != token.advice_digest
            {
                return Err(Error::StaleContext);
            }
            if request.advice_id != Some(token.advice_id)
                || request.advice_digest.as_deref() != Some(token.advice_digest.as_str())
                || token.task_revision != revision
                || token.input_digest != request.expected_input_digest
                || Some(token.choice_set_digest.as_str())
                    != request.expected_choice_set_digest.as_deref()
                || token.choice_set_id
                    != current
                        .choice_set
                        .as_ref()
                        .ok_or(Error::StaleContext)?
                        .choice_set_id
                || token.choice_set_version
                    != current
                        .choice_set
                        .as_ref()
                        .ok_or(Error::StaleContext)?
                        .version
                || opportunity
                    .try_get::<i64, _>("config_revision")
                    .map_err(storage_error)?
                    != advice
                        .try_get::<i64, _>("current_config_revision")
                        .map_err(storage_error)?
                || advice.try_get::<String, _>("mode").map_err(storage_error)? != "optional"
                || advice
                    .try_get::<Uuid, _>("advice_id")
                    .map_err(storage_error)?
                    != token.advice_id
                || advice
                    .try_get::<Uuid, _>("dispatch_id")
                    .map_err(storage_error)?
                    != token.dispatch_id
                || advice
                    .try_get::<String, _>("advice_digest")
                    .map_err(storage_error)?
                    != token.advice_digest
                || advice
                    .try_get::<String, _>("matrix_choice_set_digest")
                    .map_err(storage_error)?
                    != token.choice_set_digest
                || stored_outcome != token.outcome
                || advice
                    .try_get::<Uuid, _>("opportunity_id")
                    .map_err(storage_error)?
                    != request.opportunity_id
                || advice
                    .try_get::<String, _>("provider")
                    .map_err(storage_error)?
                    != token.provider_profile_ref.id
                || advice
                    .try_get::<String, _>("model")
                    .map_err(storage_error)?
                    != token.model_configuration.model
                || advice
                    .try_get::<String, _>("provider_profile_ref")
                    .map_err(storage_error)?
                    != token.provider_profile_ref.id
                || advice
                    .try_get::<Option<String>, _>("current_profile")
                    .map_err(storage_error)?
                    .as_deref()
                    != Some(token.provider_profile_ref.id.as_str())
                || advice
                    .try_get::<serde_json::Value, _>("model_configuration")
                    .map_err(storage_error)?
                    != serde_json::json!(token.model_configuration)
                || advice
                    .try_get::<Option<serde_json::Value>, _>("current_model")
                    .map_err(storage_error)?
                    != Some(serde_json::json!(token.model_configuration))
                || advice
                    .try_get::<String, _>("configuration_digest")
                    .map_err(storage_error)?
                    != config_sha
                || advice
                    .try_get::<String, _>("material_digest")
                    .map_err(storage_error)?
                    != token.evaluation_digest
                || opportunity
                    .try_get::<String, _>("material_digest")
                    .map_err(storage_error)?
                    != token.evaluation_digest
                || opportunity
                    .try_get::<Option<String>, _>("matrix_verification_digest")
                    .map_err(storage_error)?
                    .as_deref()
                    != Some(token.verification_digest.as_str())
                || advice
                    .try_get::<String, _>("state")
                    .map_err(storage_error)?
                    != "sealed"
                || advice
                    .try_get::<String, _>("send_certainty")
                    .map_err(storage_error)?
                    != "sent"
                || advice
                    .try_get::<Option<String>, _>("outcome")
                    .map_err(storage_error)?
                    .as_deref()
                    != Some("provider_response")
                || advice
                    .try_get::<String, _>("response_payload_sha256")
                    .map_err(storage_error)?
                    != token.response_payload_sha256
                || response
                    .as_ref()
                    .map(|bytes| format!("{:x}", Sha256::digest(bytes)))
                    != Some(token.response_payload_sha256.clone())
                || snapshot.get("provider_profile_ref")
                    != Some(&serde_json::json!(token.provider_profile_ref))
                || snapshot.get("model_configuration")
                    != Some(&serde_json::json!(token.model_configuration))
            {
                return Err(Error::StaleContext);
            }
            let verification = current_verification.ok_or(Error::StaleContext)?;
            if verification.record_digest() != token.verification_digest {
                return Err(Error::StaleContext);
            }
            let source = self
                .matrix_task_source(workspace_id, request.task_id)
                .await?
                .ok_or(Error::StaleContext)?;
            if source.revision != *current
                || match verification {
                    MatrixDispositionVerification::LegacyV1 { .. } => {
                        source.requirements_binding.is_some()
                    }
                    MatrixDispositionVerification::ContextV2 { binding, .. } => {
                        source.requirements_binding.as_ref() != Some(binding)
                    }
                }
            {
                return Err(Error::StaleContext);
            }
            let (schema, snapshot, semantic, authority) = match verification {
                MatrixDispositionVerification::LegacyV1 { .. } => {
                    (MATRIX_VERIFICATION_SCHEMA, None, None, None)
                }
                MatrixDispositionVerification::ContextV2 { binding, .. } => (
                    CONTEXT_MATRIX_VERIFICATION_SCHEMA,
                    Some(binding.snapshot_id),
                    Some(binding.semantic_digest.as_str()),
                    Some(binding.authority_schema.as_str()),
                ),
            };
            let fresh: Option<bool> = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM matrix_verifications v \
                 WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.task_id=$3 \
                   AND v.task_revision=$4 AND v.input_digest=$5 AND v.record_digest=$6 \
                   AND v.schema=$7 AND v.frozen_snapshot_id IS NOT DISTINCT FROM $8 \
                   AND v.requirements_semantic_digest IS NOT DISTINCT FROM $9 \
                   AND v.authority_schema IS NOT DISTINCT FROM $10 \
                   AND EXISTS(SELECT 1 FROM matrix_verification_bindings b \
                     WHERE b.tenant_id=v.tenant_id AND b.workspace_id=v.workspace_id \
                       AND b.verification_id=v.id) \
                   AND NOT EXISTS(SELECT 1 FROM matrix_verification_bindings b \
                     WHERE b.tenant_id=v.tenant_id AND b.workspace_id=v.workspace_id \
                       AND b.verification_id=v.id \
                       AND b.expires_at<=FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint))",
            ).bind(tenant).bind(workspace_id).bind(request.task_id).bind(revision)
             .bind(&request.expected_input_digest).bind(&token.verification_digest)
             .bind(schema).bind(snapshot).bind(semantic).bind(authority)
             .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
            if fresh != Some(true) {
                return Err(Error::StaleContext);
            }
        } else if current_advice.is_some()
            || request.advice_id.is_some()
            || request.advice_digest.is_some()
        {
            return Err(Error::StaleContext);
        }

        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION, MATRIX_TRIAL_POLICY_ID,
        MatrixTrialCandidateEvidence, MatrixTrialRankingEvidence, NativeMatrixScoreDistribution,
        matrix_trial_policy_digest,
    };

    fn evidence() -> MatrixTrialRankingEvidence {
        let score = |id: &str, level: usize, confidence: f64| {
            let mut probabilities = [0.0; 10];
            probabilities[level] = 1.0;
            let distribution = NativeMatrixScoreDistribution::new(probabilities).unwrap();
            let bounds = distribution.feasible_expected_score();
            MatrixTrialCandidateEvidence {
                candidate_id: id.into(),
                declared_score: level as f64,
                score_confidence: confidence,
                probabilities,
                displayed_mean: distribution.displayed_mean(),
                feasible_minimum: bounds.minimum,
                feasible_maximum: bounds.maximum,
            }
        };
        MatrixTrialRankingEvidence {
            policy_id: MATRIX_TRIAL_POLICY_ID.into(),
            policy_version: MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION.into(),
            policy_digest: matrix_trial_policy_digest(),
            choice_selected_candidate_id: "a".into(),
            choice_confidence: 0.8,
            choice_selected_answer_probability: 0.8,
            scores: vec![score("a", 8, 0.9), score("b", 4, 0.5)],
            low_loser_confidence: true,
        }
    }

    #[test]
    fn trial_row_and_token_require_exact_typed_evidence_not_just_rank_or_digest() {
        let token = evidence();
        let ranked = GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: vec!["a".into(), "b".into()],
        };
        let snapshot =
            serde_json::json!({"ranking_policy":MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION});
        let policy = Some(MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION);
        let encoded = serde_json::to_value(&token).unwrap();
        let decoded = crate::matrix_advice_store::decode_trial_uncertainty(
            policy,
            Some(encoded.clone()),
            &snapshot,
            &ranked,
        )
        .unwrap();
        assert_eq!(decoded, Some(token.clone()));
        let mut other_token = token.clone();
        other_token.choice_confidence = 0.7;
        assert_ne!(decoded, Some(other_token));
        assert!(
            crate::matrix_advice_store::decode_trial_uncertainty(
                None,
                Some(encoded.clone()),
                &snapshot,
                &ranked,
            )
            .is_err()
        );
        assert!(
            crate::matrix_advice_store::decode_trial_uncertainty(policy, None, &snapshot, &ranked,)
                .is_err()
        );
        assert!(
            crate::matrix_advice_store::decode_trial_uncertainty(
                policy,
                Some(encoded),
                &serde_json::json!({}),
                &ranked,
            )
            .is_err()
        );
        assert!(
            crate::matrix_advice_store::decode_trial_uncertainty(None, None, &snapshot, &ranked,)
                .is_err()
        );
        assert_eq!(
            crate::matrix_advice_store::decode_trial_uncertainty(
                None,
                None,
                &serde_json::json!({}),
                &ranked,
            )
            .unwrap(),
            None
        );
    }
}
