use super::*;

/// A pristine database may contain only PostgreSQL's built-in schemas and
/// PL/pgSQL. Tables alone miss views, functions, types and installed extensions.
const USER_CATALOG_RESIDUE_SQL: &str = "SELECT \
     (SELECT count(*) FROM pg_namespace WHERE nspname NOT IN \
       ('pg_catalog','pg_toast','public','information_schema')) + \
     (SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_proc WHERE pronamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_type WHERE typnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_operator WHERE oprnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_opclass WHERE opcnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_opfamily WHERE opfnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_collation WHERE collnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_conversion WHERE connamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_ts_config WHERE cfgnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_ts_dict WHERE dictnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_ts_parser WHERE prsnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_ts_template WHERE tmplnamespace='public'::regnamespace) + \
     (SELECT count(*) FROM pg_extension WHERE extname <> 'plpgsql') + \
     (SELECT count(*) FROM pg_event_trigger) + \
     (SELECT count(*) FROM pg_publication) + \
     (SELECT count(*) FROM pg_subscription) + \
     (SELECT count(*) FROM pg_foreign_data_wrapper) + \
     (SELECT count(*) FROM pg_foreign_server)";

async fn user_catalog_residue(pool: &PgPool) -> i64 {
    sqlx::query_scalar(USER_CATALOG_RESIDUE_SQL)
        .fetch_one(pool)
        .await
        .expect("cannot inspect PostgreSQL user catalog")
}

