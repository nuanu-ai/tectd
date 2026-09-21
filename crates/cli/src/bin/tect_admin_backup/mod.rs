mod artifact;

use artifact::{GraphRecord, LoadedBundle, Manifest};
use percent_encoding::percent_decode_str;
use std::ffi::OsStr;
use std::future::Future;
use std::os::unix::fs::PermissionsExt;
use std::path::{Component, Path, PathBuf};
use std::process::Stdio;
use std::time::Duration;
use tect_domain::{Error, Result};
use tokio::process::Command;
use url::Url;

const CHILD_TIMEOUT: Duration = Duration::from_secs(600);
const OPERATION_TIMEOUT: Duration = Duration::from_secs(900);

struct ChildAuth {
    url: Url,
    pool_url: Url,
    password: Option<String>,
}

impl ChildAuth {
    fn parse(value: &str) -> Result<Self> {
        let mut url = Url::parse(value).map_err(|_| Error::InvalidConfiguration)?;
        if !matches!(url.scheme(), "postgres" | "postgresql")
            || url.host_str().is_none()
            || url.username().is_empty()
            || url.fragment().is_some()
        {
            return Err(Error::InvalidConfiguration);
        }
        const FORBIDDEN_QUERY_KEYS: &[&str] = &[
            "dbname",
            "database",
            "host",
            "hostaddr",
            "port",
            "user",
            "username",
            "service",
            "servicefile",
            "password",
            "passfile",
            "sslpassword",
        ];
        if url.query_pairs().any(|(key, _)| {
            FORBIDDEN_QUERY_KEYS
                .iter()
                .any(|forbidden| key.eq_ignore_ascii_case(forbidden))
        }) {
            return Err(Error::InvalidConfiguration);
        }
        let pool_url = url.clone();
        let password = url
            .password()
            .map(|value| {
                percent_decode_str(value)
                    .decode_utf8()
                    .map(|value| value.into_owned())
                    .map_err(|_| Error::InvalidConfiguration)
            })
            .transpose()?;
        url.set_password(None)
            .map_err(|_| Error::InvalidConfiguration)?;
        if password.is_none()
            && let Some(passfile) = std::env::var_os("PGPASSFILE")
        {
            validate_passfile(Path::new(&passfile))?;
        }
        Ok(Self {
            url,
            pool_url,
            password,
        })
    }

    fn database_url(&self, database: &str) -> Result<String> {
        validate_identifier(database)?;
        let mut url = self.url.clone();
        url.set_path(&format!("/{database}"));
        Ok(url.into())
    }

    fn source_url(&self) -> String {
        self.url.clone().into()
    }

    fn pool_database_url(&self, database: &str) -> Result<String> {
        validate_identifier(database)?;
        let mut url = self.pool_url.clone();
        url.set_path(&format!("/{database}"));
        Ok(url.into())
    }
}

pub async fn backup(admin_url: &str, output: &Path, runtime_role: &str) -> Result<()> {
    backup_with_timeout(admin_url, output, runtime_role, OPERATION_TIMEOUT).await
}

async fn backup_with_timeout(
    admin_url: &str,
    output: &Path,
    runtime_role: &str,
    timeout: Duration,
) -> Result<()> {
    with_operation_timeout(timeout, backup_inner(admin_url, output, runtime_role)).await
}

async fn backup_inner(admin_url: &str, output: &Path, runtime_role: &str) -> Result<()> {
    validate_identifier(runtime_role)?;
    let auth = ChildAuth::parse(admin_url)?;
    require_tool_major("pg_dump", 18).await?;
    require_tool_major("pg_restore", 18).await?;
    artifact::create_bundle(output)?;
    let graph_dir = artifact::create_graph_directory(output)?;
    let pool = tect_postgres::admin::connect_admin(admin_url).await?;
    tect_postgres::admin::validate_runtime_role(&pool, runtime_role).await?;
    let mut snapshot = tect_postgres::admin::begin_backup_snapshot(&pool).await?;
    let identity = snapshot.identity().clone();
    let graphs = snapshot.export_graphs().await?;
    let mut graph_records = Vec::with_capacity(graphs.len());
    for (index, graph) in graphs.into_iter().enumerate() {
        artifact::validate_native_digest(&graph.native_digest)?;
        let file = format!("graph-{index}.nt");
        let path = graph_dir.join(&file);
        let record = artifact::write_private(&path, graph.payload.as_bytes())?;
        graph_records.push(GraphRecord {
            iri: graph.iri,
            native_digest: graph.native_digest,
            file: format!("graphs/{file}"),
            size: record.size,
            sha256: record.sha256,
        });
    }
    artifact::sync_graph_directory(&graph_dir)?;
    let dump = output.join("application.dump");
    run_child(
        "pg_dump",
        [
            OsStr::new("--no-password"),
            OsStr::new("--format=custom"),
            OsStr::new("--exclude-schema=pgrdf"),
            OsStr::new("--exclude-extension=pgrdf"),
            OsStr::new("--snapshot"),
            OsStr::new(snapshot.snapshot_id()),
            OsStr::new("--file"),
            dump.as_os_str(),
            OsStr::new("--dbname"),
            OsStr::new(&auth.source_url()),
        ],
        Some(&auth),
    )
    .await?;
    validate_toc(&dump, &auth).await?;
    let application_dump = artifact::seal_external_file(output, &dump)?;
    snapshot.finish().await?;
    pool.close().await;
    let manifest = Manifest {
        format: artifact::FORMAT.into(),
        version: artifact::VERSION,
        source_database: identity.source_database,
        runtime_role: runtime_role.into(),
        postgres_version_num: identity.postgres_version_num,
        pgrdf_version: identity.pgrdf_version,
        pgrdf_build_id: identity.pgrdf_build_id,
        schema_version: identity.schema_version,
        application_dump,
        graphs: graph_records,
    };
    artifact::publish_manifest(output, &manifest)
}

