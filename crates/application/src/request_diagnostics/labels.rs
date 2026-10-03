//! Admission bounds prevent diagnostic-only constants from exceeding the file budget.
pub(super) const BYTE_CAP: usize = 48;
pub(super) const COUNTER_BYTE_CAP: usize = 64;

pub(super) fn valid(label: &str, byte_cap: usize) -> bool {
    !label.is_empty()
        && label.len() <= byte_cap
        && label
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || matches!(byte, b'.' | b'_'))
}

pub(super) fn normalize(label: &'static str, fallback: &'static str) -> (&'static str, bool) {
    if valid(label, BYTE_CAP) {
        (label, false)
    } else {
        (fallback, true)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn current_production_inventory_is_admitted() {
        let stages = [
            "application.commit",
            "application.completion_preflight",
            "application.completion_validation",
            "application.context_assembly",
            "application.native_planning_authorization",
            "application.native_session_binding",
            "application.native_session_lock",
            "application.output_guard",
            "application.phase_mutation",
            "pg.commit",
            "pg.context_assembly",
            "pg.context_authorization",
            "pg.manifest_snapshot",
            "pg.native_batch_sql",
            "pg.native_session_advisory_lock",
            "pg.next_input_capture",
            "pg.publication_native_batch",
            "pg.publication_proof_preload",
            "pg.returned_context",
            "pg.run_for_update",
            "pg.selected_manifest_validation",
            "pg.transaction_begin_including_acquire",
            "pg.workspace_knowledge_lock",
            "response_finalization",
            "service_and_projection",
        ];
        assert!(stages.into_iter().all(|label| valid(label, BYTE_CAP)));
        let counters = [
            "context.authorization_requested_keys",
            "context.authorization_returned_rows",
            "proof.already_verified_unique_keys",
            "proof.event_rows",
            "proof.native_batch_calls",
            "proof.native_batch_requested_keys",
            "proof.native_batch_returned_rows_including_sentinels",
            "proof.native_batch_validated_triples",
            "proof.receipt_rows",
            "proof.requested_key_occurrences",
            "proof.requested_unique_keys",
            "proof.unique_unverified_keys",
            "proof.verify_cache_hits",
            "proof.verify_calls",
        ];
        assert!(
            counters
                .into_iter()
                .all(|label| valid(label, COUNTER_BYTE_CAP))
        );
        let tools = ["slice_pipeline_context", "slice_pipeline_phase_complete"];
        assert!(tools.into_iter().all(|label| valid(label, BYTE_CAP)));
        let outcomes = [
            "capacity_exceeded",
            "context_changed",
            "forbidden",
            "input_conflict",
            "input_pending",
            "internal_invariant",
            "invalid_arguments",
            "invalid_configuration",
            "invalid_native_session",
            "invalid_source",
            "invalid_workspace_key",
            "invalid_worktree_selection",
            "knowledge_lifecycle_required",
            "knowledge_payload_erased",
            "knowledge_unavailable",
            "needs_context",
            "not_found",
            "ok",
            "operation_timeout",
            "program_incomplete",
            "request_too_large",
            "session_revoked",
            "session_workspace_mismatch",
            "setup_already_applied",
            "setup_exists",
            "setup_file_conflict",
            "setup_incomplete",
            "setup_unavailable",
            "stale_context",
            "stale_revision",
            "storage_unavailable",
            "task_directory_mismatch",
            "task_directory_unbound",
            "transport_unavailable",
            "unauthorized",
            "unsupported_completion_requirement",
            "workspace_not_open",
        ];
        assert!(outcomes.into_iter().all(|label| valid(label, BYTE_CAP)));
    }
}
