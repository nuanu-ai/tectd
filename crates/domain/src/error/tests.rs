use super::*;

#[test]
fn legacy_errors_map_to_typed_refusals() {
    assert_eq!(
        Error::StaleRevision.refusal().unwrap().code,
        RefusalCode::StaleRevision
    );
    assert_eq!(
        Error::InputConflict.refusal().unwrap().code,
        RefusalCode::IdempotencyConflict
    );
    assert_eq!(
        Error::RequestTooLarge.refusal().unwrap().code,
        RefusalCode::PayloadTooLarge
    );
    assert!(Error::NotFound.refusal().is_none());
}

#[test]
fn exhaustive_pipeline_branch_table_has_complete_stable_metadata() {
    struct Branch {
        name: &'static str,
        code: RefusalCode,
        rule: &'static str,
        path: &'static str,
        expected: &'static str,
        actual: &'static str,
        next_action: &'static str,
        required: &'static str,
    }
    let branches = [
        Branch {
            name: "stale_revision",
            code: RefusalCode::StaleRevision,
            rule: "WP6-REVISION-01",
            path: "arguments.params.run_revision",
            expected: "current run revision",
            actual: "stale_revision",
            next_action: "refresh_pipeline_context",
            required: "current_revision",
        },
        Branch {
            name: "stale_checkpoint_input",
            code: RefusalCode::StaleRevision,
            rule: "WP6-CHECKPOINT-REVISION-01",
            path: "arguments.params.checkpoint_revision",
            expected: "current checkpoint revision",
            actual: "stale_revision",
            next_action: "refresh_checkpoint",
            required: "current_checkpoint_revision",
        },
        Branch {
            name: "idempotency_conflict",
            code: RefusalCode::IdempotencyConflict,
            rule: "WP6-IDEMPOTENCY-01",
            path: "arguments.params.request_id",
            expected: "unused id or exact replay payload",
            actual: "idempotency_conflict",
            next_action: "reuse_exact_payload_or_new_request_id",
            required: "consistent_request_identity",
        },
        Branch {
            name: "invalid_output",
            code: RefusalCode::InvalidOutput,
            rule: "WP6-OUTPUT-01",
            path: "arguments.params.output",
            expected: "phase output contract",
            actual: "invalid_output",
            next_action: "correct_phase_output",
            required: "valid_output",
        },
        Branch {
            name: "payload_too_large",
            code: RefusalCode::PayloadTooLarge,
            rule: "WP6-SIZE-01",
            path: "arguments.params.output",
            expected: "payload within advertised byte limit",
            actual: "payload_too_large",
            next_action: "reduce_output",
            required: "bounded_output",
        },
        Branch {
            name: "evidence_missing",
            code: RefusalCode::EvidenceMissing,
            rule: "WP6-EVIDENCE-01",
            path: "arguments.params.output.evidence_artifacts",
            expected: "all required evidence references",
            actual: "evidence_missing",
            next_action: "register_required_evidence",
            required: "complete_evidence",
        },
        Branch {
            name: "evidence_scope_mismatch",
            code: RefusalCode::EvidenceMissing,
            rule: "WP6-EVIDENCE-SCOPE-01",
            path: "arguments.params.output.evidence_artifacts",
            expected: "evidence owned by this run and phase scope",
            actual: "evidence_scope_mismatch",
            next_action: "use_in_scope_evidence",
            required: "in_scope_evidence",
        },
        Branch {
            name: "artifact_not_ready",
            code: RefusalCode::ArtifactNotReady,
            rule: "WP6-ARTIFACT-01",
            path: "arguments.params.output.evidence_artifacts",
            expected: "ready artifact revision",
            actual: "artifact_not_ready",
            next_action: "finalize_evidence_artifact",
            required: "ready_artifact",
        },
        Branch {
            name: "ambiguous_requirement",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-REQUIREMENT-01",
            path: "arguments.params.output.fields",
            expected: "one unambiguous requirement disposition",
            actual: "ambiguous_requirement",
            next_action: "clarify_requirement",
            required: "unambiguous_requirement",
        },
        Branch {
            name: "unknown_cause",
            code: RefusalCode::UnknownCause,
            rule: "WP6-CAUSE-01",
            path: "arguments.params.output.fields.cause",
            expected: "evidence-backed cause or explicit unresolved state",
            actual: "unknown_cause",
            next_action: "investigate_cause",
            required: "known_or_explicitly_unresolved_cause",
        },
        Branch {
            name: "no_test_target",
            code: RefusalCode::NoTestTarget,
            rule: "WP6-TEST-01",
            path: "arguments.params.output.fields.selected_test_target",
            expected: "executable test target",
            actual: "no_test_target",
            next_action: "select_test_target",
            required: "selected_test_target",
        },
        Branch {
            name: "review_required",
            code: RefusalCode::ReviewRequired,
            rule: "WP6-REVIEW-01",
            path: "arguments.params.output.reviewer_context",
            expected: "required fresh review decision",
            actual: "review_required",
            next_action: "complete_required_review",
            required: "ready_review",
        },
        Branch {
            name: "authority_required",
            code: RefusalCode::AuthorityRequired,
            rule: "WP6-AUTHORITY-01",
            path: "request_context.principal",
            expected: "principal authorized for protected transition",
            actual: "authority_required",
            next_action: "obtain_authority",
            required: "authorized_principal",
        },
        Branch {
            name: "environment_mismatch",
            code: RefusalCode::AuthorityRequired,
            rule: "WP6-ENVIRONMENT-01",
            path: "arguments.params.output.fields.environment",
            expected: "authorized target environment",
            actual: "environment_mismatch",
            next_action: "select_authorized_environment",
            required: "authorized_environment",
        },
        Branch {
            name: "method_version_unavailable",
            code: RefusalCode::MethodVersionUnavailable,
            rule: "WP6-VERSION-01",
            path: "arguments.params.definition_version",
            expected: "available immutable definition version",
            actual: "method_version_unavailable",
            next_action: "select_supported_method",
            required: "supported_method_version",
        },
        Branch {
            name: "delivery_refresh_required",
            code: RefusalCode::DeliveryRefreshRequired,
            rule: "WP6-DELIVERY-REFRESH-01",
            path: "arguments.params.run_id",
            expected: "fresh delivered context",
            actual: "delivery_refresh_required",
            next_action: "refresh_pipeline_context",
            required: "fresh_delivery",
        },
        Branch {
            name: "legacy_migration_required",
            code: RefusalCode::LegacyMigrationRequired,
            rule: "WP6-MIGRATION-01",
            path: "arguments.params",
            expected: "explicit successor and obligation evidence mapping",
            actual: "legacy_migration_required",
            next_action: "provide_explicit_successor_mapping",
            required: "successor_mapping",
        },
        Branch {
            name: "coverage_incomplete",
            code: RefusalCode::CoverageIncomplete,
            rule: "WP6-COVERAGE-01",
            path: "arguments.params.output.fields.coverage",
            expected: "coverage or explicit blocker for every finite obligation",
            actual: "coverage_incomplete",
            next_action: "complete_coverage",
            required: "complete_coverage",
        },
        Branch {
            name: "dependency_stale",
            code: RefusalCode::DependencyStale,
            rule: "WP6-DEPENDENCY-01",
            path: "arguments.params.consumed_knowledge",
            expected: "current backend-bound dependency",
            actual: "dependency_stale",
            next_action: "refresh_dependencies",
            required: "current_dependency",
        },
        Branch {
            name: "effect_status_unknown",
            code: RefusalCode::EffectStatusUnknown,
            rule: "WP6-EFFECT-01",
            path: "arguments.params.output.fields.effect_status",
            expected: "verified effect status",
            actual: "effect_status_unknown",
            next_action: "reconcile_effect_status",
            required: "known_effect_status",
        },
        Branch {
            name: "backend_proof",
            code: RefusalCode::BackendDerivedProofRequired,
            rule: "WP6-PROOF-01",
            path: "arguments.params.consumed_outputs",
            expected: "omitted backend-derived proof",
            actual: "backend_derived_proof_required",
            next_action: "omit_agent_supplied_proof",
            required: "backend_derived_proof",
        },
        Branch {
            name: "schema_context",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-CONTEXT-01",
            path: "arguments.params",
            expected: "context schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_instruction",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-INSTRUCTION-01",
            path: "arguments.params",
            expected: "instruction schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_begin",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-BEGIN-01",
            path: "arguments.params",
            expected: "begin schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_migration",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-MIGRATION-01",
            path: "arguments.params",
            expected: "migration schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_complete",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-COMPLETE-01",
            path: "arguments.params",
            expected: "completion schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_input",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-INPUT-01",
            path: "arguments.params",
            expected: "input schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_delivery",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-DELIVERY-01",
            path: "arguments.params",
            expected: "delivery schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_checkpoint",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-CHECKPOINT-01",
            path: "arguments.params",
            expected: "checkpoint schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_evidence_write",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-EVIDENCE-WRITE-01",
            path: "arguments.params",
            expected: "evidence write schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
        Branch {
            name: "schema_evidence_read",
            code: RefusalCode::AmbiguousRequirement,
            rule: "WP6-SCHEMA-EVIDENCE-READ-01",
            path: "arguments.params",
            expected: "evidence read schema",
            actual: "invalid_arguments",
            next_action: "read_schema_and_retry",
            required: "valid_pipeline_arguments",
        },
    ];
    assert_eq!(branches.len(), 31);
    for branch in branches {
        let refusal = Refusal::new(branch.code).normalize_pipeline(
            branch.rule,
            branch.path,
            branch.expected,
            branch.actual,
            branch.next_action,
            branch.required,
        );
        assert!(refusal.is_complete_pipeline_refusal(), "{}", branch.name);
        assert_eq!(refusal.code, branch.code, "{}", branch.name);
        assert_eq!(
            refusal.rule.as_deref(),
            Some(branch.rule),
            "{}",
            branch.name
        );
        assert_eq!(
            refusal.path.as_deref(),
            Some(branch.path),
            "{}",
            branch.name
        );
        assert_eq!(
            refusal.expected.as_deref(),
            Some(branch.expected),
            "{}",
            branch.name
        );
        assert_eq!(
            refusal.actual.as_deref(),
            Some(branch.actual),
            "{}",
            branch.name
        );
        assert_eq!(
            refusal.next_action.as_deref(),
            Some(branch.next_action),
            "{}",
            branch.name
        );
        assert_eq!(
            refusal.required.as_deref(),
            Some(branch.required),
            "{}",
            branch.name
        );
    }
}
