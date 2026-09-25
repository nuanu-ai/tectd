use super::*;
use crate::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixProviderIdentity, MatrixProviderRequest, MatrixProviderResponse,
    MatrixStartedDispatchPermit, PreparedMatrixAdviceAttempt,
};
use async_trait::async_trait;
use tect_domain::{
    CommitmentEvidence, EngineeringIntent, EngineeringMatrixInput, EngineeringMode, FactProvenance,
    MatrixEvidenceBinding, MatrixFact, OperatingEnvelope, OperationalFacts, RequiredMatrixFact,
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
        policy: &tect_domain::AdvisoryBudgetPolicy,
    ) -> Result<Option<MatrixBudgetAuthorization>> {
        Ok(Some(MatrixBudgetAuthorization {
            policy_id: policy.id().to_string(),
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
        MatrixVerificationActor {
            workspace_id: workspace,
            verifier_principal_id: Uuid::new_v4(),
            verifier_session_id: Uuid::new_v4(),
        },
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

mod positive_binding;

#[tokio::test]
async fn complete_verification_is_saved_with_exact_owner_and_digest() {
    let revision = revision();
    let verifier = Uuid::new_v4();
    let mut store = FakeStore::default();
    let record = verify_locked_revision(
        &mut store,
        &FakeValidator { trusted: true },
        MatrixVerificationActor {
            workspace_id: Uuid::new_v4(),
            verifier_principal_id: verifier,
            verifier_session_id: Uuid::new_v4(),
        },
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
            MatrixVerificationActor {
                workspace_id: Uuid::new_v4(),
                verifier_principal_id: revision.recorded_by_principal_id,
                verifier_session_id: Uuid::new_v4(),
            },
            &revision,
            &request(&revision),
            &|| Ok(100),
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
            MatrixVerificationActor {
                workspace_id: Uuid::new_v4(),
                verifier_principal_id: Uuid::new_v4(),
                verifier_session_id: Uuid::new_v4(),
            },
            &revision,
            &request,
            &|| Ok(100),
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
            MatrixVerificationActor {
                workspace_id: Uuid::new_v4(),
                verifier_principal_id: Uuid::new_v4(),
                verifier_session_id: Uuid::new_v4(),
            },
            &revision,
            &request,
            &|| Ok(100),
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
            MatrixVerificationActor {
                workspace_id: Uuid::new_v4(),
                verifier_principal_id: Uuid::new_v4(),
                verifier_session_id: Uuid::new_v4(),
            },
            &revision,
            &request,
            &|| Ok(100),
        )
        .await,
        Err(Error::InvalidArguments)
    );
    request = self::request(&revision);
    assert_eq!(
        verify_locked_revision(
            &mut store,
            &FakeValidator { trusted: false },
            MatrixVerificationActor {
                workspace_id: Uuid::new_v4(),
                verifier_principal_id: Uuid::new_v4(),
                verifier_session_id: Uuid::new_v4(),
            },
            &revision,
            &request,
            &|| Ok(100),
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
            MatrixVerificationActor {
                workspace_id: Uuid::new_v4(),
                verifier_principal_id: Uuid::new_v4(),
                verifier_session_id: Uuid::new_v4(),
            },
            &revision,
            &request(&revision),
            &clock,
        )
        .await,
        Err(Error::InvalidArguments)
    );
    assert!(store.saved.is_empty());
}
