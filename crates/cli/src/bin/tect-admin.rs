use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use tect_domain::{Error, HostAuth, Result, validate_setup_path};
use uuid::Uuid;

#[path = "../knowledge_recovery_cli.rs"]
mod knowledge_recovery_cli;
mod tect_admin_backup;

#[derive(Parser)]
#[command(name = "tect-admin")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Backup {
        #[arg(long)]
        out: PathBuf,
        #[arg(long)]
        runtime_role: String,
    },
    Restore {
        #[arg(long = "from")]
        from: PathBuf,
        #[arg(long)]
        database: String,
        #[arg(long)]
        runtime_role: String,
    },
    Migrate {
        #[arg(long)]
        runtime_role: String,
        #[arg(long)]
        enable_durable_knowledge: bool,
        #[arg(long)]
        enable_knowledge_vector_search: bool,
    },
    Enroll {
        #[arg(long)]
        tenant: Option<Uuid>,
        #[arg(long = "source-root")]
        source_roots: Vec<PathBuf>,
        #[arg(long = "setup-root")]
        setup_roots: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
    },
    EnrollVerifier {
        #[arg(long)]
        tenant: Uuid,
        #[arg(long)]
        workspace: Uuid,
        #[arg(long)]
        out: PathBuf,
    },
    EnsureTenant {
        #[arg(long)]
        tenant: Uuid,
    },
    RegisterHost {
        #[arg(long)]
        tenant: Uuid,
        #[arg(long)]
        auth_file: PathBuf,
        #[arg(long = "source-root")]
        source_roots: Vec<PathBuf>,
        #[arg(long = "setup-root")]
        setup_roots: Vec<PathBuf>,
    },
    GrantSetupRoot {
        #[arg(long)]
        host_id: String,
        #[arg(long)]
        setup_root: PathBuf,
    },
    RevokeHost {
        #[arg(long)]
        host_id: Uuid,
    },
    RevokeSession {
        #[arg(long)]
        session_id: Uuid,
    },
    KnowledgeSuppressionExport {
        #[arg(long)]
        out: PathBuf,
    },
    KnowledgeSuppressionApply {
        #[arg(long)]
        manifest: PathBuf,
        #[arg(long)]
        expected_lineage: Uuid,
        #[arg(long)]
        expected_sequence: i64,
        #[arg(long)]
        expected_digest: String,
    },
}

#[tokio::main]
async fn main() -> ExitCode {
    match run(Arguments::parse()).await {
        Ok(()) => ExitCode::SUCCESS,
        Err(error) => {
            eprintln!("error: {}", error.code());
            ExitCode::FAILURE
        }
    }
}

async fn run(arguments: Arguments) -> Result<()> {
    let admin_url =
        std::env::var("TECT_ADMIN_DATABASE_URL").map_err(|_| Error::InvalidConfiguration)?;
    match arguments.command {
        Command::Backup { out, runtime_role } => {
            tect_admin_backup::backup(&admin_url, &out, &runtime_role).await?;
            println!("backup complete at {}", out.display());
            Ok(())
        }
        Command::Restore {
            from,
            database,
            runtime_role,
        } => {
            tect_admin_backup::restore(&admin_url, &from, &database, &runtime_role).await?;
            println!("restore complete for database {database}");
            Ok(())
        }
        command => run_database_command(&admin_url, command).await,
    }
}

