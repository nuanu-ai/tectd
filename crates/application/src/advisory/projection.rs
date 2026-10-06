use std::future::Future;
use tect_domain::{
    CandidateScopeAdvisoryProjection, Error, GuardedScopeAdvice, PrincipalRole, Result,
    ScopeConstructorManifest,
};
use uuid::Uuid;

/// The projection read is lazy: Verifier cannot load Owner planning material.
pub(super) async fn owner_projection(
    role: PrincipalRole,
    read: impl Future<Output = Result<Option<CandidateScopeAdvisoryProjection>>>,
) -> Result<Option<CandidateScopeAdvisoryProjection>> {
    if role != PrincipalRole::Owner {
        return Ok(None);
    }
    read.await
}

pub(super) fn bind_projection(
    candidate_set_id: Uuid,
    opportunity_id: Uuid,
    manifest: ScopeConstructorManifest,
    advice: GuardedScopeAdvice,
) -> Result<CandidateScopeAdvisoryProjection> {
    if manifest.source.candidate_set_id != candidate_set_id
        || advice.opportunity_id.is_some_and(|id| id != opportunity_id)
        || advice.source_digest != manifest.source.digest
        || advice.manifest_digest != manifest.whole_set_digest
        || advice.eligible_set_digest != manifest.eligible_set_digest
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(CandidateScopeAdvisoryProjection {
        version: 1,
        manifest,
        advice,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    // Typed persisted-shape fixture for projection binding checks, not a manifest validation proof.
    fn material() -> (ScopeConstructorManifest, GuardedScopeAdvice) {
        let manifest = serde_json::from_value(serde_json::json!({
            "constructor": {"id":"fixture", "version":"1", "digest":"constructor"},
            "source": {
                "candidate_set_id":Uuid::from_u128(1), "candidate_set_revision":1,
                "snapshot_id":Uuid::from_u128(3), "input_cursor":0,
                "program_id":Uuid::from_u128(4), "program_revision":1,
                "program_latest_input":0, "planning_latest_input":0,
                "selected_sources_digest":"sources", "method_revision":"1",
                "method_digest":"method", "registry_revision":"1",
                "registry_digest":"registry", "inputs":[], "digest":"source"
            },
            "obligations":[], "emitted":[], "rejected":[],
            "baseline_id":"baseline", "ordered_ids":[],
            "eligible_set_digest":"eligible", "whole_set_digest":"manifest"
        }))
        .unwrap();
        let advice = serde_json::from_value(serde_json::json!({
            "id":"advice", "opportunity_id":Uuid::from_u128(2),
            "request_digest":"request", "source_digest":"source",
            "manifest_digest":"manifest", "eligible_set_digest":"eligible",
            "normalized_answers_digest":"answers", "items":[], "ranked_ids":[]
        }))
        .unwrap();
        (manifest, advice)
    }

    #[test]
    fn persisted_projection_requires_exact_target_and_digest_bindings() {
        let (manifest, advice) = material();
        let candidate_set = Uuid::from_u128(1);
        let opportunity = Uuid::from_u128(2);
        let projection =
            bind_projection(candidate_set, opportunity, manifest.clone(), advice.clone()).unwrap();
        assert_eq!(projection.version, 1);
        assert_eq!(projection.manifest, manifest);
        assert_eq!(projection.advice, advice);
        for mismatch in 0..5 {
            let (mut manifest, mut advice) = material();
            match mismatch {
                0 => manifest.source.candidate_set_id = Uuid::from_u128(9),
                1 => advice.opportunity_id = Some(Uuid::from_u128(9)),
                2 => advice.source_digest = "different".into(),
                3 => advice.manifest_digest = "different".into(),
                _ => advice.eligible_set_digest = "different".into(),
            }
            assert_eq!(
                bind_projection(candidate_set, opportunity, manifest, advice),
                Err(Error::StorageUnavailable)
            );
        }
    }

    #[tokio::test]
    async fn verifier_omits_projection_without_polling_material_read() {
        assert_eq!(
            owner_projection(PrincipalRole::Verifier, async {
                panic!("Verifier must not read Owner material");
            })
            .await,
            Ok(None)
        );
    }

    #[tokio::test]
    async fn owner_reads_projection_and_propagates_storage_failure() {
        assert_eq!(
            owner_projection(PrincipalRole::Owner, async {
                Err(Error::StorageUnavailable)
            })
            .await,
            Err(Error::StorageUnavailable)
        );
        assert_eq!(
            owner_projection(PrincipalRole::Owner, async { Ok(None) }).await,
            Ok(None)
        );
    }
}
