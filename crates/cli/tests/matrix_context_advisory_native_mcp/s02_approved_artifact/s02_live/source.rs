//! A clean Git snapshot of this branch, not the installed Active JEV Work.
use super::*;
use std::process::Command;

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .expect("git must be available for source provenance");
    assert!(output.status.success(), "Git source check failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) fn clone_current_dev_source(destination: &Path) -> String {
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .canonicalize()
        .unwrap();
    assert_eq!(
        git(&checkout, &["branch", "--show-current"]),
        "codex/tectd-jev-dev"
    );
    let head = git(&checkout, &["rev-parse", "HEAD"]);
    assert!(head.len() == 40 && head.bytes().all(|byte| byte.is_ascii_hexdigit()));
    let status = Command::new("git")
        .args([
            "clone",
            "--quiet",
            "--no-hardlinks",
            "--single-branch",
            "--branch",
        ])
        .arg("codex/tectd-jev-dev")
        .arg(&checkout)
        .arg(destination)
        .status()
        .unwrap();
    assert!(status.success(), "isolated source clone failed");
    assert_eq!(git(destination, &["rev-parse", "HEAD"]), head);
    assert!(git(destination, &["status", "--porcelain"]).is_empty());
    assert!(destination
        .join("crates/cli/tests/matrix_context_advisory_native_mcp/s02_approved_artifact/s02_live.rs")
        .is_file());
    head
}

fn planning_guard(value: &Value) -> Option<Value> {
    let manifest = &value["planning_knowledge"]["manifest"];
    manifest["id"].as_str().map(|_| {
        json!({"manifest_id":manifest["id"],"digest":manifest["digest"],
            "workspace_generation":manifest["workspace_generation"]})
    })
}

pub(super) async fn ready_active_jev_source_candidate(
    client: &mut Mcp,
    repo: &Path,
    head: &str,
) -> (Value, Value) {
    client.call("open_workspace", json!({})).await;
    let registered = client.call("register_source", json!({"path":repo})).await;
    client
        .call(
            "select_worktrees",
            json!({"worktree_ids":[registered["id"]]}),
        )
        .await;
    let begun = client.call("begin_program", json!({
        "request_id":Uuid::new_v4(),
        "input":format!(
            "Active JEV MVP optional advisor at TectD dev Git {head}; source: crates/cli/tests/matrix_context_advisory_native_mcp/s02_approved_artifact/s02_live.rs. Prove artifact-bound Matrix advice without production promotion."
        )
    })).await;
    let mut program_save = json!({
        "program_id":begun["program"]["id"],"revision":1,"input_cursor":1,
        "name":"Active JEV MVP isolated proof",
        "intent":"Prove optional, audited JEV advice against Owner-confirmed MVP facts",
        "basis":format!("Exact branch source snapshot {head} and current Owner-approved Matrix case"),
        "boundaries":"Disposable PostgreSQL and branch-built MCP only",
        "constraints":"No installed runtime contact, production promotion or automatic advice application",
        "success":"The selected Work preserves mandatory cards and has independent effect proof",
        "complete":true
    });
    if let Some(guard) = planning_guard(&begun["program"]) {
        program_save["consumed_knowledge"] = guard;
    }
    let program = client.call("save_program", program_save).await;
    let candidates = client.call("begin_candidate_set", json!({
        "request_id":Uuid::new_v4(),"program_id":program["program"]["id"],
        "program_revision":program["program"]["revision"],"boundary":"ongoing",
        "input":"Bound the Active JEV MVP Matrix advice proof and its mandatory data/secret guarantees."
    })).await;
    let context = &candidates["context"];
    let inputs = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25
            }),
        )
        .await;
    let source_ref = &inputs["items"][0]["input"]["source_ref_id"];
    let mut candidate_save = json!({
        "kind":"draft","candidate_set_id":context["candidate_set"]["id"],"revision":1,
        "snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"ongoing","goals":[{
            "identity":{"local":"goal"},
            "text":"Prove isolated artifact-bound optional Matrix advice and preserve required guarantees",
            "source_ref_id":source_ref,"resolution":{"kind":"candidate","reference":{"local":"scope"}}
        }],"evidence":[],"candidates":[{
            "identity":{"local":"scope"},"title":"Active JEV MVP Matrix proof",
            "outcome":"Source-bound optional advice and independent proof of the selected Work",
            "trigger":format!("Dev Git {head} and Owner-confirmed MVP Matrix task"),
            "delivered_behavior":"Advice is audited, optional and never auto-applied",
            "proof":"Public artifact, distinct verification and selected-effect receipts",
            "includes":["Matrix evidence","selected Work","data and secret guarantees"],
            "excludes":["installed runtime","production promotion"],"dependencies":[],
            "coverage_goals":[{"local":"goal"}],"evidence":[]
        }],"blockers":[],"protected_changes":[]}
    });
    if let Some(guard) = planning_guard(context) {
        candidate_save["consumed_knowledge"] = guard;
    }
    let saved = client.call("save_candidate_set", candidate_save).await;
    let candidate = saved["draft"]["candidates"][0].clone();
    let mut candidate_review = json!({
        "kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],
        "revision":saved["context"]["candidate_set"]["revision"],
        "snapshot_id":context["snapshot"]["id"],
        "input_cursor":saved["context"]["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"Isolated Active JEV MVP Scope is source-pinned",
            "findings":[],"candidate_decisions":[{
                "candidate_id":candidate["id"],"decision":"accept",
                "rationale":"Bounded source-derived optional-advice proof"
            }]}
    });
    if let Some(guard) = planning_guard(&saved["context"]) {
        candidate_review["consumed_knowledge"] = guard;
    }
    let reviewed = client.call("save_candidate_set", candidate_review).await;
    (reviewed["context"].clone(), candidate)
}