pub async fn restore(
    admin_url: &str,
    input: &Path,
    database: &str,
    runtime_role: &str,
) -> Result<()> {
    let result = with_operation_timeout(
        OPERATION_TIMEOUT,
        restore_inner(admin_url, input, database, runtime_role),
    )
    .await;
    if result.is_err() {
        eprintln!("restore failed for requested database {database}");
    }
    result
}

async fn with_operation_timeout<T>(
    timeout: Duration,
    operation: impl Future<Output = Result<T>>,
) -> Result<T> {
    tokio::time::timeout(timeout, operation)
        .await
        .map_err(|_| Error::StorageUnavailable)?
}

async fn restore_inner(
    admin_url: &str,
    input: &Path,
    database: &str,
    runtime_role: &str,
) -> Result<()> {
    validate_identifier(database)?;
    validate_identifier(runtime_role)?;
    let auth = ChildAuth::parse(admin_url)?;
    require_tool_major("pg_restore", 18).await?;
    let bundle = artifact::load_bundle(
        input,
        runtime_role,
        tect_postgres::admin::current_schema_version(),
    )?;
    validate_toc(&bundle.dump, &auth).await?;
    let maintenance_url = auth.pool_database_url("postgres")?;
    let maintenance = tect_postgres::admin::connect_admin(&maintenance_url).await?;
    tect_postgres::admin::validate_restore_preflight(&maintenance, runtime_role).await?;
    tect_postgres::admin::create_restore_database(&maintenance, database, runtime_role).await?;
    maintenance.close().await;
    restore_created(&auth, &bundle, database, runtime_role).await
}

async fn restore_created(
    auth: &ChildAuth,
    bundle: &LoadedBundle,
    database: &str,
    runtime_role: &str,
) -> Result<()> {
    let target_url = auth.database_url(database)?;
    run_child(
        "pg_restore",
        [
            OsStr::new("--no-password"),
            OsStr::new("--exit-on-error"),
            OsStr::new("--no-owner"),
            OsStr::new("--dbname"),
            OsStr::new(&target_url),
            bundle.dump.as_os_str(),
        ],
        Some(auth),
    )
    .await?;
    let target_pool_url = auth.pool_database_url(database)?;
    let target = tect_postgres::admin::connect_admin(&target_pool_url).await?;
    tect_postgres::admin::migrate(&target, runtime_role).await?;
    tect_postgres::enable_durable_knowledge(&target, runtime_role).await?;
    tect_postgres::admin::restore_graphs(&target, &bundle.graphs).await?;
    tect_postgres::admin::validate_restored_runtime_access(&target, runtime_role).await?;
    tect_postgres::admin::grant_database_connect(&target, database, runtime_role).await?;
    target.close().await;
    Ok(())
}

