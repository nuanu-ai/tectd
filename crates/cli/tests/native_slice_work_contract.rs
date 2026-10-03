//! Requires an explicitly isolated fixture database; never uses the installed native daemon.
// Shared fixture APIs are used by other integration tests, beyond this focused validator suite.
#[allow(dead_code)]
#[path = "pipeline_execution/full_support.rs"]
mod full_support;
// Shared process-loss fixture APIs are exercised by other integration tests.
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;
use full_support::{completion, successful_route};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;
#[path = "native_planning/local_result.rs"]
mod local_result;
fn full_draft() -> Value {
    json!({"coverage_summary":"Full design through execution lifecycle","nodes":[{
        "kind":"work","identity":{"local":"full"},
        "title":"Design and implement the complete bounded behavior",
        "outcome":"The behavior is specified, implemented, verified and handed off",
        "includes":["design","implementation","verification","handoff"],
        "excludes":["production deployment","unrelated redesign"],"dependencies":[],
        "proof":["Specification traceability and focused verification"],
        "pipeline":"slice.full-design-to-execution",
        "pipeline_reason":"The task requires the full specification and execution chain",
        "why_lightweight_insufficient":"Cross-cutting specification, review and execution phases are all required.",
        "why_further_vertical_split_not_viable":"The bounded behavior shares one contract and one integrated acceptance boundary.",
        "source_result_ids":[]
    }],"supersessions":[]})
}

