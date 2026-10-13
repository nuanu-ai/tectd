//! Migration 0044 on a database that already holds a Scope, a Slice and drafts: the
//! rows are left as they were, the two new columns of each are NULL, and the drafts
//! still read as drafts without an override.
use crate::upgrade::{Rows, in_own_database, migrate_before, seed};
use serde_json::{Value, json};
use sqlx::{PgPool, Row};
use tect_domain::{ResolvedCandidateDraft, ResolvedSliceCandidateDraft, SliceCandidateNode};
use tect_postgres::admin;
use uuid::Uuid;

/// The Scope row, the Slice row and the two draft payloads as text. The columns the
/// migration adds are left out of the rows: they did not exist before it.
async fn held(pool: &PgPool, rows: &Rows) -> [String; 4] {
    let row = sqlx::query(
        "SELECT (pg_catalog.to_jsonb(n) - 'mode_override' - 'mode_override_source_ref_id')::text, \
                (pg_catalog.to_jsonb(s) - 'mode_override' - 'mode_override_input_id')::text, \
                d.payload::text, e.payload::text \
         FROM native_scopes n, native_slices s, scope_candidate_drafts d, \
              slice_candidate_drafts e \
         WHERE n.id=$1 AND s.id=$2 AND d.candidate_set_id=$3 AND e.candidate_set_id=$4",
    )
    .bind(rows.scope)
    .bind(rows.slice)
    .bind(rows.scope_set)
    .bind(rows.slice_set)
    .fetch_one(pool)
    .await
    .unwrap();
    [row.get(0), row.get(1), row.get(2), row.get(3)]
}

async fn upgrade(pool: &PgPool, runtime_role: &str) {
    // Every migration that came before the override.
    migrate_before(pool, "0044", 43).await;
    let rows = seed(pool).await;
    let before = held(pool, &rows).await;
    admin::migrate(pool, runtime_role).await.unwrap();
    let after = held(pool, &rows).await;
    assert_eq!(before, after);

    // No default and no backfill: every new column of an existing row is NULL.
    let empty: (bool, bool) = sqlx::query_as(
        "SELECT (SELECT mode_override IS NULL AND mode_override_source_ref_id IS NULL \
                 FROM native_scopes WHERE id=$1), \
                (SELECT mode_override IS NULL AND mode_override_input_id IS NULL \
                 FROM native_slices WHERE id=$2)",
    )
    .bind(rows.scope)
    .bind(rows.slice)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(empty, (true, true));

    // The drafts read as drafts without an override and would be written back unchanged.
    let payload: Value = serde_json::from_str(&after[2]).unwrap();
    let draft: ResolvedCandidateDraft = serde_json::from_value(payload.clone()).unwrap();
    assert_eq!(draft.candidates[0].mode_override, None);
    assert_eq!(json!(draft), payload);
    let payload: Value = serde_json::from_str(&after[3]).unwrap();
    let draft: ResolvedSliceCandidateDraft = serde_json::from_value(payload.clone()).unwrap();
    assert!(matches!(
        &draft.nodes[0],
        SliceCandidateNode::Work {
            mode_override: None,
            ..
        }
    ));
    assert_eq!(json!(draft), payload);

    // The level and the id that permitted it are set together or not at all, and the
    // level is one of the four.
    let id = Uuid::new_v4();
    for (table, key, column) in [
        ("native_scopes", rows.scope, "mode_override_source_ref_id"),
        ("native_slices", rows.slice, "mode_override_input_id"),
    ] {
        let set = async |level: Option<&str>, by: Option<Uuid>| {
            sqlx::query(&format!(
                "UPDATE {table} SET mode_override=$2, {column}=$3 WHERE id=$1"
            ))
            .bind(key)
            .bind(level)
            .bind(by)
            .execute(pool)
            .await
            .is_ok()
        };
        assert!(!set(Some("demo"), None).await, "{table}: a level alone");
        assert!(!set(None, Some(id)).await, "{table}: an id alone");
        assert!(
            !set(Some("enterprise"), Some(id)).await,
            "{table}: a fifth level"
        );
        assert!(!set(Some(""), Some(id)).await, "{table}: an empty level");
        for level in ["demo", "mvp", "prod", "serious_prod"] {
            assert!(set(Some(level), Some(id)).await, "{table}: {level}");
        }
        assert!(set(None, None).await, "{table}: cleared");
    }
}

#[tokio::test]
async fn migration_0044_leaves_existing_scopes_slices_and_drafts_without_an_override() {
    in_own_database("tect_override_upgrade", |pool, runtime_role| async move {
        upgrade(&pool, &runtime_role).await;
    })
    .await;
}
