use crate::{
    MatrixEvidenceValidator, MatrixTaskRevision, MatrixVerificationStore, TransactionMode,
    WorkspaceService,
};
use std::{
    collections::BTreeMap,
    time::{SystemTime, UNIX_EPOCH},
};
use tect_domain::{
    Error, EvidenceValidationOutcome, MATRIX_VERIFICATION_SCHEMA, MatrixVerificationRecord,
    PrincipalRole, RequestContext, Result, evaluate_matrix_verification, matrix_input_digest,
    required_matrix_facts,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct MatrixEvidenceReference {
    pub fact_path: String,
    pub evidence_ref: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyMatrixTask {
    pub task_id: Uuid,
    pub expected_revision: i64,
    /// Digest returned with the saved MatrixTaskRevision.
    pub input_digest: String,
    pub evidence: Vec<MatrixEvidenceReference>,
}

impl WorkspaceService {
    /// Authorize a malformed verifier call against the same active, bound
    /// session required by a valid verification, without opening task data.
    pub async fn authenticate_matrix_verifier_session(
        &self,
        context: &RequestContext,
    ) -> Result<()> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        tx.commit().await
    }

    /// Verifies a saved owner revision. No JEV or advisory call is made.
    pub async fn verify_matrix_task(
        &self,
        context: &RequestContext,
        request: &VerifyMatrixTask,
    ) -> Result<MatrixVerificationRecord> {
        if request.task_id.is_nil()
            || request.expected_revision < 1
            || request.input_digest.len() != 64
            || !request.input_digest.bytes().all(|b| b.is_ascii_hexdigit())
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        // The store must be configured before external evidence is consulted.
        let store = tx
            .matrix_verification_store()
            .ok_or(Error::StorageUnavailable)?;
        let record = verify_locked_revision(
            store,
            self.matrix_evidence_validator.as_ref(),
            workspace.id,
            identity.principal_id,
            session.id,
            &revision,
            request,
            &current_epoch_seconds,
        )
        .await?;
        tx.commit().await?;
        Ok(record)
    }
}

pub(crate) async fn verify_locked_revision(
    store: &mut dyn MatrixVerificationStore,
    validator: &dyn MatrixEvidenceValidator,
    workspace_id: Uuid,
    verifier_principal_id: Uuid,
    verifier_session_id: Uuid,
    revision: &MatrixTaskRevision,
    request: &VerifyMatrixTask,
    clock: &(dyn Fn() -> Result<i64> + Sync),
) -> Result<MatrixVerificationRecord> {
    if revision.task_id != request.task_id {
        return Err(Error::NotFound);
    }
    if revision.revision != request.expected_revision {
        return Err(Error::StaleRevision);
    }
    if revision.input_digest != request.input_digest {
        return Err(Error::InputConflict);
    }
    if verifier_principal_id == revision.recorded_by_principal_id {
        return Err(Error::Forbidden);
    }
    let required = required_matrix_facts(&revision.input)?;
    if request.evidence.len() != required.len() {
        return Err(Error::InvalidArguments);
    }
    let mut refs = BTreeMap::new();
    for evidence in &request.evidence {
        if evidence.evidence_ref.trim().is_empty()
            || evidence.evidence_ref.len() > 4096
            || refs
                .insert(evidence.fact_path.as_str(), evidence.evidence_ref.as_str())
                .is_some()
        {
            return Err(Error::InvalidArguments);
        }
    }
    let mut bindings = Vec::with_capacity(required.len());
    let validation_now = clock()?;
    for fact in &required {
        let evidence_ref = refs
            .get(fact.path.as_str())
            .ok_or(Error::InvalidArguments)?;
        let binding = validator
            .validate(
                workspace_id,
                revision.task_id,
                revision.revision,
                fact,
                evidence_ref,
                validation_now,
            )
            .await?;
        if binding.fact_path != fact.path
            || binding.value_digest != fact.value_digest
            || binding.evidence_ref != *evidence_ref
            || binding.validation_outcome != EvidenceValidationOutcome::Accepted
        {
            return Err(Error::InvalidArguments);
        }
        bindings.push(binding);
    }
    let mut record = MatrixVerificationRecord {
        schema: MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: revision.task_id.to_string(),
        task_revision: revision.revision.to_string(),
        input_digest: matrix_input_digest(&revision.input)?,
        owner_principal: revision.recorded_by_principal_id.to_string(),
        verifier_principal: verifier_principal_id.to_string(),
        policy_version: validator.policy_version().into(),
        bindings,
        digest: String::new(),
    };
    if record.input_digest != revision.input_digest {
        return Err(Error::InternalInvariant);
    }
    record.digest = record.canonical_digest()?;
    evaluate_matrix_verification(
        &record.task_id,
        &record.task_revision,
        &revision.input,
        &record,
        clock()?,
    )?;
    store
        .append_matrix_verification(
            workspace_id,
            verifier_session_id,
            revision.task_id,
            revision.revision,
            &revision.input_digest,
            &record,
        )
        .await?;
    Ok(record)
}

pub(crate) fn current_epoch_seconds() -> Result<i64> {
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InternalInvariant)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::InternalInvariant)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::{
        MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
        MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
        MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt,
    };
    use async_trait::async_trait;
    use tect_domain::{
        CommitmentEvidence, EngineeringIntent, EngineeringMatrixInput, EngineeringMode,
        FactProvenance, MatrixEvidenceBinding, MatrixFact, OperatingEnvelope, OperationalFacts,
        RequiredMatrixFact,
    };

    fn known<T>(value: T) -> MatrixFact<T> {
        MatrixFact::Known {
            value,
            provenance: FactProvenance("owner-source".into()),
        }
    }

    fn revision() -> MatrixTaskRevision {
        let input = EngineeringMatrixInput {
            mode: known(EngineeringMode::Mvp),
            envelope: OperatingEnvelope {
                scale: known("12 workers".into()),
                operational_facts: OperationalFacts::KnownEmpty {
                    provenance: FactProvenance("owner-source".into()),
                },
            },
            criticality: known("low".into()),
            intent: known(EngineeringIntent::Other("booking".into())),
            urgency: known("normal".into()),
            promised_behavior: known("books".into()),
            promised_proof: known("acceptance".into()),
            affected_guarantees: MatrixFact::KnownEmpty {
                provenance: FactProvenance("owner-source".into()),
            },
            actual_exposure: known(false),
            demand_commitment: known(CommitmentEvidence::NoCommitment),
            latency_commitment: known(CommitmentEvidence::NoCommitment),
            urgent_repair: known(false),
        };
        let input_digest =
            crate::canonical_matrix_input_digest(&serde_json::to_value(&input).unwrap()).unwrap();
        MatrixTaskRevision {
            task_id: Uuid::new_v4(),
            revision: 7,
            request_id: Uuid::new_v4(),
            input,
            input_digest,
            choice_set: None,
            choice_set_digest: None,
            recorded_by_principal_id: Uuid::new_v4(),
            recorded_by_session_id: Uuid::new_v4(),
        }
    }

    fn request(revision: &MatrixTaskRevision) -> VerifyMatrixTask {
        VerifyMatrixTask {
            task_id: revision.task_id,
            expected_revision: revision.revision,
            input_digest: revision.input_digest.clone(),
            evidence: required_matrix_facts(&revision.input)
                .unwrap()
                .into_iter()
                .map(|fact| MatrixEvidenceReference {
                    fact_path: fact.path,
                    evidence_ref: "immutable:artifact@v1".into(),
                })
                .collect(),
        }
    }

    #[derive(Default)]
    struct FakeStore {
        saved: Vec<MatrixVerificationRecord>,
    }

    #[async_trait]
    impl MatrixVerificationStore for FakeStore {
        async fn matrix_verification_for_revision(
            &mut self,
            _workspace_id: Uuid,
            task_id: Uuid,
            revision: i64,
            input_digest: &str,
        ) -> Result<Option<MatrixVerificationRecord>> {
            Ok(self
                .saved
                .iter()
                .rev()
                .find(|record| {
                    record.task_id == task_id.to_string()
                        && record.task_revision == revision.to_string()
                        && record.input_digest == input_digest
                })
                .cloned())
        }

        async fn append_matrix_verification(
            &mut self,
            _workspace_id: Uuid,
            _verifier_session_id: Uuid,
            _task_id: Uuid,
            _expected_revision: i64,
            _expected_input_digest: &str,
            record: &MatrixVerificationRecord,
        ) -> Result<()> {
            self.saved.push(record.clone());
            Ok(())
        }
    }

    struct FakeValidator {
        trusted: bool,
    }

    struct FakeAdviceProvider(MatrixProviderIdentity);

    #[async_trait]
    impl MatrixAdviceProvider for FakeAdviceProvider {
        fn identity(&self) -> Option<MatrixProviderIdentity> {
            Some(self.0.clone())
        }

        fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
            PreparedMatrixAdviceAttempt::new(request, self.0.clone(), b"verified-body".to_vec())
        }

        async fn attempt_prepared(
            &self,
            _: PreparedMatrixAdviceAttempt,
            _: MatrixStartedDispatchPermit,
        ) -> Result<MatrixProviderResponse> {
            panic!("capture must not send")
        }
    }

    struct FakeBudget;

    #[async_trait]
    impl MatrixBudgetPolicy for FakeBudget {
        async fn authorize(
            &self,
            _: &MatrixBudgetRequest,
        ) -> Result<Option<MatrixBudgetAuthorization>> {
            Ok(Some(MatrixBudgetAuthorization {
                policy_id: "test-budget".into(),
            }))
        }
    }

    #[async_trait]
    impl MatrixEvidenceValidator for FakeValidator {
        fn policy_version(&self) -> &str {
            "test-trust-and-age/1"
        }
        async fn validate(
            &self,
            _workspace_id: Uuid,
            _task_id: Uuid,
            _revision: i64,
            fact: &RequiredMatrixFact,
            evidence_ref: &str,
            now: i64,
        ) -> Result<MatrixEvidenceBinding> {
            Ok(MatrixEvidenceBinding {
                fact_path: fact.path.clone(),
                value_digest: fact.value_digest.clone(),
                evidence_ref: evidence_ref.into(),
                content_digest: "a".repeat(64),
                source: "trusted-source".into(),
                subject: "task".into(),
                observed_at: now - 10,
                expires_at: now + 10,
                validation_outcome: if self.trusted {
                    EvidenceValidationOutcome::Accepted
                } else {
                    EvidenceValidationOutcome::Rejected
                },
            })
        }

        async fn revalidate(
            &self,
            _workspace_id: Uuid,
            _task_id: Uuid,
            _revision: i64,
            fact: &RequiredMatrixFact,
            binding: &MatrixEvidenceBinding,
            now: i64,
        ) -> Result<()> {
            if !self.trusted
                || binding.fact_path != fact.path
                || binding.value_digest != fact.value_digest
                || binding.expires_at <= now
            {
                return Err(Error::Forbidden);
            }
            Ok(())
        }
    }

    async fn composed(
        store: &mut FakeStore,
        validator: &FakeValidator,
        workspace: Uuid,
        revision: MatrixTaskRevision,
        now: i64,
    ) -> (tect_domain::EngineeringMatrixComposition, Option<String>) {
        crate::matrix_tasks::compose_current_revision_with_verification(
            Some(store),
            validator,
            workspace,
            revision.clone(),
            revision.revision,
            now,
        )
        .await
        .unwrap()
    }

    #[tokio::test]
    async fn latest_current_verification_controls_owner_composition() {
        let revision = revision();
        let workspace = Uuid::new_v4();
        let mut store = FakeStore::default();
        let record = verify_locked_revision(
            &mut store,
            &FakeValidator { trusted: true },
            workspace,
            Uuid::new_v4(),
            Uuid::new_v4(),
            &revision,
            &request(&revision),
            &|| Ok(100),
        )
        .await
        .unwrap();
        let (verified, digest) = composed(
            &mut store,
            &FakeValidator { trusted: true },
            workspace,
            revision.clone(),
            100,
        )
        .await;
        assert!(verified.is_resolved());
        assert_eq!(
            verified.source_verification_status,
            tect_domain::MatrixSourceVerificationStatus::IndependentlyVerifiedOwnerReported
        );
        assert_eq!(digest.as_deref(), Some(record.digest.as_str()));
        let (revoked, digest) = composed(
            &mut store,
            &FakeValidator { trusted: false },
            workspace,
            revision.clone(),
            100,
        )
        .await;
        assert!(!revoked.is_resolved());
        assert_eq!(digest, None);
        let (expired, digest) = composed(
            &mut store,
            &FakeValidator { trusted: true },
            workspace,
            revision.clone(),
            111,
        )
        .await;
        assert!(!expired.is_resolved());
        assert_eq!(digest, None);
        let mut advanced = revision.clone();
        advanced.revision += 1;
        let (stale, digest) = composed(
            &mut store,
            &FakeValidator { trusted: true },
            workspace,
            advanced,
            100,
        )
        .await;
        assert!(!stale.is_resolved());
        assert_eq!(digest, None);
        let mut wrong_policy = record.clone();
        wrong_policy.policy_version = "obsolete-policy".into();
        wrong_policy.digest = wrong_policy.canonical_digest().unwrap();
        store.saved.push(wrong_policy);
        let (policy_changed, digest) = composed(
            &mut store,
            &FakeValidator { trusted: true },
            workspace,
            revision.clone(),
            100,
        )
        .await;
        assert!(!policy_changed.is_resolved());
        assert_eq!(digest, None);
        let (no_store, digest) = crate::matrix_tasks::compose_current_revision_with_verification(
            None,
            &FakeValidator { trusted: true },
            workspace,
            revision.clone(),
            revision.revision,
            100,
        )
        .await
        .unwrap();
        assert!(!no_store.is_resolved());
        assert_eq!(digest, None);
    }

    #[tokio::test]
    async fn positive_binding_requires_revalidated_exact_record() {
        use tect_domain::{
            AdvisoryModelConfiguration, AdvisoryProviderProfileRef, EngineeringCandidate,
            EngineeringChoiceSet, MATRIX_CHOICE_SET_SCHEMA,
        };
        let mut revision = revision();
        let choice_set = EngineeringChoiceSet {
            schema: MATRIX_CHOICE_SET_SCHEMA.into(),
            choice_set_id: "set-1".into(),
            version: 1,
            task_id: revision.task_id.to_string(),
            task_revision: revision.revision.to_string(),
            decision_question: "Which approach?".into(),
            candidates: ["a", "b"]
                .into_iter()
                .map(|id| EngineeringCandidate {
                    candidate_id: id.into(),
                    title: id.into(),
                    approach: id.into(),
                    assumption_fact_ids: vec![],
                })
                .collect(),
        };
        revision.choice_set_digest = Some(choice_set.canonical_digest(&revision.input).unwrap());
        revision.choice_set = Some(choice_set);
        let workspace = Uuid::new_v4();
        let mut store = FakeStore::default();
        let record = verify_locked_revision(
            &mut store,
            &FakeValidator { trusted: true },
            workspace,
            Uuid::new_v4(),
            Uuid::new_v4(),
            &revision,
            &request(&revision),
            &|| Ok(100),
        )
        .await
        .unwrap();
        let (composition, verification) =
            crate::matrix_tasks::compose_current_revision_with_validated_verification(
                Some(&mut store),
                &FakeValidator { trusted: true },
                workspace,
                revision.clone(),
                revision.revision,
                100,
            )
            .await
            .unwrap();
        let verification = verification.unwrap();
        let provider = crate::MatrixProviderRequest::new_verified(
            revision.clone(),
            composition.clone(),
            &verification,
            AdvisoryProviderProfileRef {
                id: "provider".into(),
            },
            AdvisoryModelConfiguration {
                model: "model".into(),
            },
        )
        .unwrap();
        assert_eq!(
            provider.binding().verification_digest.as_deref(),
            Some(record.digest.as_str())
        );
        assert_ne!(
            provider.binding().evaluation_digest,
            tect_domain::matrix_evaluation_digest(
                &revision.input,
                &composition,
                revision.choice_set.as_ref().unwrap()
            )
            .unwrap()
            .unwrap()
        );
        let config = tect_domain::WorkspaceAdvisoryConfig {
            workspace_id: workspace,
            revision: 1,
            mode: tect_domain::WorkspaceAdvisoryMode::Optional,
            materialized: true,
            provider_profile_ref: Some(provider.provider_profile_ref().clone()),
            model_configuration: Some(provider.model_configuration().clone()),
        };
        let advice_request = crate::RequestEngineeringAdvisory {
            task_id: revision.task_id,
            expected_task_revision: revision.revision,
            request_key: "verified-capture".into(),
            session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
            request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        };
        let mut opportunity = crate::matrix_tasks::matrix_advisory_opportunity_input(
            &revision,
            &advice_request,
            &config,
            Uuid::new_v4(),
            Uuid::new_v4(),
        )
        .unwrap();
        opportunity.primary_reason = tect_domain::AdvisoryReason::CapabilityUnavailable;
        let fake_provider = FakeAdviceProvider(MatrixProviderIdentity {
            provider_profile_ref: provider.provider_profile_ref().clone(),
            model_configuration: provider.model_configuration().clone(),
            destination: "fake".into(),
            wire_version: "fake/1".into(),
        });
        let saved = crate::StoredMatrixDispatch {
            dispatch: tect_domain::AdvisoryDispatch {
                id: Uuid::new_v4(),
                opportunity_id: Uuid::new_v4(),
                predecessor_dispatch_id: None,
                attempt_number: 1,
                provider: "provider".into(),
                model: "model".into(),
                configuration_digest: "a".repeat(64),
                material_digest: provider.binding().evaluation_digest.clone(),
                payload_digest: "b".repeat(64),
                input_tokens: None,
                output_tokens: None,
                latency_ms: None,
                state: tect_domain::AdvisoryDispatchState::Sealed,
                send_certainty: tect_domain::AdvisorySendCertainty::Sent,
                outcome: Some(tect_domain::AdvisoryDispatchOutcome::ProviderResponse),
                retry_basis: tect_domain::AdvisoryRetryBasis::Initial,
                raw_response_ref: None,
            },
            binding: provider.binding().clone(),
            provider_profile_ref: provider.provider_profile_ref().clone(),
            model_configuration: provider.model_configuration().clone(),
            configuration_snapshot: serde_json::json!({}),
            destination: "fake".into(),
            wire_version: "fake/1".into(),
            request_payload: b"verified-body".to_vec(),
            request_payload_sha256: "b".repeat(64),
            response_payload: Some(b"opaque".to_vec()),
            response_payload_sha256: Some("c".repeat(64)),
        };
        assert_eq!(
            fake_provider.parse_sealed_response(&provider, &saved),
            Err(Error::TransportUnavailable)
        );
        let prepared = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut opportunity,
            Some(&provider),
            workspace,
            Uuid::new_v4(),
            &fake_provider,
            &FakeBudget,
        )
        .await
        .unwrap();
        assert!(matches!(
            prepared,
            crate::matrix_advisory_capture::PreparedMatrixOpportunity::Authorized { .. }
        ));
        assert_eq!(
            opportunity.matrix_verification_digest.as_deref(),
            Some(record.digest.as_str())
        );
        assert_eq!(
            opportunity.material_digest,
            provider.binding().evaluation_digest
        );
        let (revoked, token) =
            crate::matrix_tasks::compose_current_revision_with_validated_verification(
                Some(&mut store),
                &FakeValidator { trusted: false },
                workspace,
                revision.clone(),
                revision.revision,
                100,
            )
            .await
            .unwrap();
        assert!(token.is_none());
        assert!(
            crate::MatrixProviderRequest::new_verified(
                revision.clone(),
                revoked,
                &verification,
                AdvisoryProviderProfileRef {
                    id: "provider".into()
                },
                AdvisoryModelConfiguration {
                    model: "model".into()
                },
            )
            .is_err()
        );
        let mut advanced = revision;
        advanced.revision += 1;
        assert!(
            crate::MatrixProviderRequest::new_verified(
                advanced,
                composition,
                &verification,
                AdvisoryProviderProfileRef {
                    id: "provider".into()
                },
                AdvisoryModelConfiguration {
                    model: "model".into()
                },
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn complete_verification_is_saved_with_exact_owner_and_digest() {
        let revision = revision();
        let verifier = Uuid::new_v4();
        let mut store = FakeStore::default();
        let record = verify_locked_revision(
            &mut store,
            &FakeValidator { trusted: true },
            Uuid::new_v4(),
            verifier,
            Uuid::new_v4(),
            &revision,
            &request(&revision),
            &|| Ok(100),
        )
        .await
        .unwrap();
        assert_eq!(store.saved, vec![record.clone()]);
        assert_eq!(
            record.owner_principal,
            revision.recorded_by_principal_id.to_string()
        );
        assert_eq!(record.verifier_principal, verifier.to_string());
        assert_eq!(record.input_digest, revision.input_digest);
    }

    #[tokio::test]
    async fn same_owner_different_session_is_denied() {
        let revision = revision();
        let mut store = FakeStore::default();
        assert_eq!(
            verify_locked_revision(
                &mut store,
                &FakeValidator { trusted: true },
                Uuid::new_v4(),
                revision.recorded_by_principal_id,
                Uuid::new_v4(),
                &revision,
                &request(&revision),
                &|| Ok(100)
            )
            .await,
            Err(Error::Forbidden)
        );
        assert!(store.saved.is_empty());
    }

    #[tokio::test]
    async fn stale_revision_or_digest_never_persists() {
        let revision = revision();
        let mut store = FakeStore::default();
        let mut request = request(&revision);
        request.expected_revision -= 1;
        assert_eq!(
            verify_locked_revision(
                &mut store,
                &FakeValidator { trusted: true },
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                &revision,
                &request,
                &|| Ok(100)
            )
            .await,
            Err(Error::StaleRevision)
        );
        request.expected_revision = revision.revision;
        request.input_digest = "b".repeat(64);
        assert_eq!(
            verify_locked_revision(
                &mut store,
                &FakeValidator { trusted: true },
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                &revision,
                &request,
                &|| Ok(100)
            )
            .await,
            Err(Error::InputConflict)
        );
        assert!(store.saved.is_empty());
    }

    #[tokio::test]
    async fn missing_or_untrusted_evidence_never_persists() {
        let revision = revision();
        let mut store = FakeStore::default();
        let mut request = request(&revision);
        request.evidence.pop();
        assert_eq!(
            verify_locked_revision(
                &mut store,
                &FakeValidator { trusted: true },
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                &revision,
                &request,
                &|| Ok(100)
            )
            .await,
            Err(Error::InvalidArguments)
        );
        request = self::request(&revision);
        assert_eq!(
            verify_locked_revision(
                &mut store,
                &FakeValidator { trusted: false },
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                &revision,
                &request,
                &|| Ok(100)
            )
            .await,
            Err(Error::InvalidArguments)
        );
        assert!(store.saved.is_empty());
    }

    #[tokio::test]
    async fn evidence_expiring_during_validation_never_persists() {
        let revision = revision();
        let mut store = FakeStore::default();
        let calls = std::sync::atomic::AtomicI32::new(0);
        let clock = || {
            let count = calls.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
            Ok(if count == 0 { 100 } else { 111 })
        };
        assert_eq!(
            verify_locked_revision(
                &mut store,
                &FakeValidator { trusted: true },
                Uuid::new_v4(),
                Uuid::new_v4(),
                Uuid::new_v4(),
                &revision,
                &request(&revision),
                &clock,
            )
            .await,
            Err(Error::InvalidArguments)
        );
        assert!(store.saved.is_empty());
    }
}
