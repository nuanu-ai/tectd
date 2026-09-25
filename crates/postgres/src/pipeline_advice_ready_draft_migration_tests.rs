const MIGRATION: &str = include_str!("../migrations/0064_pipeline_advice_ready_draft_binding.sql");
const ADMIN: &str = include_str!("admin/pipeline_advice.rs");

#[test]
fn ready_set_binds_latest_saved_draft_and_matched_effect() {
    for required in [
        "CREATE OR REPLACE FUNCTION pipeline_advice_context_require_current()",
        "c.status='ready' AND c.revision=NEW.candidate_set_revision",
        "JOIN public.slice_candidate_reviews AS review",
        "review.set_revision)=",
        "review.payload->>'verdict'='ready'",
        "JOIN public.slice_candidate_drafts AS draft",
        "draft.set_revision)=",
        "a.result_revision=draft.set_revision",
        "l.result_revision=draft.set_revision",
        "draft.set_revision<c.revision",
        "NOT EXISTS (",
        "newer.set_revision>draft.set_revision",
        "NOT draft.payload_erased AND draft.payload IS NOT NULL",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(!MIGRATION.contains("a.result_revision=c.revision"));
    assert!(!MIGRATION.contains("l.result_revision=c.revision"));
}

#[test]
fn guard_locks_current_work_matrix_and_actor_rows() {
    for required in [
        "snapshot.source_snapshot_id=NEW.source_snapshot_id",
        "task.current_revision=l.task_revision",
        "a.verdict='match'",
        "node->>'node_id'=NEW.work_node_id::text",
        "node->>'node_revision'=NEW.work_node_revision::text",
        "pg_catalog.jsonb_array_elements(draft.payload->'nodes')",
        "node->>'kind'='work'",
        "node->>'revision'=NEW.work_node_revision::text",
        "FOR SHARE OF o,c,snapshot,review,draft,a,l,d,task,receipt,s,h,p,m",
    ] {
        assert!(MIGRATION.contains(required), "missing {required}");
    }
    assert!(ADMIN.contains("'a.result_revision=draft.set_revision'"));
}