async fn current(client: &mut Mcp, c: &Value) -> Value {
    route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":c["run"]["id"],"refresh":true}),
    )
    .await
}
async fn advance(client: &mut Mcp, c: Value) -> Value {
    let (v, o, t) = successful_route(&c);
    let r = route(
        client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&c, v, o, t, None, None),
    )
    .await;
    r["context"].clone()
}
fn install_artifact(request: &mut Value, value: &Value) {
    let body = value.to_string();
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    let artifacts = request["output"]["artifacts"].as_array_mut().unwrap();
    let artifact = artifacts
        .iter_mut()
        .find(|a| a["name"] == "work-order-contract.json")
        .unwrap();
    artifact["body"] = json!(body);
    artifact["digest"] = json!(digest.clone());
    for r in request["output"]["validator_receipts"]
        .as_array_mut()
        .unwrap()
    {
        for a in r["artifacts"].as_array_mut().unwrap() {
            if a["name"] == "work-order-contract.json" {
                a["digest"] = json!(digest);
            }
        }
    }
}
fn declaration(c: &Value, state: &Value) -> Value {
    let source = &state["selected_worktrees"][0]["id"];
    let path = |p| json!({"source_id":source,"path":p});
    let reads=c["bindings"].as_array().unwrap().iter().filter(|b|b["phase_id"]=="slice-full-dev-entry-gate"||b["phase_id"]=="slice-design-spec-shaper").map(|b|json!({"reference":{"kind":"native_output","phase_id":b["phase_id"],"output_revision":b["output_revision"],"digest":b["output_digest"]},"required":true})).collect::<Vec<_>>();
    let input = c["inputs"].as_array().unwrap().last().unwrap();
    json!({"contract_kind":"native_slice_work_contract_v1",
        "target":{"scope_id":c["run"]["scope_id"],"slice_id":c["run"]["slice_id"],"slice_revision":c["run"]["slice_revision"],"run_id":c["run"]["id"],"run_revision":c["run"]["revision"],"phase_id":"slice-contract-writer","definition_kind":c["run"]["definition_kind"],"definition_version":c["run"]["definition_version"],"definition_digest":c["run"]["definition_digest"]},
        "session_declaration":{"workspace_id":state["workspace"]["id"],"session_id":state["session"]["id"],"host_id":state["session"]["host_id"],"native_session_id":state["session"]["native_session_id"]},"source_checkpoint":c.get("source_checkpoint").cloned().unwrap_or(Value::Null),
        "required_reads":reads,"write_scope":{"allowed_roots":[path("src")],"allowed_paths":[path("src/fixture.rs")],"denied_roots":[path(".git"),path(".tect"),path("tect/workspace")],"before_hash_required":true},
        "authority":{"status":"authorized","source":{"kind":"native_input","input_id":input["id"],"sequence":input["sequence"],"digest":input["digest"]},"scope":[{"action":"source_edit","targets":[path("src/fixture.rs")]}],"limitations":"Fixture source only. External effects need separate authority."},
        "proof_requirements":[{"id":"source-proof","obligation":"Focused fixture proof","evidence_kind":"native_output","completion_required":true}],"validation_requirements":[{"id":"source-test","obligation":"Focused fixture test","proof_requirement_ids":["source-proof"],"completion_required":true}],
        "result_closure":{"native_result_required":true,"summary_required":true,"evidence_required":true,"scope_impact_required":true,"remaining_work_required":true,"handoff_when_blocked":true},
        "refresh_resume":{"refresh_before_write":true,"recheck_authority":true,"rehash_required_reads":true,"verify_before_hash":true,"reconcile_unknown_outcome":true,"resume_from_current_native_context":true}})
}
async fn refuses_without_persistence(client: &mut Mcp, c: &Value, request: Value) {
    refuses_with_code_without_persistence(client, c, request, "INVALID_OUTPUT").await;
}
async fn refuses_with_code_without_persistence(
    client: &mut Mcp,
    c: &Value,
    request: Value,
    expected_code: &str,
) {
    let error = route_error(client, "command", "slice.pipeline.phase.complete", request).await;
    assert_eq!(error["error"]["refusal"]["code"], expected_code, "{error}");
    let after = current(client, c).await;
    for key in ["run", "attempts", "outputs", "bindings"] {
        assert_eq!(after[key], c[key], "changed {key}");
    }
}
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native_contract_semantic_refusal_precedes_persistence_and_consumers_recheck_provenance() {
    native_contract_fixture("0.6.0-native.engineering.3").await;
}
#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native4_contract_preserves_all_provenance_negatives_and_local_result_gate() {
    native_contract_fixture("0.6.0-native.engineering.4").await;
}
async fn native_contract_fixture(version: &str) {
    assert_eq!(
        std::env::var("TECT_TEST_NATIVE_CONTRACT_FIXTURE").as_deref(),
        Ok("1"),
        "explicit isolated fixture approval required"
    );
    let pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    admin::migrate(&pool, &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap())
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    std::fs::create_dir(repo.join("src")).unwrap();
    let fixture_source = repo.join("src/fixture.rs");
    std::fs::write(&fixture_source, "// Private read-only validator fixture.\n").unwrap();
    let original_source = std::fs::read(&fixture_source).unwrap();
    let _daemon = Daemon::start(
        &tagged_url(&runtime, &format!("native-contract-{}", Uuid::new_v4())),
        root.join("native-contract.sock"),
    )
    .await;
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrolled.auth);
    let workspace_key = format!("native-contract-{}", Uuid::new_v4());
    let mut client = Mcp::start(
        &_daemon.socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened=route(&mut client,"command","scope.open",json!({"request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],"candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
    let saved = save(&mut client, &opened["created"]["planning"], full_draft()).await;
    let reviewed = review(&mut client, &saved).await;
    let opened = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let begun=route(&mut client,"command","slice.pipeline.begin",json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],"slice_id":opened["created"]["id"],"slice_revision":opened["created"]["revision"],"definition_version":version,"qualification_reason":"Isolated native contract validator fixture."})).await;
    let mut c = begun["created"].clone();
    for _ in 0..3 {
        c = advance(&mut client, c).await;
    }
    assert_eq!(c["run"]["current_phase_id"], "slice-contract-writer");
    let blocked = completion(
        &c,
        "blocked_missing_artifact_contract",
        "waiting_input",
        "continue",
        None,
        None,
    );
    assert!(
        blocked["output"]["artifacts"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let reply = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        blocked,
    )
    .await;
    c = reply["context"].clone();
    let answer=route(&mut client,"command","slice.pipeline.input",json!({"request_id":Uuid::new_v4(),"run_id":c["run"]["id"],"run_revision":c["run"]["revision"],"phase_id":"slice-contract-writer","input":"Fixture owner authorizes source_edit src/fixture.rs only; no external effects."})).await;
    c = answer["context"].clone();
    let state = client.call("get_state", json!({})).await;
    let valid = declaration(&c, &state);
    let mut blocked_with_artifact = completion(
        &c,
        "blocked_missing_artifact_contract",
        "waiting_input",
        "continue",
        None,
        None,
    );
    blocked_with_artifact["output"]["artifacts"].as_array_mut().unwrap().push(json!({"name":"work-order-contract.json","media_type":"application/json","body":"","digest":""}));
    install_artifact(&mut blocked_with_artifact, &valid);
    // The existing generic artifact/disposition gate rejects this carrier before semantic validation.
    refuses_with_code_without_persistence(
        &mut client,
        &c,
        blocked_with_artifact,
        "INPUT_SCHEMA_INVALID",
    )
    .await;

    let mutations: Vec<fn(&mut Value)> = vec![
        |v| v["extra_key"] = json!(true),
        |v| {
            v.as_object_mut().unwrap().remove("source_checkpoint");
        },
        |v| v["session_declaration"]["workspace_id"] = json!(Uuid::new_v4()),
        |v| {
            let unknown = json!(Uuid::new_v4());
            for key in ["allowed_roots", "allowed_paths", "denied_roots"] {
                for p in v["write_scope"][key].as_array_mut().unwrap() {
                    p["source_id"] = unknown.clone();
                }
            }
            for scope in v["authority"]["scope"].as_array_mut().unwrap() {
                for p in scope["targets"].as_array_mut().unwrap() {
                    p["source_id"] = unknown.clone();
                }
            }
        },
        |v| v["authority"]["source"]["input_id"] = json!(Uuid::new_v4()),
        |v| v["required_reads"][0]["reference"]["digest"] = json!("f".repeat(64)),
        |v| v["refresh_resume"]["verify_before_hash"] = json!(false),
        |v| v["write_scope"]["allowed_paths"] = json!([]),
        |v| {
            let source = v["write_scope"]["allowed_roots"][0]["source_id"].clone();
            v["required_reads"].as_array_mut().unwrap().push(json!({"reference":{"kind":"source_file","source_id":source,"path":".git/config","content_sha256":"a".repeat(64)},"required":true}));
        },
        |v| {
            v["authority"]["scope"][0]["action"] = json!("source_plan");
            v["authority"]["scope"][0]["targets"][0]["path"] = json!("tect/workspace/state");
        },
        |v| {
            v["authority"]["scope"][0]["action"] = json!("source_test");
            v["authority"]["scope"][0]["targets"][0]["path"] = json!("src/.tect/state");
        },
    ];
    for mutate in mutations {
        let mut v = valid.clone();
        mutate(&mut v);
        let mut r = completion(&c, "contract_ready", "completed", "continue", None, None);
        install_artifact(&mut r, &v);
        refuses_without_persistence(&mut client, &c, r).await;
    }
    let mut wrong_version = valid.clone();
    wrong_version["target"]["definition_version"] = json!(if version.ends_with(".3") {
        "0.6.0-native.engineering.4"
    } else {
        "0.6.0-native.engineering.3"
    });
    let mut wrong_request = completion(&c, "contract_ready", "completed", "continue", None, None);
    install_artifact(&mut wrong_request, &wrong_version);
    refuses_without_persistence(&mut client, &c, wrong_request).await;
    // The successful lineage has actual owner input granting planning/testing only.
    let answer = route(&mut client, "command", "slice.pipeline.input", json!({
        "request_id":Uuid::new_v4(),"run_id":c["run"]["id"],"run_revision":c["run"]["revision"],
        "phase_id":"slice-contract-writer",
        "input":"Fixture owner authorizes source_plan and source_test src/fixture.rs only; no source edits, business writes or external effects. This replaces the earlier edit grant."})).await;
    c = answer["context"].clone();
    let mut read_only = declaration(&c, &state);
    read_only["write_scope"]["allowed_paths"] = json!([]);
    let target = read_only["authority"]["scope"][0]["targets"][0].clone();
    read_only["authority"]["scope"] = json!([
        {"action":"source_plan","targets":[target.clone()]},
        {"action":"source_test","targets":[target]}]);
    read_only["authority"]["limitations"] = json!(
        "Private fixture planning/testing only; no source edits, business writes or external effects."
    );
    let mut r = completion(&c, "contract_ready", "completed", "continue", None, None);
    r["output"]["fields"]["allowed_write_scope"] =
        json!("No write paths; registered source context bounds planning/testing only.");
    r["output"]["fields"]["authority_state"] =
        json!("Authorized source_plan and source_test only; source_edit is not authorized.");
    r["output"]["body"] = json!(
        "Read-only native fixture contract: planning/testing authority with zero allowed write paths."
    );
    install_artifact(&mut r, &read_only);
    let accepted = route(&mut client, "command", "slice.pipeline.phase.complete", r).await;
    c = accepted["context"].clone();
    assert_eq!(c["run"]["current_phase_ordinal"], 5);
    while c["run"]["current_phase_ordinal"].as_u64().unwrap() < 10 {
        c = advance(&mut client, c).await;
    }
    // Resume under a distinct authenticated native session on the same enrolled host.
    let mut resumed = Mcp::start(
        &_daemon.socket,
        &config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    resumed.call("open_workspace", json!({})).await;
    resumed
        .call(
            "select_worktrees",
            json!({"worktree_ids":[state["selected_worktrees"][0]["id"]]}),
        )
        .await;
    let resumed_state = resumed.call("get_state", json!({})).await;
    assert_ne!(resumed_state["session"]["id"], state["session"]["id"]);
    client.finish().await;
    client = resumed;
    c = current(&mut client, &c).await;
    let (v, o, t) = successful_route(&c);
    let mut missing = completion(&c, v, o, t, None, None);
    missing["consumed_outputs"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r["phase_id"] != "slice-contract-writer");
    refuses_without_persistence(&mut client, &c, missing).await;
    // Normal revision progression is accepted at P10: the P4 authoring revision remains old.
    c = advance(&mut client, c).await;
    assert_eq!(c["run"]["current_phase_ordinal"], 11);
    let phase4 = &c["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|b| b["phase_id"] == "slice-contract-writer")
        .unwrap()
        .clone();
    sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=true,stale_reason='isolated validator fixture' WHERE run_id=$1 AND phase_id='slice-contract-writer'").bind(support::id(&c["run"]["id"])).execute(&pool).await.unwrap();
    c = current(&mut client, &c).await;
    let (v, o, t) = successful_route(&c);
    refuses_without_persistence(&mut client, &c, completion(&c, v, o, t, None, None)).await;
    sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=false,stale_reason=NULL WHERE run_id=$1 AND phase_id='slice-contract-writer'").bind(support::id(&c["run"]["id"])).execute(&pool).await.unwrap();
    assert!(!phase4["output_digest"].as_str().unwrap().is_empty());
    c = current(&mut client, &c).await;
    sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=true,stale_reason='isolated read-ref fixture' WHERE run_id=$1 AND phase_id='slice-full-dev-entry-gate'").bind(support::id(&c["run"]["id"])).execute(&pool).await.unwrap();
    c = current(&mut client, &c).await;
    let (v, o, t) = successful_route(&c);
    refuses_without_persistence(&mut client, &c, completion(&c, v, o, t, None, None)).await;
    sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=false,stale_reason=NULL WHERE run_id=$1 AND phase_id='slice-full-dev-entry-gate'").bind(support::id(&c["run"]["id"])).execute(&pool).await.unwrap();
    c = current(&mut client, &c).await;
    c = advance(&mut client, c).await;
    c = advance(&mut client, c).await;
    assert_eq!(c["run"]["current_phase_ordinal"], 13);
    let (v, o, t) = successful_route(&c);
    let mut missing = completion(&c, v, o, t, None, None);
    missing["consumed_outputs"]
        .as_array_mut()
        .unwrap()
        .retain(|r| r["phase_id"] != "slice-contract-writer");
    refuses_without_persistence(&mut client, &c, missing).await;
    if version.ends_with(".4") {
        c = local_result::validate(&mut client, &pool, c).await;
        assert_eq!(c["run"]["current_phase_ordinal"], 18);
    }
    assert_eq!(std::fs::read(&fixture_source).unwrap(), original_source);
    client.finish().await;
}
