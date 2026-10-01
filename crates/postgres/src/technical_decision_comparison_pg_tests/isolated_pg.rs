//! Test-only opt-in for a private, socket-only PG18.6 synthetic control cluster.
//! No historical database is a default. All pins are required before connecting.
use serde::Deserialize;
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    os::unix::fs::{MetadataExt, PermissionsExt},
    path::Path,
    str::FromStr,
};

const OPT_IN: &str = "jev-owned-pg18.6-synthetic-control-only";

#[derive(Clone, Debug, Deserialize)]
#[serde(deny_unknown_fields)]
struct Identity {
    data_directory: String,
    socket_directory: String,
    port: u16,
    database: String,
    database_oid: i64,
    system_identifier: String,
    server_version_num: i32,
    admin_username: String,
    runtime_role: String,
}

fn endpoints(
    opt_in: &str,
    pins: &str,
    admin_url: &str,
    runtime_url: &str,
    role: &str,
) -> (Identity, PgConnectOptions, PgConnectOptions) {
    assert_eq!(opt_in, OPT_IN, "explicit synthetic control opt-in required");
    let identity: Identity =
        serde_json::from_str(pins).expect("complete exact cluster pins required");
    assert_eq!(identity.server_version_num, 180006);
    assert!(identity.database.starts_with("jev_") && identity.database.len() > 4);
    assert!(identity.database_oid > 0 && identity.port > 0);
    assert!(
        identity
            .system_identifier
            .parse::<u64>()
            .is_ok_and(|id| id > 0)
    );
    assert!(!identity.admin_username.is_empty());
    assert!(role == "tect_ci" || (role.starts_with("jev_") && role.len() > 4));
    assert_eq!(identity.runtime_role, role);
    assert_ne!(identity.admin_username, role);
    let data = Path::new(&identity.data_directory);
    let socket = Path::new(&identity.socket_directory);
    let parent = data.parent().expect("private cluster parent required");
    assert!(parent.starts_with("/private/tmp") || parent.starts_with("/tmp"));
    assert_eq!(data.file_name().and_then(|p| p.to_str()), Some("data"));
    assert_eq!(socket.parent(), Some(parent));
    assert_ne!(socket, data);
    assert!(
        parent
            .file_name()
            .and_then(|p| p.to_str())
            .is_some_and(|p| p.starts_with("jev-"))
    );
    let admin = PgConnectOptions::from_str(admin_url).expect("valid admin DSN");
    let runtime = PgConnectOptions::from_str(runtime_url).expect("valid runtime DSN");
    assert_eq!(admin.get_username(), identity.admin_username);
    assert_eq!(runtime.get_username(), role);
    for options in [&admin, &runtime] {
        assert_eq!(options.get_database(), Some(identity.database.as_str()));
        assert_eq!(
            options.get_socket().map(|path| path.as_path()),
            Some(socket),
            "TCP/default host prohibited"
        );
        assert_eq!(options.get_port(), identity.port);
    }
    (identity, admin, runtime)
}

async fn verify_identity(admin: &PgPool, runtime: &PgPool, expected: &Identity) {
    let observed: (
        String,
        i32,
        String,
        i64,
        String,
        String,
        String,
        String,
        i32,
    ) = sqlx::query_as(
        "SELECT current_setting('data_directory'),current_setting('server_version_num')::integer,\
         current_database(),(SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),current_user,\
         current_setting('listen_addresses'),current_setting('unix_socket_directories'),\
         current_setting('port')::integer",
    )
    .fetch_one(admin)
    .await
    .unwrap();
    assert_eq!(
        observed,
        (
            expected.data_directory.clone(),
            expected.server_version_num,
            expected.database.clone(),
            expected.database_oid,
            expected.system_identifier.clone(),
            expected.admin_username.clone(),
            String::new(),
            expected.socket_directory.clone(),
            i32::from(expected.port),
        ),
        "server identity/socket-only configuration must match all pins before migration"
    );
    let mut connection = runtime.acquire().await.unwrap();
    let observed: (i32, String, i64, String, i32) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),current_user,pg_backend_pid()",
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(
        (observed.0, observed.1, observed.2, observed.3),
        (
            expected.server_version_num,
            expected.database.clone(),
            expected.database_oid,
            expected.runtime_role.clone(),
        )
    );
    let backend: (String, String, bool) = sqlx::query_as(
        "SELECT datname,usename,client_addr IS NULL FROM pg_stat_activity WHERE pid=$1",
    )
    .bind(observed.4)
    .fetch_one(admin)
    .await
    .unwrap();
    assert_eq!(
        backend,
        (
            expected.database.clone(),
            expected.runtime_role.clone(),
            true
        )
    );
}

