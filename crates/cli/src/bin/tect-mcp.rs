mod claude_pre_tool_use;

use std::path::PathBuf;
use tect_domain::Error;

#[tokio::main]
async fn main() {
    if let Err(error) = run().await {
        eprintln!("{}", error.code());
        let hook_mode = std::env::args_os()
            .nth(1)
            .is_some_and(|argument| argument == "claude-pre-tool-use");
        std::process::exit(if hook_mode { 2 } else { 1 });
    }
}

async fn run() -> tect_domain::Result<()> {
    let mut arguments = std::env::args_os();
    let executable = arguments.next().ok_or(Error::InvalidConfiguration)?;
    if let Some(mode) = arguments.next() {
        if mode != "claude-pre-tool-use" {
            return Err(Error::InvalidConfiguration);
        }
        return claude_pre_tool_use::run(std::iter::once(executable).chain(arguments));
    }
    let socket = required_absolute_path("TECT_SOCKET")?;
    let context = tect_host::host_context_from_env()?;
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
