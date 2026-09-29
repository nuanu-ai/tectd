//! Test-only Active JEV Work and differentiated allowed routes. No model is dispatched.
use super::*;
use std::{path::Path, process::Command};

fn git(repo: &Path, args: &[&str]) -> String {
    let output = Command::new("git")
        .arg("-C")
        .arg(repo)
        .args(args)
        .output()
        .unwrap();
    assert!(output.status.success(), "exact Git source required");
    String::from_utf8(output.stdout).unwrap().trim().to_owned()
}

pub(super) fn clone_dev_source(destination: &Path) -> String {
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
            .join("crates/cli/tests/matrix_context_advisory_native_mcp/s05_public_v2/s05_live.rs")
            .is_file()
    );
    head
}

fn knowledge(params: &mut Value, context: &Value) {
    let manifest = &context["planning_knowledge"]["manifest"];
    if manifest["id"].is_string() {
        params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],
            "digest":manifest["digest"],"workspace_generation":manifest["workspace_generation"]});
    }
}

pub(super) async fn ready_work(owner: &mut Mcp, repo: &Path, head: &str) -> (Value, Value) {
    owner.call("open_workspace", json!({})).await;
    let registered = owner.call("register_source", json!({"path":repo})).await;
    owner
        .call(
            "select_worktrees",
            json!({"worktree_ids":[registered["id"]]}),
        )
        .await;
    let begun = owner.call("begin_program", json!({"request_id":Uuid::new_v4(),
        "input":format!("Active JEV MVP development in the exact TectD dev Git {head}; inspect crates/cli/tests/matrix_context_advisory_native_mcp/s05_public_v2/s05_live.rs. Owner-attested one repository, development criticality, data and secret guarantees, no deployed exposure or urgent repair. Advice is optional and audited; model routing is recommendation-only and must not dispatch a model.")})).await;
    let mut params = json!({"program_id":begun["program"]["id"],"revision":1,"input_cursor":1,
        "name":"Active JEV MVP model-routing Work", "intent":"Recommend a policy-allowed code-work route without execution",
        "basis":format!("Exact clean TectD dev Git {head} and Owner-attested MVP case"),
        "boundaries":"One development repository; explicit Owner disposition; no model execution",
        "constraints":"Preserve data and secret guarantees and mandatory EM02-SCOPE@0.1/EM02-PROTECT@0.1; no auto-dispatch",
        "success":"Recommended, requested and actual routes remain distinct and actual is unknown without execution evidence",
        "complete":true});
    knowledge(&mut params, &begun["program"]);
    let program = owner.call("save_program", params).await;
    let begun_set = owner.call("begin_candidate_set",json!({"request_id":Uuid::new_v4(),
        "program_id":program["program"]["id"],"program_revision":program["program"]["revision"],
        "boundary":"ongoing","input":"Prepare one source-backed code Work node for recommendation-only routing; preserve all mandatory MVP obligations."})).await;
    let context = &begun_set["context"];
    let inputs = owner
        .call(
            "candidate_context",
            json!({"candidate_set_id":context["candidate_set"]["id"],
        "view":"inputs","limit":25}),
        )
        .await;
    let source_ref = &inputs["items"][0]["input"]["source_ref_id"];
    let mut params = json!({"kind":"draft","candidate_set_id":context["candidate_set"]["id"],
        "revision":1,"snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "draft":{"boundary":"ongoing","goals":[{"identity":{"local":"goal"},
            "text":"Route exact Active JEV MVP code Work as optional advice without executing a model; retain data and secret guarantees",
            "source_ref_id":source_ref,"resolution":{"kind":"candidate","reference":{"local":"scope"}}}],
        "evidence":[],"candidates":[{"identity":{"local":"scope"},"title":"Active JEV MVP code-work route decision",
            "outcome":"One auditable route recommendation and explicit Owner disposition, with actual route unknown",
            "trigger":format!("Exact dev source {head} and agent-authored code-work routing case"),
            "delivered_behavior":"Compare allowed code-capable routes without dispatching either model",
            "proof":"Read back distinct recommended, requested and actual route fields and immutable disposition",
            "includes":["source-backed code Work","recommendation-only routing","SCOPE and PROTECT constraints"],
            "excludes":["model execution","production promotion"],"dependencies":[],
            "coverage_goals":[{"local":"goal"}],"evidence":[]}],"blockers":[],"protected_changes":[]}});
    knowledge(&mut params, context);
    let saved = owner.call("save_candidate_set", params).await;
    let candidate = saved["draft"]["candidates"][0].clone();
    let mut params = json!({"kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],
        "revision":saved["context"]["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],
        "input_cursor":saved["context"]["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"Source-bound recommendation-only code Work",
            "findings":[],"candidate_decisions":[{"candidate_id":candidate["id"],
            "decision":"accept","rationale":"Exact dev source and bounded route comparison"}]}});
    knowledge(&mut params, &saved["context"]);
    let reviewed = owner.call("save_candidate_set", params).await;
    (reviewed["context"].clone(), candidate)
}