async fn run_database_command(admin_url: &str, command: Command) -> Result<()> {
    let pool = tect_postgres::admin::connect_admin(admin_url).await?;
    match command {
        Command::Backup { .. } | Command::Restore { .. } => return Err(Error::InternalInvariant),
        Command::Migrate {
            runtime_role,
            enable_durable_knowledge,
            enable_knowledge_vector_search,
        } => {
            tect_postgres::admin::migrate(&pool, &runtime_role).await?;
            if enable_durable_knowledge {
                tect_postgres::enable_durable_knowledge(&pool, &runtime_role).await?;
            }
            if enable_knowledge_vector_search {
                tect_postgres::enable_knowledge_vector_search(&pool, &runtime_role).await?;
            }
            println!("migration complete for runtime role {runtime_role}");
        }
        Command::Enroll {
            tenant,
            source_roots,
            setup_roots,
            out,
        } => {
            preflight_output(&out)?;
            let source_roots = canonical_source_roots(source_roots)?;
            let setup_roots = canonical_setup_roots(setup_roots)?;
            let enrollment = tect_postgres::admin::enroll_host_with_grants(
                &pool,
                tenant,
                source_roots,
                setup_roots,
            )
            .await?;
            write_auth_file(&out, &enrollment.auth)?;
            println!(
                "enrolled host {} tenant {} principal {}; auth written to {}",
                enrollment.auth.host_id,
                enrollment.tenant_id,
                enrollment.principal_id,
                out.display()
            );
        }
        Command::EnrollVerifier {
            tenant,
            workspace,
            out,
        } => {
            preflight_output(&out)?;
            let enrollment =
                tect_postgres::admin::enroll_verifier(&pool, tenant, workspace).await?;
            write_auth_file(&out, &enrollment.auth)?;
            println!(
                "enrolled verifier host {} tenant {} principal {} workspace {}; auth written to {}",
                enrollment.auth.host_id,
                enrollment.tenant_id,
                enrollment.principal_id,
                workspace,
                out.display()
            );
        }
        Command::EnsureTenant { tenant } => {
            let identity = tect_postgres::admin::ensure_tenant(&pool, tenant).await?;
            println!(
                "tenant {} principal {}",
                identity.tenant_id, identity.principal_id
            );
        }
        Command::RegisterHost {
            tenant,
            auth_file,
            source_roots,
            setup_roots,
        } => {
            let auth = tect_host::read_host_auth_file(&auth_file)?;
            let source_roots = canonical_source_roots(source_roots)?;
            let setup_roots = canonical_setup_roots(setup_roots)?;
            let registration = tect_postgres::admin::register_host(
                &pool,
                tenant,
                &auth,
                source_roots,
                setup_roots,
            )
            .await?;
            println!(
                "registered host {} tenant {} principal {}",
                registration.host_id, registration.tenant_id, registration.principal_id
            );
        }
        Command::GrantSetupRoot {
            host_id,
            setup_root,
        } => {
            let host_id = Uuid::parse_str(&host_id).map_err(|_| Error::InvalidArguments)?;
            if host_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            let setup_root = canonical_setup_root(setup_root)?;
            tect_postgres::admin::grant_setup_root(&pool, host_id, setup_root.clone()).await?;
            println!("granted setup root {setup_root} to host {host_id}");
        }
        Command::RevokeHost { host_id } => {
            tect_postgres::admin::revoke_host(&pool, host_id).await?;
            println!("revoked host {host_id}");
        }
        Command::RevokeSession { session_id } => {
            tect_postgres::admin::revoke_session(&pool, session_id).await?;
            println!("revoked session {session_id}");
        }
        Command::KnowledgeSuppressionExport { out } => {
            knowledge_recovery_cli::export(&pool, &out).await?;
        }
        Command::KnowledgeSuppressionApply {
            manifest,
            expected_lineage,
            expected_sequence,
            expected_digest,
        } => {
            knowledge_recovery_cli::apply(
                &pool,
                &manifest,
                expected_lineage,
                expected_sequence,
                expected_digest,
            )
            .await?;
        }
    }
    pool.close().await;
    Ok(())
}

fn canonical_source_roots(paths: Vec<PathBuf>) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut roots = Vec::with_capacity(paths.len());
    for path in paths {
        let canonical = std::fs::canonicalize(path).map_err(|_| Error::InvalidSource)?;
        if !canonical.is_dir() {
            return Err(Error::InvalidSource);
        }
        let root = canonical
            .into_os_string()
            .into_string()
            .map_err(|_| Error::InvalidSource)?;
        if seen.insert(root.clone()) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn canonical_setup_roots(paths: Vec<PathBuf>) -> Result<Vec<String>> {
    let mut seen = HashSet::new();
    let mut roots = Vec::with_capacity(paths.len());
    for path in paths {
        let root = canonical_setup_root(path)?;
        if seen.insert(root.clone()) {
            roots.push(root);
        }
    }
    Ok(roots)
}

fn canonical_setup_root(path: PathBuf) -> Result<String> {
    if !path.is_absolute()
        || path
            .components()
            .any(|part| !matches!(part, Component::RootDir | Component::Normal(_)))
    {
        return Err(Error::InvalidArguments);
    }
    let selected = path.to_str().ok_or(Error::InvalidArguments)?;
    validate_setup_path(selected)?;

    let canonical = std::fs::canonicalize(&path).map_err(|_| Error::SetupUnavailable)?;
    verify_setup_directory_chain(&canonical)?;
    let root = canonical
        .into_os_string()
        .into_string()
        .map_err(|_| Error::InvalidArguments)?;
    validate_setup_path(&root)?;
    Ok(root)
}

fn verify_setup_directory_chain(path: &Path) -> Result<()> {
    let mut current = PathBuf::from("/");
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(Error::InvalidArguments);
            }
        }
        let metadata = std::fs::symlink_metadata(&current).map_err(|_| Error::SetupUnavailable)?;
        if metadata.file_type().is_symlink() || !metadata.is_dir() {
            return Err(Error::SetupUnavailable);
        }
    }
    Ok(())
}

