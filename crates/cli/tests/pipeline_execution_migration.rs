use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};
use std::{fs, str::FromStr};
use tect_postgres::admin;
use uuid::Uuid;

const SCHEMA_SEVEN_MIGRATIONS: &[(&str, &str)] = &[
    (
        "0001_native_session_bootstrap.sql",
        include_str!("../../postgres/migrations/0001_native_session_bootstrap.sql"),
    ),
    (
        "0002_source_catalog_selection.sql",
        include_str!("../../postgres/migrations/0002_source_catalog_selection.sql"),
    ),
    (
        "0003_program_formation.sql",
        include_str!("../../postgres/migrations/0003_program_formation.sql"),
    ),
    (
        "0004_workspace_setup.sql",
        include_str!("../../postgres/migrations/0004_workspace_setup.sql"),
    ),
    (
        "0005_monthly_epoch_segmentation.sql",
        include_str!("../../postgres/migrations/0005_monthly_epoch_segmentation.sql"),
    ),
    (
        "0006_scope_candidate_planning.sql",
        include_str!("../../postgres/migrations/0006_scope_candidate_planning.sql"),
    ),
    (
        "0007_native_scope_slice_planning.sql",
        include_str!("../../postgres/migrations/0007_native_scope_slice_planning.sql"),
    ),
];

fn quoted_database(name: &str) -> String {
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    format!("\"{name}\"")
}

async fn connect_database(url: &str, database: &str) -> PgPool {
    let options = PgConnectOptions::from_str(url).unwrap().database(database);
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap()
}

#[tokio::test]
async fn populated_schema_seven_survives_pipeline_execution_upgrade() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let runtime_role =
        std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let base_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let base = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(base_options)
        .await
        .unwrap();
    let database = format!("tect_v7_pipeline_upgrade_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {}", quoted_database(&database)))
        .execute(&base)
        .await
        .unwrap();

    let result = run_upgrade(&admin_url, &runtime_url, &database, &runtime_role).await;

    sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        quoted_database(&database)
    ))
    .execute(&base)
    .await
    .expect("drop exact test-owned upgrade database");
    result.unwrap();
}

