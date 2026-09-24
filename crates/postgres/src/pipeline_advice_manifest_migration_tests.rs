const MIGRATION: &str = include_str!("../migrations/0063_pipeline_advice_manifest_no_call.sql");
const ADMIN: &str = include_str!("admin/pipeline_advice.rs");

#[test]
fn empty_eligibility_is_reserved_for_durable_no_call() {
    for required in [
        "DROP CONSTRAINT pipeline_advice_contexts_eligible_kind_ids_check",
        "pg_catalog.cardinality(eligible_kind_ids) BETWEEN 0 AND 8",
        "pg_catalog.cardinality(NEW.eligible_kind_ids)=0",
        "opportunity_state <> 'no_call'",
        "advisory_opportunity_pipeline_empty_no_call",
        "NEW.state<>'no_call'",
        "FOR SHARE",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
}

#[test]
fn new_rows_keep_exact_immutable_manifest_material() {
    for required in [
        "ADD COLUMN manifest_payload jsonb",
        "ADD COLUMN manifest_digest text",
        "pipeline_advice_context_manifest_digest_check",
        "pipeline_advice_context_manifest_shape_check",
        "'tect.pipeline-recommendation/1'",
        "manifest_payload->>'work_id' = work_node_id::text",
        "manifest_payload->>'catalogue_digest' = catalogue_digest",
        "option_ids IS DISTINCT FROM NEW.eligible_kind_ids",
        "NEW.manifest_payload->>'matrix_task_id'=l.task_id::text",
        "NEW.manifest_payload->>'matrix_verification_digest'=l.verification_digest",
        "REVOKE ALL PRIVILEGES ON FUNCTION pipeline_advice_manifest_require_shape() FROM PUBLIC",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(MIGRATION.contains("false)) NOT VALID"));
    assert!(ADMIN.contains("'manifest_payload','manifest_digest'"));
    assert!(ADMIN.contains("pipeline_advice_context_manifest_shape"));
    assert!(ADMIN.contains("pipeline_advice_preserve_empty_no_call"));
}

#[test]
fn prepared_reason_is_limited_to_pipeline_pre_open() {
    assert!(MIGRATION.contains("'recommendation_prepared'"));
    assert!(
        MIGRATION.contains("state = 'prepared' AND primary_reason = 'recommendation_prepared'")
    );
    assert!(MIGRATION.contains("capability = 'pipeline_recommendation'"));
    assert!(MIGRATION.contains("decision_point = 'pipeline_recommendation_before_slice_open'"));
}
