use super::*;
fn policy() -> AdvisoryBudgetPolicy {
    let id = Uuid::new_v4();
    let c = AdvisoryBudgetCeilings {
        provider_calls: 2,
        input_tokens: 1,
        output_tokens: 1,
        request_utf8_bytes: 10,
        elapsed_monotonic_ms: 100,
        retry_dispatches: 1,
    };
    AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, 0, 200, c),
        0,
        200,
        c,
        Uuid::new_v4(),
        "a".repeat(128),
    )
    .unwrap()
}
#[test]
fn signed_snapshot_binds_exact_evaluated_policy_identity_for_scope_and_matrix() {
    let p = policy();
    let snapshot = serde_json::json!({
        "budget_policy_id": p.id().to_string(),
        "budget_policy": {
            "policy_id": p.id().to_string(),
            "policy_version": p.version(),
            "policy_digest": p.digest(),
        },
    });
    let digest = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&snapshot).unwrap())
    );
    assert_eq!(
        require_signed_policy_identity(&snapshot, &digest, Some(&p)),
        Ok(())
    );
    assert_eq!(
        require_signed_policy_identity(&snapshot, &digest, None),
        Err(Error::BudgetPolicyInvalid)
    );

    let replacement = AdvisoryBudgetPolicy::new(
        p.id(),
        2,
        AdvisoryBudgetPolicy::digest_for(p.id(), 2, 0, 200, p.ceilings()),
        0,
        200,
        p.ceilings(),
        Uuid::new_v4(),
        "a".repeat(128),
    )
    .unwrap();
    assert_eq!(
        require_signed_policy_identity(&snapshot, &digest, Some(&replacement)),
        Err(Error::BudgetPolicyInvalid)
    );

    let mut swapped = snapshot.clone();
    swapped["budget_policy"]["policy_version"] = serde_json::json!(p.version() + 1);
    let swapped_digest = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&swapped).unwrap())
    );
    assert_eq!(
        require_signed_policy_identity(&swapped, &swapped_digest, Some(&p)),
        Err(Error::BudgetPolicyInvalid)
    );
    assert_eq!(
        require_signed_policy_identity(&swapped, &digest, Some(&p)),
        Err(Error::BudgetPolicyInvalid)
    );

    let missing = serde_json::json!({"budget_policy_id": p.id().to_string()});
    let missing_digest = format!(
        "{:x}",
        sha2::Sha256::digest(serde_json::to_vec(&missing).unwrap())
    );
    assert_eq!(
        require_signed_policy_identity(&missing, &missing_digest, Some(&p)),
        Err(Error::BudgetPolicyInvalid)
    );
}
#[test]
fn exact_edges_retry_and_unknown_monotonic_elapsed() {
    let p = policy();
    let prior = ReservationPriorUsage {
        calls: 0,
        request_bytes: 0,
        retries: 0,
        elapsed_ms: 0,
    };
    assert_eq!(
        remaining_budget_envelope(
            &p,
            prior,
            ReservationRequestUsage {
                request_bytes: 10,
                is_retry: false,
                monotonic_elapsed_ms: None,
            }
        ),
        Ok(100)
    );
    assert_eq!(
        remaining_budget_envelope(
            &p,
            prior,
            ReservationRequestUsage {
                request_bytes: 11,
                is_retry: false,
                monotonic_elapsed_ms: None,
            }
        ),
        Err(Error::BudgetExhaustedBeforeDispatch)
    );
    let retry_prior = ReservationPriorUsage {
        calls: 1,
        request_bytes: 5,
        retries: 0,
        elapsed_ms: 0,
    };
    assert_eq!(
        remaining_budget_envelope(
            &p,
            retry_prior,
            ReservationRequestUsage {
                request_bytes: 5,
                is_retry: true,
                monotonic_elapsed_ms: Some(99),
            }
        ),
        Ok(1)
    );
    assert_eq!(
        remaining_budget_envelope(
            &p,
            retry_prior,
            ReservationRequestUsage {
                request_bytes: 5,
                is_retry: true,
                monotonic_elapsed_ms: Some(100),
            }
        ),
        Err(Error::BudgetExhaustedBeforeDispatch)
    );
    assert_eq!(
        remaining_budget_envelope(
            &p,
            retry_prior,
            ReservationRequestUsage {
                request_bytes: 5,
                is_retry: true,
                monotonic_elapsed_ms: None,
            }
        ),
        Err(Error::BudgetPolicyInvalid)
    );
    assert_eq!(
        remaining_budget_envelope(
            &p,
            ReservationPriorUsage {
                calls: 2,
                request_bytes: 0,
                retries: 0,
                elapsed_ms: 0,
            },
            ReservationRequestUsage {
                request_bytes: 1,
                is_retry: false,
                monotonic_elapsed_ms: None,
            }
        ),
        Err(Error::BudgetExhaustedBeforeDispatch)
    );
    assert_eq!(
        remaining_budget_envelope(
            &p,
            ReservationPriorUsage {
                calls: 0,
                request_bytes: 0,
                retries: 1,
                elapsed_ms: 0,
            },
            ReservationRequestUsage {
                request_bytes: 1,
                is_retry: true,
                monotonic_elapsed_ms: Some(1),
            }
        ),
        Err(Error::BudgetExhaustedBeforeDispatch)
    );
}