async fn validate_toc(dump: &Path, auth: &ChildAuth) -> Result<()> {
    let output = run_child(
        "pg_restore",
        [OsStr::new("--list"), dump.as_os_str()],
        Some(auth),
    )
    .await?;
    let toc = std::str::from_utf8(&output).map_err(|_| Error::InvalidConfiguration)?;
    if toc.contains("_pgrdf_") || toc.contains("TABLE DATA pgrdf ") || toc.contains("TABLE pgrdf ")
    {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

async fn require_tool_major(program: &str, major: u32) -> Result<()> {
    let output = run_child(program, [OsStr::new("--version")], None).await?;
    let version = std::str::from_utf8(&output).map_err(|_| Error::InvalidConfiguration)?;
    let expected = format!(" {major}.");
    if !version.contains(&expected) {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

async fn run_child<const N: usize>(
    program: &str,
    arguments: [&OsStr; N],
    auth: Option<&ChildAuth>,
) -> Result<Vec<u8>> {
    if !matches!(program, "pg_dump" | "pg_restore") {
        return Err(Error::InternalInvariant);
    }
    let mut command = Command::new(program);
    command
        .args(arguments)
        .stdin(Stdio::null())
        .stdout(Stdio::piped())
        .stderr(Stdio::piped())
        .kill_on_drop(true);
    if let Some(password) = auth.and_then(|value| value.password.as_ref()) {
        command.env("PGPASSWORD", password);
    }
    let child = command.spawn().map_err(|_| Error::InvalidConfiguration)?;
    let output = tokio::time::timeout(CHILD_TIMEOUT, child.wait_with_output())
        .await
        .map_err(|_| Error::StorageUnavailable)?
        .map_err(|_| Error::StorageUnavailable)?;
    if !output.status.success() {
        return Err(Error::StorageUnavailable);
    }
    Ok(output.stdout)
}

fn validate_identifier(value: &str) -> Result<()> {
    if value.is_empty()
        || value.len() > 63
        || (!value.as_bytes()[0].is_ascii_alphabetic() && value.as_bytes()[0] != b'_')
        || !value
            .bytes()
            .all(|byte| byte.is_ascii_alphanumeric() || byte == b'_')
    {
        return Err(Error::InvalidArguments);
    }
    Ok(())
}

fn validate_passfile(path: &Path) -> Result<()> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidConfiguration);
    }
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            _ => return Err(Error::InvalidConfiguration),
        }
        let metadata =
            std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if metadata.file_type().is_symlink() {
            return Err(Error::InvalidConfiguration);
        }
    }
    let metadata = std::fs::symlink_metadata(path).map_err(|_| Error::InvalidConfiguration)?;
    if !metadata.file_type().is_file() || metadata.permissions().mode() & 0o777 != 0o600 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reserved_uri_password_is_decoded_and_removed_from_child_url() {
        let auth = ChildAuth::parse(
            "postgresql://operator:p%40ss%2Fword@localhost:5432/tect?sslmode=require",
        )
        .unwrap();
        assert_eq!(auth.password.as_deref(), Some("p@ss/word"));
        let child = auth.source_url();
        assert!(!child.contains("p%40ss"));
        assert!(!child.contains("p@ss"));
        assert!(child.contains("sslmode=require"));
        let target = auth.database_url("restored_database").unwrap();
        let parsed = Url::parse(&target).unwrap();
        assert_eq!(parsed.path(), "/restored_database");
        assert_eq!(parsed.query(), Some("sslmode=require"));
    }

    #[test]
    fn target_authority_and_auth_query_overrides_are_rejected() {
        for key in [
            "dbname",
            "database",
            "host",
            "hostaddr",
            "port",
            "user",
            "username",
            "service",
            "servicefile",
            "password",
            "passfile",
            "sslpassword",
            "DbNaMe",
        ] {
            let value = format!("postgresql://operator@localhost/tect?{key}=override");
            assert!(ChildAuth::parse(&value).is_err(), "accepted {key}");
        }
        assert!(ChildAuth::parse("postgresql://operator@localhost/tect#fragment").is_err());
    }

    #[tokio::test]
    async fn cancelled_snapshot_wait_does_not_leave_the_publisher_lock() {
        let (Ok(admin_url), Ok(runtime_role)) = (
            std::env::var("TECT_TEST_ADMIN_URL"),
            std::env::var("TECT_TEST_RUNTIME_ROLE"),
        ) else {
            return;
        };
        let pool = tect_postgres::admin::connect_admin(&admin_url)
            .await
            .unwrap();
        tect_postgres::admin::validate_runtime_role(&pool, &runtime_role)
            .await
            .unwrap();
        let mut gate = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))")
            .execute(&mut *gate)
            .await
            .unwrap();
        let result = with_operation_timeout(Duration::from_millis(100), async {
            let snapshot = tect_postgres::admin::begin_backup_snapshot(&pool).await?;
            snapshot.finish().await
        })
        .await;
        assert_eq!(result, Err(Error::StorageUnavailable));
        gate.rollback().await.unwrap();

        let mut probe = pool.begin().await.unwrap();
        tokio::time::timeout(
            Duration::from_secs(2),
            sqlx::query("SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended('tect-dk-native-publisher',0))")
                .execute(&mut *probe),
        )
        .await
        .expect("cancelled backup snapshot left an advisory lock")
        .unwrap();
        probe.rollback().await.unwrap();
        pool.close().await;
    }
}