// Agent-authored development policy assumptions for this exact code Work.
// Model names, floors and fixture host capability are not a production
// catalogue, proof of availability, economics, or authority to dispatch.
pub(super) struct ActiveRoutes;
impl ModelRouteCatalogueProvider for ActiveRoutes {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        let route =
            |id: &str, model: &str, effort: &str, budget: u64, latency: u64, capability: &str| {
                ModelRoute {
                    id: id.into(),
                    provider: "openai".into(),
                    model: model.into(),
                    effort: effort.into(),
                    enabled: true,
                    allowed_matrix_choice_ids: vec!["b".into()],
                    allowed_roles: vec!["agent".into()],
                    allowed_tools: vec!["code".into()],
                    allowed_data_classes: vec!["internal".into()],
                    required_host_capabilities: vec![capability.into()],
                    minimum_budget_units: budget,
                    minimum_latency_ms: latency,
                }
            };
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: vec![
                route("route-a", "gpt-6-sol", "medium", 18, 80, "model-api"),
                route("route-b", "gpt-6-luna", "xhigh", 8, 30, "model-api"),
                route(
                    "route-ineligible",
                    "gpt-6-sol",
                    "medium",
                    18,
                    80,
                    "unavailable-capability",
                ),
            ],
        }))
    }
}

pub(super) fn active_declarations() -> Vec<Value> {
    vec![
        declaration("mode", json!("mvp")),
        declaration(
            "intent",
            json!({"kind":"other","description":"Active JEV as optional TectD V2 advisor"}),
        ),
        declaration(
            "urgency",
            json!("Finish and verify this sprint; not an emergency production repair"),
        ),
        declaration(
            "promised_behavior",
            json!(
                "JEV is optional; skip/off prevents provider send; actual JEV calls are durably auditable; advice never auto-applies."
            ),
        ),
        declaration(
            "promised_proof",
            json!(
                "PG/CI tests plus real JEV outcomes; agent disposition; separate effect verification where selected"
            ),
        ),
        json!({"operation":"set","value":{"kind":"no_demand_commitment"}}),
        json!({"operation":"set","value":{"kind":"no_latency_commitment"}}),
    ]
}

pub(super) async fn record_work(owner: &mut Mcp, task: Uuid, program: Uuid, head: &str) -> Value {
    let provenance = "owner-approval:active-jev-s02-operating-facts-2026-09-29";
    let known = |value: Value| json!({"state":"known","value":value,"provenance":provenance});
    let input = json!({"mode":{"state":"absent"},"intent":{"state":"absent"},
        "urgency":{"state":"absent"},"promised_behavior":{"state":"absent"},
        "promised_proof":{"state":"absent"},"demand_commitment":{"state":"absent"},
        "latency_commitment":{"state":"absent"},"envelope":{"scale":known(json!("One workspace/repo")),
            "operational_facts":{"state":"reported","entries":[
                {"name":"jev_optional","fact":known(json!("JEV optional"))},
                {"name":"calls_audited","fact":known(json!("calls audited"))},
                {"name":"testing_isolated","fact":known(json!("testing isolated"))}]}},
        "criticality":known(json!("Development, not a production incident")),
        "affected_guarantees":known(json!(["data","secret"])),
        "actual_exposure":known(json!(false)),"urgent_repair":known(json!(false))});
    let choice = json!({"schema":"tect.matrix-choice-set/1","choice_set_id":"active-jev-s05-test-sequence",
        "version":1,"task_id":task.to_string(),"task_revision":"1",
        "decision_question":format!("Which agent-authored isolated implementation sequence prepares Active JEV MVP routing at exact dev Git {head} without executing a model?"),
        "candidates":[{"candidate_id":"a","title":"Route-first test sequence",
            "approach":"Exercise signed route advice on exact dev source before promotion","assumption_fact_ids":["actual_exposure"]},
            {"candidate_id":"b","title":"Evidence-first test sequence",
            "approach":"Verify source-bound Work and mandatory safeguards before route advice","assumption_fact_ids":["affected_guarantees"]}]});
    route(
        owner,
        "command",
        "task.source.record",
        json!({"task_id":task,"revision":1,
        "expected_current_revision":0,"request_id":Uuid::new_v4(),"input":input,
        "choice_set":choice,"requirements_locator":locator(program)}),
    )
    .await
}
