use super::*;

#[test]
fn signed_scope_family_requires_exact_scope_target() {
    let target = Uuid::from_u128(1);
    assert_eq!(
        signed_scope_dispatch_target(
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
            "scope_candidate_set",
            Some(target),
        )
        .unwrap(),
        target,
    );
    for target in [None, Some(Uuid::nil())] {
        assert!(matches!(
            signed_scope_dispatch_target(
                AdvisoryCapability::ScopeDecomposition,
                AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
                "scope_candidate_set",
                target,
            ),
            Err(Error::InputConflict)
        ));
    }
}

#[test]
fn signed_scope_family_rejects_foreign_matrix_and_wrong_kind() {
    for (capability, point, kind) in [
        (
            AdvisoryCapability::EngineeringProfile,
            AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
            "matrix_task",
        ),
        (
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
            "scope_candidate_set",
        ),
        (
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
            "program",
        ),
    ] {
        assert!(matches!(
            signed_scope_dispatch_target(capability, point, kind, Some(Uuid::from_u128(1))),
            Err(Error::InputConflict)
        ));
    }
}

#[test]
fn signed_scope_default_source_contract_is_forbidden() {
    // Static default-body contract only: no mock transaction/rollback proof.
    let source = include_str!("../../../../application/src/advisory_ports.rs");
    let body = source
        .split("async fn start_signed_scope_dispatch(")
        .nth(1)
        .unwrap()
        .split("async fn start_advisory_dispatch(")
        .next()
        .unwrap();
    assert!(body.contains("Err(Error::Forbidden)"));
}
