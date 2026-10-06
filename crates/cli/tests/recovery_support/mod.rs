#[allow(dead_code)]
pub mod candidate_collections;
#[allow(dead_code)]
pub mod candidate_reads;
#[allow(dead_code)]
pub mod candidate_reviews;
pub mod help_reads;
#[allow(dead_code)]
pub mod native_reads;
#[allow(dead_code)]
pub mod pipeline_reads;

use serde_json::{Value, json};
use std::{
    fs,
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    time::Duration,
};
use tect_domain::HostAuth;
use tokio::{io::AsyncWriteExt, process::Child};

mod daemon;
mod mcp;
mod wire;
pub use daemon::Daemon;
pub use mcp::Mcp;

pub fn tool_payload(response: &Value) -> Value {
    assert!(response.get("error").is_none(), "{response}");
    let result = &response["result"];
    assert!(result.get("structuredContent").is_none(), "{response}");
    let content = result["content"].as_array().expect("tool content array");
    assert_eq!(content.len(), 3, "{response}");
    assert_eq!(content[0]["type"], "text", "{response}");
    let intro = content[0]["text"].as_str().expect("fixed tool intro");
    assert!(!intro.is_empty() && intro.len() <= 2_000, "{response}");
    assert_eq!(content[1]["type"], "text", "{response}");
    assert_eq!(content[2]["type"], "text", "{response}");
    assert_eq!(
        content[2]["text"],
        "Follow the rules from workspace.open or help {\"text\":\"response-rules\"}. Required checks, approvals and authority still apply. Dependencies alone grant no permission or automatic resumption. Claim monitoring or continuation only when real."
    );
    let payload: Value =
        serde_json::from_str(content[1]["text"].as_str().expect("JSON tool payload"))
            .expect("content[1] must contain one JSON object");
    assert!(payload.is_object(), "{payload}");
    assert!(payload["actions"].is_array(), "{payload}");
    assert!(
        payload["recommended_action"].is_number() || payload["recommended_action"].is_null(),
        "{payload}"
    );
    payload
}

pub fn host_file(path: &Path, auth: &HostAuth) {
    use std::io::Write;
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(auth).unwrap()).unwrap();
}
pub fn private_temp() -> tempfile::TempDir {
    let dir = tempfile::tempdir().unwrap();
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    dir
}
pub fn tagged_url(url: &str, tag: &str) -> String {
    format!(
        "{url}{}application_name={tag}",
        if url.contains('?') { "&" } else { "?" }
    )
}

pub fn public_call(name: &str, arguments: Value) -> Value {
    let routed = match name {
        "get_state" => return json!({"name":"get_state","arguments":arguments}),
        "get_program" => ("query", "program.get"),
        "list_programs" => ("query", "program.list"),
        "list_sources" => ("query", "source.list"),
        "get_setup" => ("query", "setup.get"),
        "candidate_context" => ("query", "scope.candidates.context"),
        "open_workspace" => ("command", "workspace.open"),
        "register_source" => ("command", "source.register"),
        "select_worktrees" => ("command", "session.select_worktrees"),
        "begin_program" => ("command", "program.begin"),
        "save_program" => ("command", "program.save"),
        "record_program_input" => ("command", "program.record_input"),
        "inspect_setup" => ("command", "setup.inspect"),
        "begin_setup" => ("command", "setup.begin"),
        "save_setup" => ("command", "setup.save"),
        "record_setup_input" => ("command", "setup.record_input"),
        "begin_candidate_set" => ("command", "scope.candidates.begin"),
        "save_candidate_set" => ("command", "scope.candidates.save"),
        "record_candidate_input" => ("command", "scope.candidates.record_input"),
        "refresh_candidate_set" => ("command", "scope.candidates.refresh"),
        "scope_candidate_delta" => ("command", "scope.candidates.delta"),
        "scope_candidate_delta_status" => ("query", "scope.candidates.delta.status"),
        "pipeline_run_migrate" => ("command", "slice.pipeline.run.migrate"),
        "apply_setup" => ("execute", "setup.apply"),
        "read_skill" => {
            return json!({"name":"help","arguments":{
                "mode":"describe","method":arguments["name"]
            }});
        }
        _ => return json!({"name":name,"arguments":arguments}),
    };
    json!({"name":routed.0,"arguments":{"route":routed.1,"params":arguments}})
}

