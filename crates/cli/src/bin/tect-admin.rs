use clap::{Parser, Subcommand};
use std::collections::HashSet;
use std::fs::{File, OpenOptions};
use std::io::Write;
use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
use std::path::{Component, Path, PathBuf};
use std::process::ExitCode;
use tect_domain::{Error, HostAuth, Result};
use uuid::Uuid;

#[derive(Parser)]
#[command(name = "tect-admin")]
struct Arguments {
    #[command(subcommand)]
    command: Command,
}

#[derive(Subcommand)]
enum Command {
    Migrate {
        #[arg(long)]
        runtime_role: String,
    },
    Enroll {
        #[arg(long)]
        tenant: Option<Uuid>,
        #[arg(long = "source-root")]
        source_roots: Vec<PathBuf>,
        #[arg(long)]
        out: PathBuf,
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
    let pool = tect_postgres::admin::connect_admin(&admin_url).await?;
    match arguments.command {
        Command::Migrate { runtime_role } => {
            tect_postgres::admin::migrate(&pool, &runtime_role).await?;
            println!("migration complete for runtime role {runtime_role}");
        }
        Command::Enroll {
            tenant,
            source_roots,
            out,
        } => {
            preflight_output(&out)?;
            let source_roots = canonical_source_roots(source_roots)?;
            let enrollment = tect_postgres::admin::enroll_host(&pool, tenant, source_roots).await?;
            write_auth_file(&out, &enrollment.auth)?;
            println!(
                "enrolled host {} tenant {} principal {}; auth written to {}",
                enrollment.auth.host_id,
                enrollment.tenant_id,
                enrollment.principal_id,
                out.display()
            );
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
