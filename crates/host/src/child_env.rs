//! The Jev client's settings and key stay in the daemon: no child process inherits them.
use std::os::unix::ffi::OsStrExt;
use tokio::process::Command;

/// The variables the daemon reads for Jev: the client's three and its key, and the
/// daily limit for workspaces the daemon creates.
pub const JEV_ENVIRONMENT: [&str; 5] = [
    "TECT_JEV_ENDPOINT",
    "TECT_JEV_MODEL",
    "TECT_JEV_TIMEOUT_MS",
    "TYPESAFE_API_KEY",
    "TECT_JEV_NEW_WORKSPACE_DAILY_LIMIT",
];

/// Withhold the Jev key and every `TECT_JEV_*` variable from a child process.
pub fn withhold_jev_environment(command: &mut Command) {
    for name in JEV_ENVIRONMENT {
        command.env_remove(name);
    }
    for (name, _) in std::env::vars_os() {
        if name.as_bytes().starts_with(b"TECT_JEV_") {
            command.env_remove(name);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::ffi::OsStr;
    use std::process::Stdio;

    /// Whether the command is instructed to remove the variable from its child.
    fn withheld(command: &Command, name: &str) -> bool {
        command
            .as_std()
            .get_envs()
            .any(|(key, value)| key == OsStr::new(name) && value.is_none())
    }

    #[tokio::test]
    async fn a_child_never_receives_the_jev_key_or_settings() {
        let mut command = Command::new("/usr/bin/env");
        command
            .env("TYPESAFE_API_KEY", "probe-key")
            .env("TECT_JEV_ENDPOINT", "http://127.0.0.1:1/v1/systemone")
            .env("TECT_JEV_MODEL", "jev-latest")
            .env("TECT_JEV_TIMEOUT_MS", "100")
            .env("TECT_JEV_NEW_WORKSPACE_DAILY_LIMIT", "7")
            .env("TECT_CHILD_PROBE", "kept")
            .stdin(Stdio::null());
        withhold_jev_environment(&mut command);
        for name in JEV_ENVIRONMENT {
            assert!(withheld(&command, name), "{name}");
        }
        let output = command.output().await.unwrap();
        let environment = String::from_utf8_lossy(&output.stdout);
        assert!(output.status.success());
        assert!(environment.contains("TECT_CHILD_PROBE=kept"));
        assert!(!environment.contains("TYPESAFE_API_KEY"));
        assert!(!environment.contains("TECT_JEV_"));
        assert!(!environment.contains("probe-key"));
    }
}
