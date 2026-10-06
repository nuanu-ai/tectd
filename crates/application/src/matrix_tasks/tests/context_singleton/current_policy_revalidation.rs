use super::*;

// Fixture proof of the reused external-validation seam, not live authority proof.
struct CurrentArtifactValidator {
    policy: &'static str,
    evidence_ref: &'static str,
    revalidations: std::sync::atomic::AtomicUsize,
}
#[async_trait::async_trait]
impl crate::MatrixEvidenceValidator for CurrentArtifactValidator {
    fn policy_version(&self) -> &str {
        self.policy
    }
    async fn validate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        _: &RequiredMatrixFact,
        _: &str,
        _: i64,
    ) -> Result<MatrixEvidenceBinding> {
        panic!("revalidation only")
    }
    async fn revalidate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        binding: &MatrixEvidenceBinding,
        now: i64,
    ) -> Result<()> {
        self.revalidations
            .fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        if binding.evidence_ref == self.evidence_ref
            && now < binding.expires_at
            && fact.path == binding.fact_path
            && fact.value_digest == binding.value_digest
        {
            Ok(())
        } else {
            Err(Error::Forbidden)
        }
    }
}

#[tokio::test]
async fn context_singleton_revalidates_current_policy_record_reference_and_freshness() {
    let (revision, context, record, snapshot) = fixture(1);
    for (policy, evidence_ref, now, expected, external_check) in [
        ("test-policy/1", "immutable-test-ref", 20, true, true),
        ("test-policy/2", "immutable-test-ref", 20, false, false),
        ("test-policy/1", "revoked-or-replaced-ref", 20, false, true),
        ("test-policy/1", "immutable-test-ref", 100, false, false),
    ] {
        let validator = CurrentArtifactValidator {
            policy,
            evidence_ref,
            revalidations: std::sync::atomic::AtomicUsize::new(0),
        };
        let composed = binding::compose_bound_revision_with_verification(
            Some(&mut Store(Some(record.clone()))),
            &validator,
            Uuid::new_v4(),
            &revision,
            snapshot,
            &context,
            now,
        )
        .await
        .unwrap();
        assert_eq!(composed.is_some(), expected);
        assert_eq!(
            validator
                .revalidations
                .load(std::sync::atomic::Ordering::SeqCst)
                > 0,
            external_check
        );
    }
    assert!(
        binding::compose_bound_revision_with_verification(
            Some(&mut Store(Some(record))),
            &crate::DisabledMatrixEvidenceValidator,
            Uuid::new_v4(),
            &revision,
            snapshot,
            &context,
            20,
        )
        .await
        .unwrap()
        .is_none()
    );
}