async fn run_upgrade(
    admin_url: &str,
    runtime_url: &str,
    database: &str,
    runtime_role: &str,
) -> Result<(), String> {
    let pool = connect_database(admin_url, database).await;
    let migration_dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    for (name, body) in SCHEMA_SEVEN_MIGRATIONS {
        fs::write(migration_dir.path().join(name), body).map_err(|error| error.to_string())?;
    }
    sqlx::migrate::Migrator::new(migration_dir.path())
        .await
        .map_err(|error| error.to_string())?
        .run(&pool)
        .await
        .map_err(|error| error.to_string())?;

    let ids = seed_schema_seven(&pool).await?;
    let before = preserved_rows(&pool, &ids).await?;

    admin::migrate(&pool, runtime_role)
        .await
        .map_err(|error| error.to_string())?;

    let after = preserved_rows(&pool, &ids).await?;
    if before != after {
        return Err("schema-7 Program, Scope, Slice, graph or Result data changed".into());
    }
    let migration_count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .map_err(|error| error.to_string())?;
    if migration_count != 8 {
        return Err(format!("expected 8 migrations, observed {migration_count}"));
    }
    let table_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname = ANY($1) AND c.relkind='r'",
    )
    .bind(vec![
        "slice_pipeline_runs",
        "slice_pipeline_phase_attempts",
        "slice_pipeline_phase_outputs",
        "slice_pipeline_output_bindings",
        "slice_pipeline_inputs",
        "slice_pipeline_receipts",
    ])
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    if table_count != 6 {
        return Err("schema 8 did not create all six pipeline tables".into());
    }
    let forced_rls_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_class c JOIN pg_catalog.pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname LIKE 'slice_pipeline_%' AND c.relrowsecurity AND c.relforcerowsecurity",
    )
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    if forced_rls_count != 6 {
        return Err("pipeline tables do not all enforce RLS".into());
    }
    let runtime_privileges: bool = sqlx::query_scalar(
        "SELECT \
         pg_catalog.has_table_privilege($1,'slice_pipeline_runs','SELECT,INSERT,UPDATE') AND \
         pg_catalog.has_table_privilege($1,'slice_pipeline_output_bindings','SELECT,INSERT,UPDATE') AND \
         pg_catalog.has_table_privilege($1,'slice_pipeline_phase_attempts','SELECT,INSERT') AND \
         NOT pg_catalog.has_table_privilege($1,'slice_pipeline_phase_attempts','UPDATE') AND \
         pg_catalog.has_column_privilege($1,'slice_pipeline_phase_attempts','result_payload','UPDATE') AND \
         pg_catalog.has_table_privilege($1,'slice_pipeline_phase_outputs','SELECT,INSERT') AND \
         NOT pg_catalog.has_table_privilege($1,'slice_pipeline_phase_outputs','UPDATE') AND \
         pg_catalog.has_table_privilege($1,'slice_pipeline_inputs','SELECT,INSERT') AND \
         NOT pg_catalog.has_table_privilege($1,'slice_pipeline_inputs','UPDATE') AND \
         pg_catalog.has_column_privilege($1,'slice_pipeline_inputs','result_payload','UPDATE') AND \
         pg_catalog.has_table_privilege($1,'slice_pipeline_receipts','SELECT,INSERT') AND \
         NOT pg_catalog.has_table_privilege($1,'slice_pipeline_receipts','UPDATE')",
    )
    .bind(runtime_role)
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    if !runtime_privileges {
        return Err(
            "pipeline table and result-payload column privileges are not least-privilege".into(),
        );
    }
    let binding_columns_null: bool = sqlx::query_scalar(
        "SELECT pipeline_run_id IS NULL AND pipeline_definition_version IS NULL \
         AND pipeline_definition_digest IS NULL AND pipeline_final_attempt_id IS NULL \
         AND pipeline_result_origin IS NULL FROM slice_results WHERE id=$1",
    )
    .bind(ids.result)
    .fetch_one(&pool)
    .await
    .map_err(|error| error.to_string())?;
    if !binding_columns_null {
        return Err("migration populated managed-run bindings on a legacy Result".into());
    }

    let runtime = connect_database(runtime_url, database).await;
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,false)")
        .bind(ids.tenant.to_string())
        .execute(&runtime)
        .await
        .map_err(|error| error.to_string())?;
    let visible: i64 = sqlx::query_scalar("SELECT count(*) FROM slice_pipeline_runs")
        .fetch_one(&runtime)
        .await
        .map_err(|error| error.to_string())?;
    if visible != 0 {
        return Err("empty upgraded fixture unexpectedly exposes pipeline runs".into());
    }
    runtime.close().await;
    pool.close().await;
    Ok(())
}

struct SeedIds {
    tenant: Uuid,
    program: Uuid,
    scope: Uuid,
    slice_set: Uuid,
    slice_snapshot: Uuid,
    slice: Uuid,
    result: Uuid,
}

