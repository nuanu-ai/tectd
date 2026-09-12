//! UTC monthly segmentation is derived by PostgreSQL and stays out of the MCP API.
mod recovery_support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::json;
use sqlx::{Connection, Executor, PgConnection, PgPool};
use tect_postgres::admin;
use uuid::Uuid;

const OLD_MIGRATIONS: [&str; 4] = [
    include_str!("../../postgres/migrations/0001_native_session_bootstrap.sql"),
    include_str!("../../postgres/migrations/0002_source_catalog_selection.sql"),
    include_str!("../../postgres/migrations/0003_program_formation.sql"),
    include_str!("../../postgres/migrations/0004_workspace_setup.sql"),
];
const EPOCH_MIGRATION: &str =
    include_str!("../../postgres/migrations/0005_monthly_epoch_segmentation.sql");
const TABLES: [&str; 11] = [
    "workspaces",
    "agent_sessions",
    "workspace_events",
    "source_repositories",
    "source_worktrees",
    "session_worktrees",
    "programs",
    "program_inputs",
    "setup_session_directories",
    "workspace_setups",
    "workspace_setup_inputs",
];

fn schema_sql(script: &str, schema: &str) -> String {
    script.replace("public.", &format!("{schema}."))
}

async fn versions(pool: &PgPool, tenant: Uuid) -> Vec<Vec<String>> {
    let mut rows = Vec::new();
    for table in TABLES {
        rows.push(
            sqlx::query_scalar::<_, String>(&format!(
                "SELECT xmin::text || ':' || row_to_json(t)::text FROM {table} t \
                 WHERE tenant_id=$1 ORDER BY row_to_json(t)::text"
            ))
            .bind(tenant)
            .fetch_all(pool)
            .await
            .unwrap(),
        );
    }
    rows
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
async fn migration_backfills_utc_months_and_preserves_birth_identity_and_rls() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    assert!(
        !role.is_empty()
            && role.len() <= 63
            && role
                .bytes()
                .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    );
    let schema = format!("epoch_{}", Uuid::new_v4().simple());
    let mut connection = PgConnection::connect(&admin_url).await.unwrap();
    connection
        .execute(format!("CREATE SCHEMA {schema}").as_str())
        .await
        .unwrap();
    connection
        .execute(format!("SET search_path TO {schema}, pg_catalog").as_str())
        .await
        .unwrap();
    for migration in OLD_MIGRATIONS {
        sqlx::raw_sql(&schema_sql(migration, &schema))
            .execute(&mut connection)
            .await
            .unwrap();
    }

    // These rows exist before the epoch migration. Offset spelling crosses the UTC boundary.
    sqlx::raw_sql(
        r#"
        INSERT INTO tenants (id) VALUES
            ('10000000-0000-4000-8000-000000000001'),
            ('10000000-0000-4000-8000-000000000002');
        INSERT INTO principals (id,tenant_id,role) VALUES
            ('20000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001','owner'),
            ('20000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000002','owner');
        INSERT INTO hosts
            (id,tenant_id,principal_id,credential_digest,allowed_source_roots,allowed_setup_roots)
        VALUES
            ('30000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
             '20000000-0000-4000-8000-000000000001',repeat('1',64),'[]','[]'),
            ('30000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000002',
             '20000000-0000-4000-8000-000000000002',repeat('2',64),'[]','[]');
        INSERT INTO workspaces (id,tenant_id,key,created_at) VALUES
            ('40000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
             'september-workspace','2026-10-01 13:30:00+14'),
            ('40000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000002',
             'foreign-october','2026-09-30 17:00:00-07');
        INSERT INTO memberships (tenant_id,workspace_id,principal_id) VALUES
            ('10000000-0000-4000-8000-000000000001','40000000-0000-4000-8000-000000000001',
             '20000000-0000-4000-8000-000000000001'),
            ('10000000-0000-4000-8000-000000000002','40000000-0000-4000-8000-000000000002',
             '20000000-0000-4000-8000-000000000002');
        INSERT INTO agent_sessions
            (id,tenant_id,host_id,workspace_id,native_session_id,created_at)
        VALUES ('50000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '30000000-0000-4000-8000-000000000001','40000000-0000-4000-8000-000000000001',
            '50000000-0000-4000-8000-000000000001','2026-10-01 13:30:00+14');
        INSERT INTO workspace_events (id,tenant_id,workspace_id,kind,entity_id,created_at) VALUES
            ('60000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
             '40000000-0000-4000-8000-000000000001','workspace_opened',
             '40000000-0000-4000-8000-000000000001','2026-10-01 13:30:00+14'),
            ('60000000-0000-4000-8000-000000000002','10000000-0000-4000-8000-000000000001',
             '40000000-0000-4000-8000-000000000001','session_opened',
             '50000000-0000-4000-8000-000000000001','2026-09-30 17:00:00-07');
        INSERT INTO source_repositories
            (id,tenant_id,workspace_id,host_id,common_dir,created_at)
        VALUES ('70000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '40000000-0000-4000-8000-000000000001','30000000-0000-4000-8000-000000000001',
            '/__tect_test__/epoch.git','2026-10-01 13:30:00+14');
        INSERT INTO source_worktrees
            (id,tenant_id,workspace_id,host_id,repository_id,path,created_at)
        VALUES ('71000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '40000000-0000-4000-8000-000000000001','30000000-0000-4000-8000-000000000001',
            '70000000-0000-4000-8000-000000000001','/__tect_test__/epoch','2026-10-01 13:30:00+14');
        INSERT INTO session_worktrees
            (tenant_id,workspace_id,host_id,session_id,worktree_id,created_at)
        VALUES ('10000000-0000-4000-8000-000000000001','40000000-0000-4000-8000-000000000001',
            '30000000-0000-4000-8000-000000000001','50000000-0000-4000-8000-000000000001',
            '71000000-0000-4000-8000-000000000001','2026-09-30 17:00:00-07');
        INSERT INTO programs
            (id,tenant_id,workspace_id,status,revision,current_step,input_cursor,latest_input,
             max_input_bytes,created_at)
        VALUES ('80000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '40000000-0000-4000-8000-000000000001','draft',1,'compose',0,1,5,
            '2026-10-01 13:30:00+14');
        INSERT INTO program_inputs
            (id,tenant_id,workspace_id,program_id,sequence,request_id,session_id,input,created_at)
        VALUES ('81000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '40000000-0000-4000-8000-000000000001','80000000-0000-4000-8000-000000000001',1,
            '82000000-0000-4000-8000-000000000001','50000000-0000-4000-8000-000000000001',
            'input','2026-09-30 17:00:00-07');
        INSERT INTO setup_session_directories
            (tenant_id,workspace_id,host_id,session_id,task_directory,device,inode,created_at)
        VALUES ('10000000-0000-4000-8000-000000000001','40000000-0000-4000-8000-000000000001',
            '30000000-0000-4000-8000-000000000001','50000000-0000-4000-8000-000000000001',
            '/__tect_test__/epoch',1,1,'2026-10-01 13:30:00+14');
        INSERT INTO workspace_setups
            (id,tenant_id,workspace_id,host_id,task_directory,device,inode,status,revision,
             current_step,input_cursor,latest_input,max_input_bytes,created_at,updated_at)
        VALUES ('90000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '40000000-0000-4000-8000-000000000001','30000000-0000-4000-8000-000000000001',
            '/__tect_test__/epoch',1,1,'draft',1,'compose',0,1,5,'2026-10-01 13:30:00+14',
            '2026-09-30 17:00:00-07');
        INSERT INTO workspace_setup_inputs
            (id,tenant_id,workspace_id,host_id,setup_id,sequence,request_id,session_id,input,created_at)
        VALUES ('91000000-0000-4000-8000-000000000001','10000000-0000-4000-8000-000000000001',
            '40000000-0000-4000-8000-000000000001','30000000-0000-4000-8000-000000000001',
            '90000000-0000-4000-8000-000000000001',1,'92000000-0000-4000-8000-000000000001',
            '50000000-0000-4000-8000-000000000001','input','2026-09-30 17:00:00-07');
        "#,
    )
    .execute(&mut connection)
    .await
    .unwrap();
    connection
        .execute("SET TIME ZONE 'Pacific/Kiritimati'")
        .await
        .unwrap();
    sqlx::raw_sql(&schema_sql(EPOCH_MIGRATION, &schema))
        .execute(&mut connection)
        .await
        .unwrap();

    let generated: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns \
         WHERE table_schema=$1 AND column_name='epoch_month' AND is_generated='ALWAYS' \
           AND is_nullable='NO' AND data_type='date'",
    )
    .bind(&schema)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(generated, 11);
    let excluded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM information_schema.columns WHERE table_schema=$1 \
         AND table_name IN ('tenants','principals','hosts','memberships') \
         AND column_name='epoch_month'",
    )
    .bind(&schema)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(excluded, 0);
    let indexes: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_catalog.pg_indexes WHERE schemaname=$1 \
         AND indexname LIKE '%_epoch_idx'",
    )
    .bind(&schema)
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(indexes, 11);

    let rows: Vec<(String, String)> = sqlx::query_as(
        "SELECT 'program'::text,epoch_month::text FROM programs \
         UNION ALL SELECT 'program_input',epoch_month::text FROM program_inputs \
         UNION ALL SELECT 'workspace',epoch_month::text FROM workspaces \
             WHERE tenant_id='10000000-0000-4000-8000-000000000001' \
         UNION ALL SELECT 'workspace_event_'||kind,epoch_month::text FROM workspace_events \
         ORDER BY 1",
    )
    .fetch_all(&mut connection)
    .await
    .unwrap();
    assert_eq!(
        rows,
        [
            ("program".into(), "2026-09-01".into()),
            ("program_input".into(), "2026-10-01".into()),
            ("workspace".into(), "2026-09-01".into()),
            ("workspace_event_session_opened".into(), "2026-10-01".into()),
            (
                "workspace_event_workspace_opened".into(),
                "2026-09-01".into()
            ),
        ]
    );
    connection
        .execute("SET TIME ZONE 'America/Los_Angeles'")
        .await
        .unwrap();
    let unchanged: Vec<String> =
        sqlx::query_scalar("SELECT epoch_month::text FROM workspace_events ORDER BY kind")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(unchanged, ["2026-10-01", "2026-09-01"]);

    let birth_change = connection
        .execute(
            "UPDATE programs SET created_at='2026-10-01 00:00:00+00' \
             WHERE id='80000000-0000-4000-8000-000000000001'",
        )
        .await
        .unwrap_err();
    assert_eq!(
        birth_change
            .as_database_error()
            .unwrap()
            .code()
            .map(|code| code.into_owned()),
        Some("23514".into())
    );
    connection
        .execute(
            "UPDATE programs SET name='long lived' \
             WHERE id='80000000-0000-4000-8000-000000000001'; \
             UPDATE workspace_setups SET updated_at='2026-11-01 00:00:00+00' \
             WHERE id='90000000-0000-4000-8000-000000000001'",
        )
        .await
        .unwrap();
    let birth_months: (String, String) = sqlx::query_as(
        "SELECT p.epoch_month::text,s.epoch_month::text FROM programs p,workspace_setups s \
         WHERE p.id='80000000-0000-4000-8000-000000000001' \
           AND s.id='90000000-0000-4000-8000-000000000001'",
    )
    .fetch_one(&mut connection)
    .await
    .unwrap();
    assert_eq!(birth_months, ("2026-09-01".into(), "2026-09-01".into()));

    connection
        .execute(format!("GRANT USAGE ON SCHEMA {schema} TO \"{role}\"").as_str())
        .await
        .unwrap();
    connection
        .execute(format!("GRANT SELECT ON ALL TABLES IN SCHEMA {schema} TO \"{role}\"").as_str())
        .await
        .unwrap();
    connection
        .execute(format!("SET ROLE \"{role}\"").as_str())
        .await
        .unwrap();
    connection
        .execute(
            "SELECT pg_catalog.set_config(
                'tect.tenant_id','10000000-0000-4000-8000-000000000001',false)",
        )
        .await
        .unwrap();
    let visible: Vec<(String, String)> =
        sqlx::query_as("SELECT key,epoch_month::text FROM workspaces")
            .fetch_all(&mut connection)
            .await
            .unwrap();
    assert_eq!(
        visible,
        [("september-workspace".into(), "2026-09-01".into())]
    );
    connection.execute("RESET ROLE").await.unwrap();
    connection
        .execute(format!("DROP SCHEMA {schema} CASCADE").as_str())
        .await
        .unwrap();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn backend_replay_keeps_epoch_and_get_state_is_db_only_and_read_only() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("epoch.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-epoch-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let native = Uuid::new_v4().to_string();
    let workspace_key = format!("epoch-{}", Uuid::new_v4().simple());
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;
    let opened = client.call("open_workspace", json!({})).await;
    assert!(!opened.to_string().contains("epoch_month"));
    let workspace_id: Uuid = opened["workspace"]["id"].as_str().unwrap().parse().unwrap();
    let session_id: Uuid = opened["session"]["id"].as_str().unwrap().parse().unwrap();
    let historical_program_id = Uuid::new_v4();
    let request_id = Uuid::new_v4();
    let input = "cross-month epoch replay";
    sqlx::query(
        "INSERT INTO programs \
             (id,tenant_id,workspace_id,status,revision,current_step,input_cursor,latest_input,\
              max_input_bytes,created_at) \
         VALUES ($1,$2,$3,'draft',1,'compose',0,1,$4,'2000-01-31 23:59:59-05')",
    )
    .bind(historical_program_id)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(input.len() as i64)
    .execute(&pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO program_inputs \
             (tenant_id,workspace_id,program_id,sequence,request_id,session_id,input,created_at) \
         VALUES ($1,$2,$3,1,$4,$5,$6,'2000-01-31 23:59:59-05')",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(historical_program_id)
    .bind(request_id)
    .bind(session_id)
    .bind(input)
    .execute(&pool)
    .await
    .unwrap();
    let before_replay = versions(&pool, enrollment.tenant_id).await;
    let replay = client
        .call(
            "begin_program",
            json!({"request_id":request_id,"input":input}),
        )
        .await;
    assert_eq!(replay["program"]["id"], historical_program_id.to_string());
    assert_eq!(versions(&pool, enrollment.tenant_id).await, before_replay);
    let canonical: (i64, String, String, String) = sqlx::query_as(
        "SELECT count(i.id),p.epoch_month::text,min(i.epoch_month)::text,\
                date_trunc('month',clock_timestamp() AT TIME ZONE 'UTC')::date::text \
         FROM programs p JOIN program_inputs i ON i.program_id=p.id WHERE p.id=$1 \
         GROUP BY p.epoch_month",
    )
    .bind(historical_program_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(canonical.0, 1);
    assert_eq!(canonical.1, canonical.2);
    assert_eq!(canonical.1, "2000-02-01");
    assert_ne!(canonical.1, canonical.3, "fixture must cross a UTC month");

    let before = versions(&pool, enrollment.tenant_id).await;
    let state = client.call("get_state", json!({})).await;
    assert!(!state.to_string().contains("epoch_month"));
    assert_eq!(versions(&pool, enrollment.tenant_id).await, before);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
