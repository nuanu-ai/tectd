use sqlx::postgres::{PgConnectOptions, PgPoolOptions};
use sqlx::{PgPool, Row};
use std::{fs, str::FromStr};
use tect_postgres::admin;
use uuid::Uuid;

const LEGACY_MIGRATIONS: &[(&str, &str)] = &[
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
];

fn quoted_database(name: &str) -> String {
    assert!(
        name.bytes()
            .all(|byte| byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'_')
    );
    format!("\"{name}\"")
}

async fn connect_database(admin_url: &str, database: &str) -> PgPool {
    let options = PgConnectOptions::from_str(admin_url)
        .unwrap()
        .database(database);
    PgPoolOptions::new()
        .max_connections(2)
        .connect_with(options)
        .await
        .unwrap()
}

#[tokio::test]
async fn schema_six_program_and_scope_candidate_survive_native_planning_upgrade() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_role =
        std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let base_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let base = PgPoolOptions::new()
        .max_connections(1)
        .connect_with(base_options.clone())
        .await
        .unwrap();
    let database = format!("tect_v6_upgrade_{}", Uuid::new_v4().simple());
    sqlx::query(&format!("CREATE DATABASE {}", quoted_database(&database)))
        .execute(&base)
        .await
        .unwrap();

    let result = run_upgrade(&admin_url, &database, &runtime_role).await;

    let cleanup = sqlx::query(&format!(
        "DROP DATABASE {} WITH (FORCE)",
        quoted_database(&database)
    ))
    .execute(&base)
    .await;
    cleanup.expect("drop exact test-owned upgrade database");
    result.unwrap();
}

async fn run_upgrade(admin_url: &str, database: &str, runtime_role: &str) -> Result<(), String> {
    let pool = connect_database(admin_url, database).await;
    let migration_dir = tempfile::tempdir().map_err(|error| error.to_string())?;
    for (name, body) in LEGACY_MIGRATIONS {
        fs::write(migration_dir.path().join(name), body).map_err(|error| error.to_string())?;
    }
    sqlx::migrate::Migrator::new(migration_dir.path())
        .await
        .map_err(|error| error.to_string())?
        .run(&pool)
        .await
        .map_err(|error| error.to_string())?;

    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate_set = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    sqlx::query("INSERT INTO tenants(id) VALUES($1)")
        .bind(tenant)
        .execute(&pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("legacy-{workspace}"))
        .execute(&pool)
        .await
        .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,\
         boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) \
         VALUES($1,$2,$3,'open',4,'Legacy program','Preserve intent','Legacy evidence',\
         'One bounded Scope','No adjacent work','Existing behavior survives','ready',2,2,4096)",
    )
    .bind(program)
    .bind(tenant)
    .bind(workspace)
    .execute(&pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,\
         origin_input,origin_payload,origin_result,revision,status,boundary,input_cursor,latest_input,max_input_bytes) \
         VALUES($1,$2,$3,$4,$5,'legacy planning input','{}'::jsonb,'{}'::jsonb,2,'review_required','finite',1,1,4096)",
    )
    .bind(candidate_set)
    .bind(tenant)
    .bind(workspace)
    .bind(program)
    .bind(Uuid::new_v4())
    .execute(&pool)
    .await
    .map_err(|error| error.to_string())?;
    let program_digest = "a".repeat(64);
    sqlx::query(
        "INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) \
         VALUES($1,$2,$3,'legacy program body')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(&program_digest)
    .execute(&pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query(
        "INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,\
         program_revision,program_latest_input,planning_latest_input,program_body_digest,\
         selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,\
         method_body,method_origin_refs,registry_revision,registry_digest,rules) \
         VALUES($1,$2,$3,$4,1,4,2,1,$5,'{}'::uuid[],$6,'legacy-method','1',$7,\
         'legacy method body','[]'::jsonb,'2',$8,'[]'::jsonb)",
    )
    .bind(snapshot)
    .bind(tenant)
    .bind(workspace)
    .bind(candidate_set)
    .bind(&program_digest)
    .bind("b".repeat(64))
    .bind("c".repeat(64))
    .bind("d".repeat(64))
    .execute(&pool)
    .await
    .map_err(|error| error.to_string())?;
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$2 WHERE id=$1")
        .bind(candidate_set)
        .bind(snapshot)
        .execute(&pool)
        .await
        .map_err(|error| error.to_string())?;
    let legacy_draft = serde_json::json!({
        "boundary":"finite","goals":[],"evidence":[],
        "candidates":[{"id":Uuid::new_v4(),"revision":1,"title":"Legacy candidate"}],
        "blockers":[],"protected_changes":[]
    });
    sqlx::query(
        "INSERT INTO scope_candidate_drafts(tenant_id,workspace_id,candidate_set_id,set_revision,payload) \
         VALUES($1,$2,$3,2,$4)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(candidate_set)
    .bind(&legacy_draft)
    .execute(&pool)
    .await
    .map_err(|error| error.to_string())?;

    let before = legacy_rows(&pool, program, candidate_set).await?;
    admin::migrate(&pool, runtime_role)
        .await
        .map_err(|error| error.to_string())?;
    let after = legacy_rows(&pool, program, candidate_set).await?;
    if before != after {
        return Err("legacy Program or ScopeCandidate rows changed during upgrade".into());
    }
    let migration_count: i64 = sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .map_err(|error| error.to_string())?;
    let native_table: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.native_scopes')::text")
            .fetch_one(&pool)
            .await
            .map_err(|error| error.to_string())?;
    if migration_count != 7 || native_table.as_deref() != Some("native_scopes") {
        return Err("schema 7 was not installed after preserving legacy rows".into());
    }
    pool.close().await;
    Ok(())
}

async fn legacy_rows(
    pool: &PgPool,
    program: Uuid,
    candidate_set: Uuid,
) -> Result<(String, String, String, String), String> {
    let row = sqlx::query(
        "SELECT row_to_json(p)::text, row_to_json(s)::text, row_to_json(n)::text, d.payload::text \
         FROM programs p JOIN scope_candidate_sets s ON s.program_id=p.id \
         JOIN scope_candidate_snapshots n ON n.candidate_set_id=s.id \
         JOIN scope_candidate_drafts d ON d.candidate_set_id=s.id \
         WHERE p.id=$1 AND s.id=$2 AND n.id=s.current_snapshot_id AND d.set_revision=2",
    )
    .bind(program)
    .bind(candidate_set)
    .fetch_one(pool)
    .await
    .map_err(|error| error.to_string())?;
    Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
}