#[tokio::test]
#[ignore = "explicit TECT_TEST_GUARD_NEGATIVE_URL on a fresh disposable PostgreSQL 18 database"]
async fn rejects_schema_view_function_and_type_before_migration() {
    let url = std::env::var("TECT_TEST_GUARD_NEGATIVE_URL")
        .expect("dedicated negative-guard URL required");
    let parsed = Url::parse(&url).unwrap();
    assert_eq!(parsed.host_str(), Some("127.0.0.1"));
    assert_eq!(parsed.username(), "postgres");
    let pool = PgPool::connect_with(PgConnectOptions::from_str(&url).unwrap())
        .await
        .unwrap();
    let identity: (i32, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(identity.0 / 10_000, 18);
    assert_eq!(
        identity.1.to_string(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID").unwrap()
    );
    assert_eq!(
        identity.2,
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap()
    );
    assert_eq!(user_catalog_residue(&pool).await, 0);
    let mut statements = vec![
        "CREATE SCHEMA guard_negative",
        "CREATE VIEW public.guard_negative_view AS SELECT 1 AS value",
        "CREATE FUNCTION public.guard_negative_fn() RETURNS integer LANGUAGE sql AS 'SELECT 1'",
        "CREATE TYPE public.guard_negative_type AS ENUM ('one')",
    ];
    let pgcrypto_available: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM pg_available_extensions WHERE name='pgcrypto')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    if pgcrypto_available {
        statements.push("CREATE EXTENSION pgcrypto");
    }
    for statement in statements {
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(statement).execute(&mut *tx).await.unwrap();
        let residue: i64 = sqlx::query_scalar(USER_CATALOG_RESIDUE_SQL)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(
            residue > 0,
            "pre-migration catalog guard missed a user object"
        );
        tx.rollback().await.unwrap();
        assert_eq!(user_catalog_residue(&pool).await, 0);
    }
}

pub(super) async fn fresh_database() -> (PgPool, String) {
    let isolated_root =
        std::fs::canonicalize(std::env::var("TECT_TEST_ISOLATED_ROOT").unwrap()).unwrap();
    let codex_home = std::fs::canonicalize(std::env::var("CODEX_HOME").unwrap()).unwrap();
    assert_eq!(codex_home, isolated_root.join("codex-home"));
    assert_ne!(
        codex_home,
        std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".codex")
    );
    let expected_data = std::fs::canonicalize(isolated_root.join("pgdata")).unwrap();
    let test_exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let build_dir = test_exe.parent().unwrap().parent().unwrap();
    let mcp_exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_tectd-mcp")).unwrap();
    assert_eq!(mcp_exe, build_dir.join("tectd-mcp"));
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let expected_system = std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID")
        .expect("explicit disposable PostgreSQL system ID required");
    let expected_oid: i64 = std::env::var("TECT_TEST_EXPECTED_DB_OID")
        .expect("explicit disposable database OID required")
        .parse()
        .expect("database OID must be numeric");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("explicit runtime role required");
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("admin URL required");
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").expect("runtime URL required");
    let admin = Url::parse(&admin_url).expect("valid admin URL required");
    let runtime = Url::parse(&runtime_url).expect("valid runtime URL required");
    for url in [&admin, &runtime] {
        assert!(matches!(url.scheme(), "postgres" | "postgresql"));
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert!(url.port().is_some() && url.port() == admin.port());
        assert_eq!(url.path(), admin.path());
        assert!(url.query().is_none() && url.fragment().is_none());
    }
    assert_eq!(admin.username(), "postgres");
    assert_eq!(runtime.username(), role);
    assert_ne!(role, "postgres");
    let pool = PgPool::connect_with(PgConnectOptions::from_str(&admin_url).unwrap())
        .await
        .expect("isolated admin connection failed");
    let identity: (i32, String, String, i64, String, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),current_setting('data_directory')",
    )
    .fetch_one(&pool)
    .await
    .expect("admin identity query failed");
    assert_eq!(identity.0 / 10_000, 18);
    assert_eq!(identity.1, admin.path().trim_start_matches('/'));
    assert_eq!(identity.2, "postgres");
    assert_eq!(identity.3, expected_oid);
    assert_eq!(identity.4, expected_system);
    assert_eq!(std::fs::canonicalize(&identity.5).unwrap(), expected_data);
    assert_eq!(
        user_catalog_residue(&pool).await,
        0,
        "database must have no user schema or objects before migrations"
    );
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
        .await
        .expect("isolated runtime connection failed");
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    ).fetch_one(&runtime_pool).await.unwrap();
    assert_eq!(
        runtime_identity,
        (identity.1.clone(), role.clone(), expected_oid)
    );
    drop(runtime_pool);
    admin::migrate(&pool, &role)
        .await
        .expect("isolated migrations failed");
    let ledger: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ledger.len(), 122);
    for (index, (version, success, _)) in ledger.iter().enumerate() {
        assert_eq!(*version, index as i64 + 1);
        assert!(*success);
    }
    for (version, bytes) in [
        (
            105,
            include_bytes!(
                "../../../../../postgres/migrations/0105_matrix_declared_requirements_context.sql"
            )
            .as_slice(),
        ),
        (
            106,
            include_bytes!(
                "../../../../../postgres/migrations/0106_matrix_task_requirements_binding.sql"
            )
            .as_slice(),
        ),
        (
            107,
            include_bytes!(
                "../../../../../postgres/migrations/0107_context_matrix_verification.sql"
            )
            .as_slice(),
        ),
        (
            108,
            include_bytes!(
                "../../../../../postgres/migrations/0108_matrix_v1_dispatch_cutover_allowlist.sql"
            )
            .as_slice(),
        ),
        (
            109,
            include_bytes!(
                "../../../../../postgres/migrations/0109_matrix_planning_context_selection.sql"
            )
            .as_slice(),
        ),
        (
            110,
            include_bytes!(
                "../../../../../postgres/migrations/0110_pipeline_context_matrix_authority.sql"
            )
            .as_slice(),
        ),
        (
            111,
            include_bytes!(
                "../../../../../postgres/migrations/0111_session_advisory_preference.sql"
            )
            .as_slice(),
        ),
    ] {
        assert_eq!(
            ledger[(version - 1) as usize].2,
            Sha384::digest(bytes).to_vec()
        );
    }
    (pool, runtime_url)
}
