use super::*;
use std::process::Command;

fn git(repo: &std::path::Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "Git source provenance failed");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) fn clone_current_dev_source(destination: &std::path::Path) -> String {
    let checkout = std::path::Path::new(env!("CARGO_MANIFEST_DIR"))
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
    assert!(head.len() == 40 && head.bytes().all(|ch| ch.is_ascii_hexdigit()));
    assert!(
        Command::new("git")
            .args([
                "clone",
                "--quiet",
                "--no-hardlinks",
                "--single-branch",
                "--branch",
                "codex/tectd-jev-dev"
            ])
            .arg(&checkout)
            .arg(destination)
            .status()
            .unwrap()
            .success()
    );
    assert_eq!(git(destination, &["rev-parse", "HEAD"]), head);
    assert!(git(destination, &["status", "--porcelain"]).is_empty());
    assert!(
        destination
            .join("crates/cli/tests/anti_bloat_one_shot.rs")
            .is_file()
    );
    head
}
async fn tool(pool: &PgPool, client: &mut Mcp, name: &str, params: Value) -> Value {
    identity(pool).await;
    client.call(name, params).await
}
fn knowledge(params: &mut Value, context: &Value) {
    let manifest = &context["planning_knowledge"]["manifest"];
    if manifest["id"].is_string() {
        params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],"digest":manifest["digest"],"workspace_generation":manifest["workspace_generation"]});
    }
}
// Same public source-planning sequence as native_planning/support, with the
// owned-cluster identity refreshed immediately before each mutating tool.
pub(super) async fn ready(
    pool: &PgPool,
    client: &mut Mcp,
    source: &std::path::Path,
    head: &str,
) -> (Value, Vec<Value>, Vec<Value>) {
    tool(pool, client, "open_workspace", json!({})).await;
    let registered = tool(pool, client, "register_source", json!({"path":source})).await;
    tool(
        pool,
        client,
        "select_worktrees",
        json!({"worktree_ids":[registered["id"]]}),
    )
    .await;
    let begun=tool(pool,client,"begin_program",json!({"request_id":Uuid::new_v4(),"input":format!("Active JEV MVP isolated dev Work at exact TectD Git {head}, source crates/cli/tests/anti_bloat_one_shot.rs. Owner-attested operating facts: one repository, development criticality, data and secret guarantees, no current deployed exposure or urgent incident. Jev remains optional and audited. Preserve EM02-SCOPE@0.1 and EM02-PROTECT@0.1; no installed runtime or production promotion.")})).await;
    let mut params = json!({"program_id":begun["program"]["id"],"revision":1,"input_cursor":1,"name":"Active JEV MVP isolated anti-bloat Work","intent":"Keep optional audited JEV advice source-relative and recommendation-only","basis":format!("Exact clean TectD dev Git {head} and fixed Owner-attested MVP case"),"boundaries":"One development repository, explicit caller disposition, no installed runtime","constraints":"Preserve EM02-SCOPE@0.1 and EM02-PROTECT@0.1; protect data and secrets; no automatic application","success":"Required MVP, SCOPE and PROTECT Work remain after any optional exploratory reduction and independent verification","complete":true});
    knowledge(&mut params, &begun["program"]);
    let program = tool(pool, client, "save_program", params).await;
    let candidates=tool(pool,client,"begin_candidate_set",json!({"request_id":Uuid::new_v4(),"program_id":program["program"]["id"],"program_revision":program["program"]["revision"],"boundary":"ongoing","input":"Preserve selected Active JEV MVP Work and all applicable SCOPE/PROTECT obligations; identify only unrequested growth."})).await;
    let context = &candidates["context"];
    let inputs = tool(
        pool,
        client,
        "candidate_context",
        json!({"candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25}),
    )
    .await;
    let reference = &inputs["items"][0]["input"]["source_ref_id"];
    let requirements = [
        (
            "mvp",
            "Required Active JEV MVP Work",
            "Optional audited JEV advice stays recommendation-only with explicit caller disposition",
            "Public advice and selected effect are separately auditable",
        ),
        (
            "scope",
            "Required EM02-SCOPE@0.1 Work",
            "Keep this case bounded to one development repository and preserve the full SCOPE obligation",
            "The full SCOPE body remains unchanged after optional reduction",
        ),
        (
            "protect",
            "Required EM02-PROTECT@0.1 Work",
            "Preserve data and secret guarantees; neither payment nor deployed exposure is claimed",
            "The full PROTECT body remains unchanged after optional reduction",
        ),
    ];
    let goals = requirements.iter().map(|(local, _, behavior, _)| json!({
        "identity":{"local":format!("goal-{local}")},"text":behavior,"source_ref_id":reference,
        "resolution":{"kind":"candidate","reference":{"local":local}}
    })).collect::<Vec<_>>();
    let draft_candidates = requirements
        .iter()
        .map(|(local, title, behavior, proof)| {
            json!({
                "identity":{"local":local},"title":title,"outcome":behavior,
                "trigger":format!("Exact dev Git {head} and Owner-attested MVP case"),
                "delivered_behavior":behavior,"proof":proof,"includes":[behavior],
                "excludes":["automatic JEV application","production promotion"],"dependencies":[],
                "coverage_goals":[{"local":format!("goal-{local}")}],"evidence":[]
            })
        })
        .collect::<Vec<_>>();
    let mut params = json!({"kind":"draft","candidate_set_id":context["candidate_set"]["id"],"revision":1,"snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"ongoing","goals":goals,"evidence":[],"candidates":draft_candidates,"blockers":[],"protected_changes":[]}});
    knowledge(&mut params, context);
    let saved = tool(pool, client, "save_candidate_set", params).await;
    let candidates = saved["draft"]["candidates"].as_array().unwrap().clone();
    let decisions = candidates.iter().map(|candidate| json!({
        "candidate_id":candidate["id"],"decision":"accept","rationale":"Source-bound mandatory Active JEV Work"
    })).collect::<Vec<_>>();
    let mut params = json!({"kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],"revision":saved["context"]["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],"input_cursor":saved["context"]["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),"review":{"verdict":"ready","summary":"MVP, SCOPE and PROTECT remain required","findings":[],"candidate_decisions":decisions}});
    knowledge(&mut params, &saved["context"]);
    let reviewed = tool(pool, client, "save_candidate_set", params).await;
    (
        reviewed["context"].clone(),
        candidates,
        saved["draft"]["goals"].as_array().unwrap().clone(),
    )
}
