const BINDING: &str = include_str!("../migrations/0056_matrix_advisory_binding.sql");
const REASONS: &str = include_str!("../migrations/0058_matrix_advisory_verification_reasons.sql");
const PRIOR: &str = include_str!("../migrations/0047_advisory_budget_no_call_reason.sql");

#[test]
fn no_choice_only_extends_engineering_no_call_and_retains_revision_binding() {
    for required in [
        "matrix_task_revision IS NOT NULL",
        "source_revision = matrix_task_revision::text",
        "matrix_choice_set_digest IS NULL AND state = 'no_call'",
        "matrix_choice_set_digest IS NOT NULL",
        "matrix_choice_set_digest ~ '^[0-9a-f]{64}$'",
        "advisory_opportunity_matrix_choice_fk",
        "FOREIGN KEY (tenant_id, workspace_id, work_item_id, matrix_task_revision)",
        "REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision)",
    ] {
        assert!(BINDING.contains(required), "missing {required}");
    }
}
#[test]
fn new_reason_is_no_call_only_and_dispatch_requires_choice() {
    assert_eq!(REASONS.matches("'choice_set_not_applicable'").count(), 2);
    assert_eq!(
        REASONS
            .matches("'budget_exhausted_before_dispatch'")
            .count(),
        2
    );
    assert!(PRIOR.contains("'budget_policy_invalid'"));
    for required in [
        "BEFORE INSERT ON advisory_dispatch",
        "o.capability = 'engineering_profile'",
        "o.matrix_choice_set_digest IS NULL",
        "FOR SHARE",
        "IF NOT FOUND THEN",
    ] {
        assert!(BINDING.contains(required), "missing {required}");
    }
}
#[test]
fn revision_reason_only_extends_matrix_invalidation_constraints() {
    assert!(REASONS.contains("'matrix_task_revision_changed','matrix_verification_stale'"));
    assert!(
        REASONS.contains("AND capability='engineering_profile' AND work_item_kind='matrix_task'")
    );
    for required in [
        "state='advised' AND primary_reason='provider_response'",
        "state='invalidated' AND primary_reason='configuration_changed'",
        "state='unresolved' AND primary_reason='send_unknown'",
    ] {
        assert!(REASONS.contains(required), "missing {required}");
    }
}
#[test]
fn readiness_reasons_are_matrix_no_call_only() {
    for reason in [
        "matrix_evidence_unresolved",
        "matrix_source_unverified",
        "matrix_task_unbound",
        "matrix_snapshot_missing",
        "matrix_binding_mismatch",
        "matrix_context_unresolved",
        "matrix_context_stale",
        "matrix_authority_schema_unsupported",
        "matrix_operating_evidence_unresolved",
    ] {
        assert_eq!(
            REASONS.matches(&format!("'{reason}'")).count(),
            2,
            "{reason}"
        );
    }
    assert!(REASONS.contains("state='prepared' AND primary_reason='dispatch_authorized'"));
}