pub(crate) async fn connect_and_migrate() -> (PgPool, PgPool) {
    let required = |name| {
        std::env::var(name).unwrap_or_else(|_| panic!("{name} required; no default database"))
    };
    let (identity, admin_options, runtime_options) = endpoints(
        &required("TECT_TEST_CONTROL_PG_OPT_IN"),
        &required("TECT_TEST_CONTROL_PG_IDENTITY"),
        &required("TECT_TEST_ADMIN_URL"),
        &required("TECT_TEST_RUNTIME_URL"),
        &required("TECT_TEST_RUNTIME_ROLE"),
    );
    let data = Path::new(&identity.data_directory);
    let uid = std::process::Command::new("id").arg("-u").output().unwrap();
    assert!(
        uid.status.success(),
        "current OS ownership must be verifiable"
    );
    let uid: u32 = std::str::from_utf8(&uid.stdout)
        .unwrap()
        .trim()
        .parse()
        .unwrap();
    for path in [
        data.parent().unwrap(),
        data,
        Path::new(&identity.socket_directory),
    ] {
        assert_eq!(
            path.canonicalize()
                .expect("existing private cluster directory"),
            path
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().permissions().mode() & 0o777,
            0o700
        );
        assert_eq!(
            std::fs::metadata(path).unwrap().uid(),
            uid,
            "cluster must belong to current OS user"
        );
    }
    let admin = PgPool::connect_with(admin_options).await.unwrap();
    let runtime = PgPool::connect_with(runtime_options).await.unwrap();
    verify_identity(&admin, &runtime, &identity).await;
    // Includes NOSUPERUSER/NOBYPASSRLS, no ownership/member bypass or native access.
    crate::runtime::verify_runtime_role(&runtime).await.unwrap();
    crate::admin::migrate(&admin, &identity.runtime_role)
        .await
        .unwrap();
    crate::runtime::verify_runtime_role(&runtime).await.unwrap();
    let unforced: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relrowsecurity AND NOT c.relforcerowsecurity",
    )
    .fetch_one(&admin)
    .await
    .unwrap();
    assert_eq!(
        unforced, 0,
        "every RLS-protected fixture table must FORCE RLS"
    );
    let guarded: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pg_class c JOIN pg_namespace n ON n.oid=c.relnamespace \
         WHERE n.nspname='public' AND c.relname IN ('matrix_tasks','agent_sessions','slice_candidate_sets') \
         AND c.relrowsecurity AND c.relforcerowsecurity",
    ).fetch_one(&admin).await.unwrap();
    assert_eq!(
        guarded, 3,
        "fixture source tables require enabled FORCE RLS"
    );
    (admin, runtime)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    #[should_panic(expected = "explicit synthetic control opt-in required")]
    fn control_pg_rejects_missing_opt_in_before_dsn() {
        endpoints("", "{}", "", "", "");
    }

    #[test]
    #[should_panic(expected = "complete exact cluster pins required")]
    fn control_pg_rejects_partial_identity_before_dsn() {
        endpoints(OPT_IN, "{}", "", "", "");
    }

    #[test]
    #[should_panic(expected = "complete exact cluster pins required")]
    fn control_pg_rejects_unknown_identity_fields_before_dsn() {
        endpoints(OPT_IN, r#"{"accepted":true}"#, "", "", "");
    }

    fn pins() -> serde_json::Value {
        serde_json::json!({
            "data_directory":"/private/tmp/jev-fresh-control/data",
            "socket_directory":"/private/tmp/jev-fresh-control/socket",
            "port":59183,"database":"jev_fresh_control","database_oid":16385,
            "system_identifier":"7691549829209212292","server_version_num":180006,
            "admin_username":"postgres","runtime_role":"jev_runtime"
        })
    }
    const ADMIN: &str = "postgresql://postgres@localhost:59183/jev_fresh_control?host=/private/tmp/jev-fresh-control/socket";
    const RUNTIME: &str = "postgresql://jev_runtime@localhost:59183/jev_fresh_control?host=/private/tmp/jev-fresh-control/socket";

    #[test]
    fn control_pg_accepts_explicit_matching_socket_endpoints() {
        endpoints(OPT_IN, &pins().to_string(), ADMIN, RUNTIME, "jev_runtime");
    }

    #[test]
    fn control_pg_rejects_endpoint_mismatch_before_connection() {
        for (admin, runtime, role) in [
            (
                "postgresql://postgres@localhost:59183/jev_fresh_control",
                RUNTIME.to_string(),
                "jev_runtime",
            ),
            (
                ADMIN,
                RUNTIME.replace("?host=/private/tmp/jev-fresh-control/socket", ""),
                "jev_runtime",
            ),
            (ADMIN, RUNTIME.replace("59183", "59184"), "jev_runtime"),
            (
                ADMIN,
                RUNTIME.replace("jev_fresh_control?", "postgres?"),
                "jev_runtime",
            ),
            (
                ADMIN,
                RUNTIME.replace("control/socket", "control/other"),
                "jev_runtime",
            ),
            (
                ADMIN,
                RUNTIME.replace("jev_runtime@", "postgres@"),
                "jev_runtime",
            ),
            (ADMIN, RUNTIME.to_string(), "postgres"),
        ] {
            assert!(
                std::panic::catch_unwind(|| endpoints(
                    OPT_IN,
                    &pins().to_string(),
                    admin,
                    &runtime,
                    role
                ))
                .is_err()
            );
        }
    }

    #[test]
    fn control_pg_rejects_unsafe_identity_before_connection() {
        for (field, value) in [
            ("database", serde_json::json!("postgres")),
            ("server_version_num", serde_json::json!(180005)),
            ("database_oid", serde_json::json!(0)),
            ("system_identifier", serde_json::json!("0")),
            (
                "data_directory",
                serde_json::json!("/var/lib/postgresql/data"),
            ),
            ("socket_directory", serde_json::json!("/private/tmp/socket")),
            ("runtime_role", serde_json::json!("postgres")),
            ("admin_username", serde_json::json!("jev_runtime")),
        ] {
            let mut identity = pins();
            identity[field] = value;
            assert!(
                std::panic::catch_unwind(|| endpoints(
                    OPT_IN,
                    &identity.to_string(),
                    ADMIN,
                    RUNTIME,
                    "jev_runtime"
                ))
                .is_err()
            );
        }
    }
}
