const MIGRATION: &str = include_str!("../migrations/0050_matrix_no_choice_no_call.sql");
const PRIOR: &str = include_str!("../migrations/0049_engineering_profile_opportunity.sql");
const PRIOR_REASON: &str = include_str!("../migrations/0044_advisory_budget_no_call_reason.sql");

fn check_expression<'a>(migration: &'a str, constraint: &str) -> &'a str {
    let start = migration
        .find(constraint)
        .unwrap_or_else(|| panic!("missing {constraint}"));
    let remainder = &migration[start + constraint.len()..];
    remainder
        .split_once(");")
        .unwrap_or_else(|| panic!("unterminated {constraint}"))
        .0
}

fn normalized(expression: &str) -> String {
    expression.split_whitespace().collect::<Vec<_>>().join(" ")
}

#[test]
fn no_choice_only_extends_engineering_no_call_and_retains_revision_binding() {
    assert!(MIGRATION.contains("DROP CONSTRAINT advisory_opportunity_matrix_binding_check"));
    assert!(MIGRATION.contains("matrix_task_revision IS NOT NULL"));
    assert!(MIGRATION.contains("source_revision = matrix_task_revision::text"));
    assert!(MIGRATION.contains("matrix_choice_set_digest IS NULL AND state = 'no_call'"));
    assert!(MIGRATION.contains("matrix_choice_set_digest IS NOT NULL"));
    assert!(MIGRATION.contains("matrix_choice_set_digest ~ '^[0-9a-f]{64}$'"));
    assert!(
        MIGRATION
            .contains("FOREIGN KEY (tenant_id, workspace_id, work_item_id, matrix_task_revision)")
    );
    assert!(
        MIGRATION.contains(
            "REFERENCES matrix_task_revisions (tenant_id, workspace_id, task_id, revision)"
        )
    );
    assert!(PRIOR.contains("ADD CONSTRAINT advisory_opportunity_matrix_choice_fk"));
    assert!(!MIGRATION.contains("DROP CONSTRAINT advisory_opportunity_matrix_choice_fk"));
}

#[test]
fn new_reason_is_no_call_only_and_dispatch_requires_choice() {
    assert!(MIGRATION.contains("'budget_policy_invalid', 'choice_set_not_applicable'"));
    assert_eq!(MIGRATION.matches("'choice_set_not_applicable'").count(), 2);
    assert!(MIGRATION.contains("state = 'no_call' AND primary_reason IN"));
    for constraint in [
        "ADD CONSTRAINT advisory_opportunity_reason_check CHECK (",
        "ADD CONSTRAINT advisory_opportunity_state_reason_check CHECK (",
    ] {
        let before = normalized(check_expression(PRIOR_REASON, constraint));
        let after = normalized(
            &check_expression(MIGRATION, constraint).replace(", 'choice_set_not_applicable'", ""),
        );
        assert_eq!(after, before, "existing {constraint} cases changed");
    }
    assert!(MIGRATION.contains("BEFORE INSERT ON advisory_dispatch"));
    assert!(MIGRATION.contains("o.capability = 'engineering_profile'"));
    assert!(MIGRATION.contains("o.matrix_choice_set_digest IS NULL"));
    assert!(MIGRATION.contains("FOR SHARE"));
    assert!(MIGRATION.contains("IF NOT FOUND THEN"));
}