fn preflight_output(path: &Path) -> Result<()> {
    if !path.is_absolute() {
        return Err(Error::InvalidArguments);
    }
    match std::fs::symlink_metadata(path) {
        Ok(_) => return Err(Error::InvalidConfiguration),
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => {}
        Err(_) => return Err(Error::InvalidConfiguration),
    }
    let parent = path.parent().ok_or(Error::InvalidArguments)?;
    let metadata = verify_directory_chain(parent)?;
    if metadata.permissions().mode() & 0o777 != 0o700 {
        return Err(Error::InvalidConfiguration);
    }
    Ok(())
}

fn verify_directory_chain(path: &Path) -> Result<std::fs::Metadata> {
    let mut current = PathBuf::from("/");
    let mut final_metadata =
        std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
    for component in path.components() {
        match component {
            Component::RootDir => continue,
            Component::Normal(part) => current.push(part),
            Component::CurDir | Component::ParentDir | Component::Prefix(_) => {
                return Err(Error::InvalidArguments);
            }
        }
        final_metadata =
            std::fs::symlink_metadata(&current).map_err(|_| Error::InvalidConfiguration)?;
        if final_metadata.file_type().is_symlink() || !final_metadata.is_dir() {
            return Err(Error::InvalidConfiguration);
        }
    }
    Ok(final_metadata)
}

fn write_auth_file(path: &Path, auth: &HostAuth) -> Result<()> {
    preflight_output(path)?;
    let parent = path.parent().unwrap_or_else(|| Path::new("."));
    let file_name = path
        .file_name()
        .and_then(|name| name.to_str())
        .ok_or(Error::InvalidConfiguration)?;
    let temporary = parent.join(format!(".{file_name}.{}.tmp", Uuid::new_v4()));
    let result = write_and_link(&temporary, path, auth);
    if temporary.exists() {
        let _ = std::fs::remove_file(&temporary);
    }
    result
}

fn write_and_link(temporary: &Path, destination: &Path, auth: &HostAuth) -> Result<()> {
    let mut file = OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(temporary)
        .map_err(|_| Error::InvalidConfiguration)?;
    let mut contents = serde_json::to_vec_pretty(auth).map_err(|_| Error::InvalidConfiguration)?;
    contents.push(b'\n');
    file.write_all(&contents)
        .and_then(|()| file.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    std::fs::hard_link(temporary, destination).map_err(|_| Error::InvalidConfiguration)?;
    File::open(destination)
        .and_then(|created| created.sync_all())
        .map_err(|_| Error::InvalidConfiguration)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn vector_search_activation_is_an_explicit_migrate_flag() {
        let args = Arguments::try_parse_from([
            "tect-admin",
            "migrate",
            "--runtime-role",
            "tect_runtime",
            "--enable-knowledge-vector-search",
        ])
        .unwrap();
        let Command::Migrate {
            enable_durable_knowledge,
            enable_knowledge_vector_search,
            ..
        } = args.command
        else {
            panic!("migrate")
        };
        assert!(!enable_durable_knowledge);
        assert!(enable_knowledge_vector_search);
        let default =
            Arguments::try_parse_from(["tect-admin", "migrate", "--runtime-role", "tect_runtime"])
                .unwrap();
        let Command::Migrate {
            enable_knowledge_vector_search,
            ..
        } = default.command
        else {
            panic!("migrate")
        };
        assert!(!enable_knowledge_vector_search);
    }
}
