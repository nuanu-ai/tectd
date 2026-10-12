//! Migration 0044 on a database that already holds a Scope, a Slice and drafts: the
//! rows are left as they were, the two new columns of each are NULL, and the drafts
//! still read as drafts without an override.
use serde_json::{Value, json};
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};
use std::{fs, path::Path, str::FromStr};
use tect_domain::{ResolvedCandidateDraft, ResolvedSliceCandidateDraft, SliceCandidateNode};
use tect_postgres::admin;
use uuid::Uuid;

/// Every migration that came before the override.
fn earlier_migrations() -> tempfile::TempDir {
    let all = Path::new(env!("CARGO_MANIFEST_DIR")).join("../postgres/migrations");
    let earlier = tempfile::tempdir().unwrap();
    let mut copied = 0;
    for entry in fs::read_dir(all).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if name.as_str() < "0044" {
            fs::copy(entry.path(), earlier.path().join(&name)).unwrap();
            copied += 1;
        }
    }
    assert_eq!(copied, 43);
    earlier
}

struct Rows {
    scope: Uuid,
    slice: Uuid,
    scope_set: Uuid,
    slice_set: Uuid,
}

/// One Program with a Scope candidate draft, an opened Scope with a Slice candidate
/// draft, and an opened Slice, written as the schema before the override takes them.
async fn seed(pool: &PgPool) -> Rows {
    let [tenant, workspace, program, scope_set, scope_snapshot] = [(); 5].map(|()| Uuid::new_v4());
    let [scope, slice_set, slice_snapshot, slice, candidate, node] =
        [(); 6].map(|()| Uuid::new_v4());
    let run = async |sql: &str, ids: &[Uuid], payload: Option<&Value>| {
        let mut query = sqlx::query(sql);
        for id in ids {
            query = query.bind(*id);
        }
        if let Some(payload) = payload {
            query = query.bind(payload);
        }
        query.execute(pool).await.unwrap();
    };
    run("INSERT INTO tenants(id) VALUES($1)", &[tenant], None).await;
    run(
        "INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,'before-override')",
        &[workspace, tenant],
        None,
    )
    .await;
    run(
        "INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,\
         boundaries,constraints,success,mode,current_step,input_cursor,latest_input,\
         max_input_bytes) VALUES($1,$2,$3,'open',2,'Program','Intent','Basis','Boundaries',\
         'Constraints','Success','prod','ready',1,1,4096)",
        &[program, tenant, workspace],
        None,
    )
    .await;
    run(
        "INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,\
         origin_request_id,origin_input,origin_payload,origin_result,revision,status,boundary,\
         input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'planning input',\
         '{}'::jsonb,'{}'::jsonb,3,'ready','ongoing',1,1,4096)",
        &[scope_set, tenant, workspace, program, Uuid::new_v4()],
        None,
    )
    .await;
    run(
        "INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) \
         VALUES($1,$2,pg_catalog.repeat('a',64),'program body')",
        &[tenant, workspace],
        None,
    )
    .await;
    run(
        "INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,\
         sequence,program_revision,program_latest_input,planning_latest_input,\
         program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,\
         method_revision,method_digest,method_body,method_origin_refs,registry_revision,\
         registry_digest,rules) VALUES($1,$2,$3,$4,1,2,1,1,pg_catalog.repeat('a',64),\
         '{}'::uuid[],pg_catalog.repeat('b',64),'method','1',pg_catalog.repeat('c',64),\
         'method body','[]'::jsonb,'3',pg_catalog.repeat('d',64),'[]'::jsonb)",
        &[scope_snapshot, tenant, workspace, scope_set],
        None,
    )
    .await;
    run(
        "UPDATE scope_candidate_sets SET current_snapshot_id=$2 WHERE id=$1",
        &[scope_set, scope_snapshot],
        None,
    )
    .await;
    let scope_draft = json!({"boundary":"ongoing","goals":[],"evidence":[],"blockers":[],
        "pending_question":null,"empty_disposition":null,"protected_changes":[],
        "delta":{"added":[{"candidate_id":candidate,"revision":1}],"changed":[],
            "unchanged":[],"superseded":[]},
        "candidates":[{"id":candidate,"revision":1,"title":"Export","outcome":"A file",
            "trigger":"A request","delivered_behavior":"A file is written","proof":"It opens",
            "includes":["csv"],"excludes":[],"dependencies":[],"coverage_goal_ids":[],
            "evidence_ids":[]}]});
    run(
        "INSERT INTO scope_candidate_drafts(tenant_id,workspace_id,candidate_set_id,\
         set_revision,payload) VALUES($1,$2,$3,2,$4)",
        &[tenant, workspace, scope_set],
        Some(&scope_draft),
    )
    .await;
    run(
        "INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,\
         source_candidate_set_revision,source_snapshot_id,source_candidate_id,\
         source_candidate_revision,boundary,title,outcome,includes,excludes,\
         origin_request_id,origin_payload) VALUES($1,$2,$3,$4,3,$5,$6,1,'ongoing','Export',\
         'A file','[\"csv\"]'::jsonb,'[]'::jsonb,$7,'{}'::jsonb)",
        &[
            scope,
            tenant,
            workspace,
            scope_set,
            scope_snapshot,
            candidate,
            Uuid::new_v4(),
        ],
        None,
    )
    .await;
    run(
        "INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id,revision,status) \
         VALUES($1,$2,$3,$4,3,'ready')",
        &[slice_set, tenant, workspace, scope],
        None,
    )
    .await;
    run(
        "INSERT INTO slice_planning_snapshots(id,tenant_id,workspace_id,candidate_set_id,\
         sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,\
         planning_latest_input,method,registry_revision,registry_digest,rules,catalogue) \
         VALUES($1,$2,$3,$4,1,1,3,$5,0,'{}'::jsonb,'3','digest','[]'::jsonb,'{}'::jsonb)",
        &[slice_snapshot, tenant, workspace, slice_set, scope_snapshot],
        None,
    )
    .await;
    run(
        "UPDATE slice_candidate_sets SET current_snapshot_id=$2 WHERE id=$1",
        &[slice_set, slice_snapshot],
        None,
    )
    .await;
    run(
        "UPDATE native_scopes SET slice_candidate_set_id=$2 WHERE id=$1",
        &[scope, slice_set],
        None,
    )
    .await;
    let slice_draft = json!({"coverage_summary":"The whole Scope","supersessions":[],
        "nodes":[{"kind":"work","id":node,"revision":1,"title":"Write the file",
            "outcome":"A file","includes":["csv"],"excludes":[],"dependencies":[],
            "proof":["It opens"],"pipeline":"slice.lightweight-tdd-development",
            "pipeline_reason":"Bounded","why_lightweight_insufficient":null,
            "why_further_vertical_split_not_viable":null,"source_result_ids":[]}]});
    run(
        "INSERT INTO slice_candidate_drafts(tenant_id,workspace_id,candidate_set_id,\
         set_revision,payload) VALUES($1,$2,$3,2,$4)",
        &[tenant, workspace, slice_set],
        Some(&slice_draft),
    )
    .await;
    run(
        "INSERT INTO native_slices(id,tenant_id,workspace_id,scope_id,candidate_id,\
         candidate_revision,opening_snapshot_id,title,outcome,pipeline,origin_request_id,\
         origin_payload) VALUES($1,$2,$3,$4,$5,1,$6,'Write the file','A file',\
         'slice.lightweight-tdd-development',$7,'{}'::jsonb)",
        &[
            slice,
            tenant,
            workspace,
            scope,
            node,
            slice_snapshot,
            Uuid::new_v4(),
        ],
        None,
    )
    .await;
    Rows {
        scope,
        slice,
        scope_set,
        slice_set,
    }
}

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
    sqlx::migrate::Migrator::new(earlier_migrations().path())
        .await
        .unwrap()
        .run(pool)
        .await
        .unwrap();
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
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_role =
        std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let options = PgConnectOptions::from_str(&admin_url).unwrap();
    let base = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let database = format!("tect_override_upgrade_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE \"{database}\""))
        .execute(&base)
        .await
        .unwrap();
    let pool = PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options.database(&database))
        .await
        .unwrap();
    // The checks run in a task, so that the database is dropped whatever they find.
    let checked = tokio::spawn({
        let pool = pool.clone();
        async move { upgrade(&pool, &runtime_role).await }
    })
    .await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&base)
        .await
        .expect("drop the test-owned upgrade database");
    checked.unwrap();
}