async fn seed_schema_seven(pool: &PgPool) -> Result<SeedIds, String> {
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let source_set = Uuid::new_v4();
    let source_snapshot = Uuid::new_v4();
    let source_candidate = Uuid::new_v4();
    let scope = Uuid::new_v4();
    let slice_set = Uuid::new_v4();
    let slice_snapshot = Uuid::new_v4();
    let slice = Uuid::new_v4();
    let result = Uuid::new_v4();

    sqlx::query("INSERT INTO tenants(id) VALUES($1)")
        .bind(tenant)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("pipeline-upgrade-{workspace}"))
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) \
         VALUES($1,$2,$3,'open',1,'Pipeline upgrade','Preserve data','Schema 7 proof','One Slice','No runtime mutation','Rows survive','ready',1,1,4096)",
    )
    .bind(program)
    .bind(tenant)
    .bind(workspace)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,origin_result,revision,status,boundary,input_cursor,latest_input,max_input_bytes) \
         VALUES($1,$2,$3,$4,$5,'schema 7 source','{}','{}',1,'ready','ongoing',1,1,4096)",
    )
    .bind(source_set)
    .bind(tenant)
    .bind(workspace)
    .bind(program)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'schema 7 program body')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind("a".repeat(64))
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) \
         VALUES($1,$2,$3,$4,1,1,1,1,$5,'{}',$6,'schema-7','1',$7,'body','[]','1',$8,'[]')",
    )
    .bind(source_snapshot)
    .bind(tenant)
    .bind(workspace)
    .bind(source_set)
    .bind("a".repeat(64))
    .bind("b".repeat(64))
    .bind("c".repeat(64))
    .bind("d".repeat(64))
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$2 WHERE id=$1")
        .bind(source_set)
        .bind(source_snapshot)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO native_scopes(id,tenant_id,workspace_id,source_candidate_set_id,source_candidate_set_revision,source_snapshot_id,source_candidate_id,source_candidate_revision,boundary,title,outcome,includes,excludes,origin_request_id,origin_payload,origin_result) \
         VALUES($1,$2,$3,$4,1,$5,$6,1,'ongoing','Schema 7 Scope','Preserve native planning','[]','[]',$7,'{}','{}')",
    )
    .bind(scope)
    .bind(tenant)
    .bind(workspace)
    .bind(source_set)
    .bind(source_snapshot)
    .bind(source_candidate)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO slice_candidate_sets(id,tenant_id,workspace_id,scope_id,revision,status,input_cursor,latest_input) VALUES($1,$2,$3,$4,3,'ready',0,0)",
    )
    .bind(slice_set)
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO slice_planning_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,planning_latest_input,method,registry_revision,registry_digest,rules,catalogue) \
         VALUES($1,$2,$3,$4,1,1,1,$5,0,'{}','1',$6,'[]','{}')",
    )
    .bind(slice_snapshot)
    .bind(tenant)
    .bind(workspace)
    .bind(slice_set)
    .bind(source_snapshot)
    .bind("e".repeat(64))
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query("UPDATE slice_candidate_sets SET current_snapshot_id=$2 WHERE id=$1")
        .bind(slice_set)
        .bind(slice_snapshot)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query("UPDATE native_scopes SET slice_candidate_set_id=$2 WHERE id=$1")
        .bind(scope)
        .bind(slice_set)
        .execute(pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO native_slices(id,tenant_id,workspace_id,scope_id,candidate_id,candidate_revision,opening_snapshot_id,title,outcome,pipeline,state,origin_request_id,origin_payload,origin_result) \
         VALUES($1,$2,$3,$4,$5,1,$6,'Schema 7 Slice','Preserve result','slice.lightweight-tdd-development','completed',$7,'{}','{}')",
    )
    .bind(slice)
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .bind(Uuid::new_v4())
    .bind(slice_snapshot)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO slice_results(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,outcome,summary,evidence,scope_impact,remaining_work,request_id,request_payload,result_payload) \
         VALUES($1,$2,$3,$4,$5,1,1,'completed','Schema 7 result','[{\"kind\":\"test\",\"reference\":\"schema7\",\"observation\":\"preserved\"}]','None','None',$6,'{}','{}')",
    )
    .bind(result)
    .bind(tenant)
    .bind(workspace)
    .bind(scope)
    .bind(slice)
    .bind(Uuid::new_v4())
    .execute(pool)
    .await
    .map_err(|error| error.to_string())?;

    Ok(SeedIds {
        tenant,
        program,
        scope,
        slice_set,
        slice_snapshot,
        slice,
        result,
    })
}

async fn preserved_rows(pool: &PgPool, ids: &SeedIds) -> Result<Vec<String>, String> {
    let row = sqlx::query(
        "SELECT \
         (SELECT jsonb_build_object('revision',revision,'name',name,'intent',intent)::text FROM programs WHERE id=$1), \
         (SELECT jsonb_build_object('revision',revision,'title',title,'outcome',outcome,'slice_candidate_set_id',slice_candidate_set_id)::text FROM native_scopes WHERE id=$2), \
         (SELECT jsonb_build_object('revision',revision,'status',status,'current_snapshot_id',current_snapshot_id)::text FROM slice_candidate_sets WHERE id=$3), \
         (SELECT jsonb_build_object('sequence',sequence,'registry_digest',registry_digest)::text FROM slice_planning_snapshots WHERE id=$4), \
         (SELECT jsonb_build_object('revision',revision,'state',state,'pipeline',pipeline)::text FROM native_slices WHERE id=$5), \
         (SELECT jsonb_build_object('revision',revision,'outcome',outcome,'summary',summary,'evidence',evidence,'provenance',provenance)::text FROM slice_results WHERE id=$6)",
    )
    .bind(ids.program)
    .bind(ids.scope)
    .bind(ids.slice_set)
    .bind(ids.slice_snapshot)
    .bind(ids.slice)
    .bind(ids.result)
    .fetch_one(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok((0..6).map(|index| row.get(index)).collect())
}
