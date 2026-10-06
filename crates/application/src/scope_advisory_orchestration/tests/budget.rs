use super::*;
use crate::SignedScopeBudgetPreflight;
use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};

/// Domain-syntactic pure fixture only: its all-zero signature is NOT an
/// authenticated owner approval and this helper never installs a policy.
pub(super) fn syntactic_policy(bytes: i64) -> AdvisoryBudgetPolicy {
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 1,
        output_tokens: 1,
        request_utf8_bytes: bytes,
        elapsed_monotonic_ms: 1,
        retry_dispatches: 1,
    };
    let id = Uuid::from_u128(500);
    AdvisoryBudgetPolicy::new(
        id,
        7,
        AdvisoryBudgetPolicy::digest_for(id, 7, 100, 200, ceilings),
        100,
        200,
        ceilings,
        Uuid::from_u128(501),
        "0".repeat(128),
    )
    .unwrap()
}

fn prepared(body: Vec<u8>) -> PreparedScopeAdviceAttempt {
    PreparedScopeAdviceAttempt::new(
        fixture_scope_request(),
        body,
        "fixture-profile".into(),
        "fixture-model".into(),
        "https://fixture.invalid/advice".into(),
        "fixture-wire/1".into(),
    )
    .unwrap()
}

fn budget_request(prepared: &PreparedScopeAdviceAttempt) -> ScopeBudgetRequest {
    ScopeBudgetRequest {
        workspace_id: Uuid::from_u128(1),
        actor_id: Uuid::from_u128(2),
        candidate_set_id: Uuid::from_u128(3),
        config_revision: 4,
        manifest_digest: "a".repeat(64),
        body_length: prepared.body_length(),
        body_sha256: prepared.body_sha256().to_owned(),
    }
}

fn configured() -> WorkspaceAdvisoryConfig {
    let mut config = no_call_config(WorkspaceAdvisoryMode::Optional);
    config.provider_profile_ref = Some(AdvisoryProviderProfileRef {
        id: "fixture-profile".into(),
    });
    config.model_configuration = Some(AdvisoryModelConfiguration {
        model: "fixture-model".into(),
    });
    config
}

#[tokio::test]
async fn signed_scope_preflight_accepts_positive_exact_utf8_byte_ceiling() {
    let prepared = prepared("é".as_bytes().to_vec());
    let request = budget_request(&prepared);
    assert_eq!(request.body_length, 2);
    assert_eq!(request.body_sha256, sha256(prepared.body()));
    let policy = syntactic_policy(2);
    assert_eq!(
        SignedScopeBudgetPreflight
            .evaluate(&request, &policy)
            .await
            .unwrap(),
        Some(crate::ScopeBudgetPolicyEvaluation {
            policy_id: policy.id().to_string()
        })
    );
}

#[tokio::test]
async fn signed_scope_preflight_denies_over_limit_without_transport() {
    let request = budget_request(&prepared("é".as_bytes().to_vec()));
    assert_eq!(
        SignedScopeBudgetPreflight
            .evaluate(&request, &syntactic_policy(1))
            .await
            .unwrap(),
        None
    );
}

#[tokio::test]
async fn signed_scope_preflight_denies_zero_body_and_explicit_deny_stays_deny() {
    let request = budget_request(&prepared(Vec::new()));
    let policy = syntactic_policy(1);
    assert_eq!(
        SignedScopeBudgetPreflight
            .evaluate(&request, &policy)
            .await
            .unwrap(),
        None
    );
    let positive = budget_request(&prepared(b"x".to_vec()));
    assert_eq!(
        DenyScopeBudget.evaluate(&positive, &policy).await.unwrap(),
        None
    );
}

#[tokio::test]
async fn absent_verified_budget_adapter_cannot_authorize() {
    assert!(
        crate::matrix_advisory_capture::lookup_verified_matrix_budget(None, Uuid::from_u128(1))
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn scope_authorization_rejects_policy_id_substitution() {
    let policy = syntactic_policy(1024);
    let budget = crate::ScopeBudgetPolicyEvaluation {
        policy_id: Uuid::from_u128(999).to_string(),
    };
    assert_eq!(
        scope_dispatch_authorization(
            &prepared(b"x".to_vec()),
            ScopeDispatchMetadata {
                dispatch_id: Uuid::from_u128(21),
                opportunity_id: Uuid::from_u128(22),
                provider: "fixture",
                adapter_version: "v1",
                material_digest: "a".repeat(64)
            },
            &configured(),
            &budget,
            &policy,
        ),
        Err(Error::BudgetPolicyInvalid)
    );
}

#[test]
fn scope_authorization_snapshots_exact_verified_identity_and_prepared_bytes() {
    let policy = syntactic_policy(1024);
    let budget = crate::ScopeBudgetPolicyEvaluation {
        policy_id: policy.id().to_string(),
    };
    let prepared = prepared("é".as_bytes().to_vec());
    let authorization = scope_dispatch_authorization(
        &prepared,
        ScopeDispatchMetadata {
            dispatch_id: Uuid::from_u128(21),
            opportunity_id: Uuid::from_u128(22),
            provider: "fixture",
            adapter_version: "v1",
            material_digest: "a".repeat(64),
        },
        &configured(),
        &budget,
        &policy,
    )
    .unwrap();
    assert_eq!(
        authorization.configuration_snapshot["budget_policy_id"],
        policy.id().to_string()
    );
    assert_eq!(
        authorization.configuration_snapshot["budget_policy"],
        serde_json::json!({"policy_id":policy.id().to_string(),
            "policy_version":policy.version(), "policy_digest":policy.digest()})
    );
    assert_eq!(authorization.request_payload, prepared.body());
    assert_eq!(authorization.payload_digest, prepared.body_sha256());
    assert_eq!(
        authorization.configuration_snapshot["request_body_length"],
        prepared.body_length()
    );
    assert_eq!(
        authorization.configuration_snapshot["request_body_sha256"],
        prepared.body_sha256()
    );
    assert_eq!(
        authorization.configuration_digest,
        sha256(&serde_json::to_vec(&authorization.configuration_snapshot).unwrap())
    );
    assert!(
        authorization
            .configuration_snapshot
            .get("matrix_authority")
            .is_none()
    );
}
