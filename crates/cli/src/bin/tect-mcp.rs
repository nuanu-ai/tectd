use std::path::PathBuf;
use tect_domain::Error;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}", error.code());
        std::process::exit(1);
    }
}

async fn run() -> tect_domain::Result<()> {
    let socket = required_absolute_path("TECT_SOCKET")?;
    let context = tect_host::context_from_env()?;
    tect_host::run_stdio(&socket, context).await
}

fn required_absolute_path(name: &str) -> tect_domain::Result<PathBuf> {
    let value = std::env::var(name).map_err(|_| Error::InvalidConfiguration)?;
    let path = PathBuf::from(value);
    if !path.is_absolute() {
        return Err(Error::InvalidConfiguration);
    }
    Ok(path)
}