#[allow(dead_code)]
pub fn action_name(action: &Value) -> Option<&str> {
    action["arguments"]["route"]
        .as_str()
        .or_else(|| action["arguments"]["method"].as_str())
        .or_else(|| action["tool"].as_str())
}

#[allow(dead_code)]
pub fn action_params(action: &Value) -> &Value {
    action["arguments"]
        .get("params")
        .unwrap_or(&action["arguments"])
}

#[allow(dead_code)]
pub fn find_action<'a>(payload: &'a Value, name: &str) -> Option<&'a Value> {
    payload["actions"]
        .as_array()?
        .iter()
        .find(|action| action_name(action) == Some(name))
}

#[allow(dead_code)]
pub fn ready_action(name: &str, arguments: Value) -> Value {
    let mut call = public_call(name, arguments);
    call["tool"] = call["name"].take();
    call.as_object_mut().unwrap().remove("name");
    call["kind"] = json!("ready_call");
    call
}
async fn normal_finish<I: tokio::io::AsyncWrite + Unpin>(
    mut input: I,
    child: &mut Child,
    deadline: Duration,
) -> Result<(), String> {
    tokio::time::timeout(deadline, async {
        input
            .flush()
            .await
            .map_err(|e| format!("MCP stdin flush: {e}"))?;
        input
            .shutdown()
            .await
            .map_err(|e| format!("MCP stdin shutdown: {e}"))?;
        drop(input);
        let status = child.wait().await.map_err(|e| format!("MCP wait: {e}"))?;
        if status.success() {
            Ok(())
        } else {
            Err(format!("MCP EOF exit was not successful: {status}"))
        }
    })
    .await
    .unwrap_or_else(|_| Err("MCP normal shutdown deadline exceeded".into()))
}
async fn kill_and_reap(child: &mut Child) -> Result<(), String> {
    let probe = child.try_wait();
    kill_and_reap_with_probe(child, probe).await
}
async fn kill_and_reap_with_probe(
    child: &mut Child,
    probe: std::io::Result<Option<std::process::ExitStatus>>,
) -> Result<(), String> {
    let mut failures = Vec::new();
    match probe {
        Ok(Some(_)) => return Ok(()),
        Ok(None) => {}
        Err(error) => failures.push(format!("owned child status: {error}")),
    }
    // Probe failure is not permission to abandon cleanup of this retained Child.
    if let Err(error) = child.start_kill() {
        failures.push(format!("owned child kill call: {error}"));
    }
    match tokio::time::timeout(Duration::from_secs(5), child.wait()).await {
        Ok(Ok(_)) => {}
        Ok(Err(error)) => failures.push(format!("owned child reap: {error}")),
        Err(_) => failures.push("owned child reap exceeded five seconds".into()),
    }
    if failures.is_empty() {
        Ok(())
    } else {
        Err(failures.join("; "))
    }
}

// This shared module is compiled per integration binary; only normal-tail suites use these APIs.
#[allow(dead_code)]
pub async fn finish_and_stop(client: Mcp, daemon: &mut Daemon) {
    let mcp = client.finish_result().await;
    stop_after_mcp(mcp, daemon).await;
}

#[derive(Debug)]
#[allow(dead_code)]
pub struct CleanupReport {
    pub mcp: Result<(), String>,
    pub daemon_already_exited: Result<bool, String>,
    pub socket: Result<(), String>,
}
impl CleanupReport {
    #[allow(dead_code)]
    pub fn normal_tail_succeeded(&self) -> bool {
        self.mcp.is_ok() && matches!(self.daemon_already_exited, Ok(false)) && self.socket.is_ok()
    }
}
#[allow(dead_code)]
pub async fn cleanup_after_mcp(mcp: Result<(), String>, daemon: &mut Daemon) -> CleanupReport {
    // Normal-tail callers never request daemon exit before this cleanup.
    let stop = daemon.stop().await;
    let unlink = daemon.unlink_after_stop();
    CleanupReport {
        mcp,
        daemon_already_exited: stop,
        socket: unlink,
    }
}
#[allow(dead_code)]
pub async fn stop_after_mcp(mcp: Result<(), String>, daemon: &mut Daemon) {
    let report = cleanup_after_mcp(mcp, daemon).await;
    assert!(report.normal_tail_succeeded(), "owned cleanup: {report:?}");
}

fn retain_first(original: String, cleanup: Result<(), String>) -> String {
    match cleanup {
        Ok(()) => original,
        Err(cleanup) => format!("{original}; fallback cleanup: {cleanup}"),
    }
}
