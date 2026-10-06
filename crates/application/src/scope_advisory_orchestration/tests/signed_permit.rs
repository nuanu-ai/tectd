use super::*;

fn signed_fixture() -> (
    PreparedScopeAdviceAttempt,
    AdvisoryDispatchAuthorization,
    AdvisoryDispatchStart,
) {
    let policy = super::budget::syntactic_policy(65_536);
    let prepared = PreparedScopeAdviceAttempt::new(
        fixture_scope_request(),
        b"{\"scope\":\"signed\"}".to_vec(),
        "fixture-profile".into(),
        "fixture-model".into(),
        "https://fixture.invalid/advice".into(),
        "fixture-wire/1".into(),
    )
    .unwrap();
    let mut config = no_call_config(WorkspaceAdvisoryMode::Optional);
    config.provider_profile_ref = Some(AdvisoryProviderProfileRef {
        id: "fixture-profile".into(),
    });
    config.model_configuration = Some(AdvisoryModelConfiguration {
        model: "fixture-model".into(),
    });
    let authorization = scope_dispatch_authorization(
        &prepared,
        ScopeDispatchMetadata {
            dispatch_id: Uuid::from_u128(21),
            opportunity_id: Uuid::from_u128(22),
            provider: "fixture",
            adapter_version: "v1",
            material_digest: "a".repeat(64),
        },
        &config,
        &crate::ScopeBudgetPolicyEvaluation {
            policy_id: policy.id().to_string(),
        },
        &policy,
    )
    .unwrap();
    let started = fixture_started_dispatch(&authorization, true);
    (prepared, authorization, started)
}

#[test]
fn signed_permit_requires_reservation() {
    let (prepared, authorization, mut started) = signed_fixture();
    started.budget_reservation = None;
    assert!(matches!(
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &prepared),
        Err(Error::BudgetPolicyInvalid)
    ));
}

#[test]
fn signed_permit_requires_exact_nested_and_flat_policy_identity() {
    let (prepared, authorization, started) = signed_fixture();
    for (key, value) in [
        (
            "policy_id",
            serde_json::json!(Uuid::from_u128(99).to_string()),
        ),
        ("policy_version", serde_json::json!(999)),
        ("policy_digest", serde_json::json!("f".repeat(64))),
        ("unexpected", serde_json::json!(true)),
    ] {
        let mut changed = authorization.clone();
        changed.configuration_snapshot["budget_policy"][key] = value;
        assert!(matches!(
            StartedScopeDispatchPermit::after_committed_start(&started, &changed, &prepared),
            Err(Error::BudgetPolicyInvalid)
        ));
    }
    let mut changed = authorization.clone();
    changed.configuration_snapshot["budget_policy_id"] =
        serde_json::json!(Uuid::from_u128(99).to_string());
    assert!(matches!(
        StartedScopeDispatchPermit::after_committed_start(&started, &changed, &prepared),
        Err(Error::BudgetPolicyInvalid)
    ));
}

#[test]
fn signed_permit_binds_reservation_dispatch_body_and_one_call() {
    let (prepared, authorization, started) = signed_fixture();
    let permit =
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &prepared)
            .unwrap();
    assert!(permit.permits(authorization.dispatch_id, &prepared));
    assert!(!permit.permits(Uuid::from_u128(99), &prepared));
    for field in ["dispatch", "request_sha", "body_length", "calls"] {
        let mut changed = started.clone();
        let reservation = changed.budget_reservation.as_mut().unwrap();
        match field {
            "dispatch" => reservation.dispatch_id = Uuid::from_u128(99),
            "request_sha" => reservation.request_sha256 = "f".repeat(64),
            "body_length" => reservation.request_utf8_bytes += 1,
            "calls" => reservation.reserved_calls = 2,
            _ => unreachable!(),
        }
        assert!(matches!(
            StartedScopeDispatchPermit::after_committed_start(&changed, &authorization, &prepared),
            Err(Error::InputConflict)
        ));
    }
    let different = PreparedScopeAdviceAttempt::new(
        fixture_scope_request(),
        b"{\"scope\":\"changed\"}".to_vec(),
        "fixture-profile".into(),
        "fixture-model".into(),
        "https://fixture.invalid/advice".into(),
        "fixture-wire/1".into(),
    )
    .unwrap();
    assert!(!permit.permits(authorization.dispatch_id, &different));
    assert!(matches!(
        StartedScopeDispatchPermit::after_committed_start(&started, &authorization, &different),
        Err(Error::InputConflict)
    ));
}
