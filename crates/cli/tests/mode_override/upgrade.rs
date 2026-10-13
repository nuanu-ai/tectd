//! A database of its own brought to the schema before one migration and seeded with
//! what a workspace then held, for the tests of what that migration leaves alone.
use serde_json::{Value, json};
use sqlx::PgPool;
use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use std::{fs, future::Future, path::Path, str::FromStr};
use uuid::Uuid;

/// Every migration that came before `version`, which are `count` files.
pub fn migrations_before(version: &str, count: usize) -> tempfile::TempDir {
    let all = Path::new(env!("CARGO_MANIFEST_DIR")).join("../postgres/migrations");
    let earlier = tempfile::tempdir().unwrap();
    let mut copied = 0;
    for entry in fs::read_dir(all).unwrap() {
        let entry = entry.unwrap();
        let name = entry.file_name().into_string().unwrap();
        if name.as_str() < version {
            fs::copy(entry.path(), earlier.path().join(&name)).unwrap();
            copied += 1;
        }
    }
    assert_eq!(copied, count);
    earlier
}

/// Bring `pool` to the schema before `version`.
pub async fn migrate_before(pool: &PgPool, version: &str, count: usize) {
    sqlx::migrate::Migrator::new(migrations_before(version, count).path())
        .await
        .unwrap()
        .run(pool)
        .await
        .unwrap();
}

pub struct Rows {
    pub tenant: Uuid,
    pub workspace: Uuid,
    pub scope: Uuid,
    pub slice: Uuid,
    pub scope_set: Uuid,
    pub slice_set: Uuid,
}

/// One Program with a Scope candidate draft, an opened Scope with a Slice candidate
/// draft, and an opened Slice, written as the schema before the mode override takes
/// them; later migrations up to the one under test leave these columns as they are.
pub async fn seed(pool: &PgPool) -> Rows {
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
        tenant,
        workspace,
        scope,
        slice,
        scope_set,
        slice_set,
    }
}

/// Run `check` against a database of its own, which is dropped whatever the check
/// finds. The check gets the operator's pool to it and the runtime role.
pub async fn in_own_database<Check, Checked>(prefix: &str, check: Check)
where
    Check: FnOnce(PgPool, String) -> Checked + Send + 'static,
    Checked: Future<Output = ()> + Send + 'static,
{
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_role =
        std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let options = PgConnectOptions::from_str(&admin_url).unwrap();
    let base = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(options.clone())
        .await
        .unwrap();
    let database = format!("{prefix}_{}", Uuid::new_v4().simple());
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
    let checked = tokio::spawn(check(pool.clone(), runtime_role)).await;
    pool.close().await;
    sqlx::query(&format!("DROP DATABASE \"{database}\" WITH (FORCE)"))
        .execute(&base)
        .await
        .expect("drop the test-owned upgrade database");
    checked.unwrap();
}
