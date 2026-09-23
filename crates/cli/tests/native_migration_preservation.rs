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
    (
        "0007_native_scope_slice_planning.sql",
        include_str!("../../postgres/migrations/0007_native_scope_slice_planning.sql"),
    ),
    (
        "0008_native_slice_pipeline_execution.sql",
        include_str!("../../postgres/migrations/0008_native_slice_pipeline_execution.sql"),
    ),
    (
        "0009_durable_knowledge.sql",
        include_str!("../../postgres/migrations/0009_durable_knowledge.sql"),
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
async fn schema_nine_metadata_survives_dk2_upgrade() {
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
    let principal = Uuid::new_v4();
    let host = Uuid::new_v4();
    let session = Uuid::new_v4();
    sqlx::query("INSERT INTO principals(id,tenant_id,role) VALUES($1,$2,'owner')")
        .bind(principal)
        .bind(tenant)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(principal)
        .execute(&pool)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query(
        "INSERT INTO hosts(id,tenant_id,principal_id,credential_digest) VALUES($1,$2,$3,$4)",
    )
    .bind(host)
    .bind(tenant)
    .bind(principal)
    .bind("e".repeat(64))
    .execute(&pool)
    .await
    .map_err(|e| e.to_string())?;
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(session).bind(tenant).bind(host).bind(workspace).bind(Uuid::new_v4().to_string()).execute(&pool).await.map_err(|e|e.to_string())?;
    let unit = Uuid::new_v4();
    let change = Uuid::new_v4();
    let event = Uuid::new_v4();
    let receipt = Uuid::new_v4();
    sqlx::query("INSERT INTO knowledge_changes(id,tenant_id,workspace_id,unit_id,operation,stage,expected_generation,proposed_unit_revision,proposal_digest,semantic_diff,preparation_method,review_method,reason,authority_basis,prepared_principal_id,prepared_session_id) VALUES($1,$2,$3,$4,'retract','committed',0,1,$5,'legacy retract','{}','{}','legacy reason','legacy authority',$6,$7)")
        .bind(change).bind(tenant).bind(workspace).bind(unit).bind("f".repeat(64)).bind(principal).bind(session).execute(&pool).await.map_err(|e|e.to_string())?;
    sqlx::query("INSERT INTO knowledge_publication_events(id,tenant_id,workspace_id,unit_id,unit_revision,change_id,operation,actor_principal_id,actor_session_id,rdf_digest,rdf_digest_method,rdf_digest_scope,unit_iri,revision_iri,event_iri) VALUES($1,$2,$3,$4,1,$5,'retract',$6,$7,$8,'rdfc-1.0-sha256','lifecycle_event_payload',$9,$10,$11)")
        .bind(event).bind(tenant).bind(workspace).bind(unit).bind(change).bind(principal).bind(session).bind("a".repeat(64)).bind(format!("urn:unit:{unit}")).bind(format!("urn:revision:{unit}:1")).bind(format!("urn:event:{event}")).execute(&pool).await.map_err(|e|e.to_string())?;
    sqlx::query("INSERT INTO knowledge_revisions(tenant_id,workspace_id,unit_id,revision,constraint_payload,source_sha256,rdf_digest,rdf_digest_method,rdf_digest_scope,publication_event_id,unit_iri,revision_iri,source_iri,publication_event_iri) VALUES($1,$2,$3,1,'{}',$4,$5,'rdfc-1.0-sha256','revision_publication_payload',$6,$7,$8,$9,$10)")
        .bind(tenant).bind(workspace).bind(unit).bind("b".repeat(64)).bind("c".repeat(64)).bind(event).bind(format!("urn:unit:{unit}")).bind(format!("urn:revision:{unit}:1")).bind(format!("urn:source:{unit}")).bind(format!("urn:event:{event}")).execute(&pool).await.map_err(|e|e.to_string())?;
    sqlx::query("INSERT INTO knowledge_unit_heads(tenant_id,workspace_id,unit_id,accepted_revision,active,proposal_fingerprint,last_event_id) VALUES($1,$2,$3,1,false,'legacy-head',$4)")
        .bind(tenant).bind(workspace).bind(unit).bind(event).execute(&pool).await.map_err(|e|e.to_string())?;
    sqlx::query("INSERT INTO knowledge_command_receipts(tenant_id,workspace_id,operation,request_id,actor_session_id,request_payload,result_payload) VALUES($1,$2,'publish',$3,$4,$5,$6)")
        .bind(tenant).bind(workspace).bind(receipt).bind(session).bind(serde_json::json!({"change_id":change})).bind(serde_json::json!({"published":{"change_id":change}})).execute(&pool).await.map_err(|e|e.to_string())?;

    let before = legacy_rows(&pool, program, candidate_set).await?;
    let dk1_before = dk1_rows(&pool, change, event, unit, receipt).await?;
    admin::migrate(&pool, runtime_role)
        .await
        .map_err(|error| error.to_string())?;
    let after = legacy_rows(&pool, program, candidate_set).await?;
    let dk1_after = dk1_rows(&pool, change, event, unit, receipt).await?;
    if before != after {
        return Err("legacy Program or ScopeCandidate rows changed during upgrade".into());
    }
    let program_payload_erased: Option<bool> =
        sqlx::query_scalar("SELECT payload_erased FROM programs WHERE id=$1")
            .bind(program)
            .fetch_optional(&pool)
            .await
            .map_err(|error| error.to_string())?;
    if program_payload_erased != Some(false) {
        return Err("migrated legacy Program payload_erased was not exact false".into());
    }
    if dk1_before != dk1_after {
        return Err(
            "populated DK-1 retract/event/revision/receipt bytes changed during upgrade".into(),
        );
    }
    let embedded_migrations = sqlx::migrate!("../postgres/migrations");
    let expected_count =
        i64::try_from(embedded_migrations.iter().count()).map_err(|error| error.to_string())?;
    let expected_latest = embedded_migrations
        .iter()
        .map(|migration| migration.version)
        .max()
        .ok_or("embedded migration set is empty")?;
    let (migration_count, latest_version): (i64, Option<i64>) =
        sqlx::query_as("SELECT count(*), max(version) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .map_err(|error| error.to_string())?;
    let native_table: Option<String> =
        sqlx::query_scalar("SELECT to_regclass('public.native_scopes')::text")
            .fetch_one(&pool)
            .await
            .map_err(|error| error.to_string())?;
    if migration_count != expected_count
        || latest_version != Some(expected_latest)
        || native_table.as_deref() != Some("native_scopes")
    {
        return Err(
            "current schema was not installed after preserving legacy and DK-1 rows".into(),
        );
    }
    pool.close().await;
    Ok(())
}

async fn dk1_rows(
    pool: &PgPool,
    change: Uuid,
    event: Uuid,
    unit: Uuid,
    receipt: Uuid,
) -> Result<(String, String, String, String), String> {
    let row=sqlx::query("SELECT jsonb_build_object('operation',c.operation,'proposal_fingerprint',c.proposal_fingerprint,'source_sha256',c.source_sha256,'proposal',c.proposal)::text,jsonb_build_object('rdf_digest',e.rdf_digest,'revision_iri',e.revision_iri,'event_iri',e.event_iri)::text,jsonb_build_object('source_sha256',r.source_sha256,'rdf_digest',r.rdf_digest,'constraint_payload',r.constraint_payload)::text,jsonb_build_object('operation',x.operation,'request_payload',x.request_payload,'result_payload',x.result_payload)::text FROM knowledge_changes c JOIN knowledge_publication_events e ON e.id=$2 JOIN knowledge_revisions r ON r.unit_id=$3 AND r.revision=1 JOIN knowledge_command_receipts x ON x.request_id=$4 WHERE c.id=$1")
        .bind(change).bind(event).bind(unit).bind(receipt).fetch_one(pool).await.map_err(|e|e.to_string())?;
    Ok((row.get(0), row.get(1), row.get(2), row.get(3)))
}

async fn legacy_rows(
    pool: &PgPool,
    program: Uuid,
    candidate_set: Uuid,
) -> Result<(String, String, String, String), String> {
    let row = sqlx::query(
        "SELECT (to_jsonb(p)-'payload_erased')::text, row_to_json(s)::text, row_to_json(n)::text, d.payload::text \
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
