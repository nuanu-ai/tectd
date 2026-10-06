use super::*;
use sqlx::{ConnectOptions, Connection, PgConnection, Row, postgres::PgConnectOptions};
use std::str::FromStr;

pub(super) async fn fixture() -> PgPool {
    let pool = connect_admin(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, "180006");
    MIGRATOR.run(&pool).await.unwrap();
    assert_eq!(ledger(&pool).await, expected_ledger_count());
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let mut runtime = PgConnection::connect_with(&options(&role)).await.unwrap();
    identity(&mut runtime, &role).await;
    let admin: String = sqlx::query_scalar("SELECT current_user::text")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_ne!(admin, role);
    pool
}

pub(super) fn options(role: &str) -> PgConnectOptions {
    PgConnectOptions::from_str(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .unwrap()
        .username(role)
}

pub(super) async fn role(pool: &PgPool) -> String {
    let role = format!("boot_{}", Uuid::new_v4().simple());
    let quoted = quote_identifier(&role).unwrap();
    sqlx::query(&format!("CREATE ROLE {quoted} LOGIN NOSUPERUSER NOBYPASSRLS NOCREATEDB NOCREATEROLE NOREPLICATION NOINHERIT"))
        .execute(pool).await.unwrap();
    let mut connection = PgConnection::connect_with(&options(&role)).await.unwrap();
    identity(&mut connection, &role).await;
    role
}

async fn identity(connection: &mut PgConnection, role: &str) {
    let row = sqlx::query(
        "SELECT current_user::text AS current,session_user::text AS session,r.oid::bigint AS oid,
         r.rolsuper,r.rolbypassrls,r.rolcreatedb,r.rolcreaterole,r.rolreplication,r.rolinherit,r.rolcanlogin,
         (SELECT count(*) FROM pg_catalog.pg_auth_members WHERE member=r.oid) AS memberships,
         EXISTS(SELECT 1 FROM pg_catalog.pg_database d WHERE d.datname=current_database()
                AND pg_catalog.pg_has_role(r.oid,d.datdba,'MEMBER')) AS database_owner,
         EXISTS(SELECT 1 FROM pg_catalog.pg_namespace n WHERE n.nspname='public'
                AND pg_catalog.pg_has_role(r.oid,n.nspowner,'MEMBER')) AS schema_owner
         FROM pg_catalog.pg_roles r WHERE r.rolname=current_user"
    ).fetch_one(connection).await.unwrap();
    assert_eq!(row.get::<String, _>("current"), role);
    assert_eq!(row.get::<String, _>("session"), role);
    for flag in [
        "rolsuper",
        "rolbypassrls",
        "rolcreatedb",
        "rolcreaterole",
        "rolreplication",
        "rolinherit",
        "database_owner",
        "schema_owner",
    ] {
        assert!(!row.get::<bool, _>(flag), "{flag}");
    }
    assert!(row.get::<bool, _>("rolcanlogin"));
    assert_eq!(row.get::<i64, _>("memberships"), 0);
    println!(
        "bootstrap role_oid={} strict_identity=true",
        row.get::<i64, _>("oid")
    );
}

pub(super) fn expected_ledger_count() -> i64 {
    i64::try_from(
        MIGRATOR
            .iter()
            .filter(|migration| migration.migration_type.is_up_migration())
            .count(),
    )
    .expect("successful up-migration ledger count fits i64")
}

pub(super) async fn ledger(pool: &PgPool) -> i64 {
    sqlx::query_scalar("SELECT count(*) FROM _sqlx_migrations WHERE success")
        .fetch_one(pool)
        .await
        .unwrap()
}

pub(super) async fn privileges(connection: &mut PgConnection, role: &str) -> Vec<bool> {
    sqlx::query_scalar(
        "SELECT ARRAY[
         has_table_privilege($1,'public.matrix_planning_selection_links','SELECT'),
         has_table_privilege($1,'public.matrix_planning_selection_links','INSERT'),
         has_table_privilege($1,'public.matrix_planning_effect_attestations','SELECT'),
         has_table_privilege($1,'public.matrix_planning_effect_attestations','INSERT'),
         has_function_privilege($1,'public.matrix_planning_lock_verification(uuid,uuid,uuid)','EXECUTE')]"
    ).bind(role).fetch_one(connection).await.unwrap()
}

pub(super) async fn acl(connection: &mut PgConnection) -> String {
    sqlx::query_scalar(
        "SELECT jsonb_build_object(
         'tables',(SELECT jsonb_agg(jsonb_build_array(c.oid::text,c.relacl::text) ORDER BY c.oid)
          FROM pg_class c WHERE c.oid IN ('public.matrix_planning_selection_links'::regclass,
                                         'public.matrix_planning_effect_attestations'::regclass)),
         'columns',(SELECT jsonb_agg(jsonb_build_array(a.attrelid::text,a.attnum,a.attacl::text)
                          ORDER BY a.attrelid,a.attnum) FROM pg_attribute a
          WHERE a.attrelid IN ('public.matrix_planning_selection_links'::regclass,
                              'public.matrix_planning_effect_attestations'::regclass)
          AND a.attnum>0 AND NOT a.attisdropped),
         'functions',(SELECT jsonb_agg(jsonb_build_array(p.oid::text,p.proacl::text) ORDER BY p.oid)
          FROM pg_proc p JOIN pg_namespace n ON n.oid=p.pronamespace WHERE n.nspname='public'
          AND p.proname IN ('matrix_planning_selection_require_active_owner',
          'matrix_planning_selection_require_mapped_nodes','matrix_planning_effect_require_active_verifier',
          'matrix_planning_lock_context','matrix_planning_selection_require_context',
          'matrix_planning_effect_require_context','matrix_verification_bindings_require_unconsumed_v2',
          'matrix_planning_lock_verification')),
         'trigger',(SELECT tgenabled::text FROM pg_trigger WHERE
          tgrelid='public.matrix_planning_selection_links'::regclass
          AND tgname='matrix_planning_selection_context_guard'))::text"
    ).fetch_one(connection).await.unwrap()
}

pub(super) async fn connected(role: &str) -> crate::PgStore {
    crate::PgStore::connect(options(role).to_url_lossy().as_str(), 2)
        .await
        .unwrap()
}
