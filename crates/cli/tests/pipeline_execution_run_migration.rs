#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::Path;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn mapping() -> Value {
    json!([{"legacy_obligation_id":"legacy-phase-01",
        "successor_obligation_id":"slice-lightweight-k1",
        "evidence_refs":[{"reference":"artifact://migration/legacy-phase-01",
            "digest":"aaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaaa"}]}])
}

fn assert_complete_pipeline_refusal(value: &Value) {
    let refusal = &value["error"]["refusal"];
    for field in [
        "code",
        "message",
        "next_action",
        "required",
        "rule",
        "path",
        "expected",
        "actual",
    ] {
        assert!(
            refusal[field]
                .as_str()
                .is_some_and(|value| !value.is_empty()),
            "missing refusal.{field}: {value}"
        );
    }
}

fn v07_completion(context: &Value, fields: Value) -> Value {
    json!({
        "request_id":Uuid::new_v4(),
        "run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],
        "outcome":"completed",
        "transition":"continue",
        "output":{
            "producer_context_id":"pipeline-run-migration-public-mcp",
            "fields":fields,
            "verdict":"pass",
            "dispositions":["satisfied"]
        }
    })
}

async fn run_fixture(client: &mut Mcp, repo: &Path) -> (Value, Value) {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let opened_scope = route(
        client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(
        client,
        &opened_scope["created"]["planning"],
        lifecycle_support::lightweight_draft(),
    )
    .await;
    let reviewed = review(client, &saved).await;
    let opened_slice = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = opened_slice["created"].clone();
    let begun = route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "qualification_reason":"Legacy v0.6 run requires an explicit successor mapping."}),
    )
    .await;
    (begun["created"].clone(), reviewed["scope"]["id"].clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn pipeline_run_migration_is_atomic_idempotent_and_preserves_predecessor() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-run-migration.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-run-migration-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("pipeline-run-migration-{}", Uuid::new_v4()),
    )
    .await;

    let (context, _scope_id) = run_fixture(&mut client, &repo).await;
    let predecessor = context["run"].clone();
    assert!(
        !predecessor["definition_version"]
            .as_str()
            .unwrap()
            .starts_with("0.7")
    );
    let request_id = Uuid::new_v4();
    let idempotency_key = format!("migration-{}", Uuid::new_v4());
    let params = json!({"request_id":request_id,"predecessor_run_id":predecessor["id"],
        "expected_revision":predecessor["revision"],"idempotency_key":idempotency_key,
        "successor_definition_version":"0.7.0-native.k1k5","mappings":mapping()});
    let migrated = client.call("pipeline_run_migrate", params.clone()).await;
    assert_eq!(migrated["status"], "committed");
    assert_eq!(migrated["predecessor_run_id"], predecessor["id"]);
    assert_ne!(migrated["successor_run_id"], predecessor["id"]);
    assert_eq!(
        migrated["successor_definition_version"],
        "0.7.0-native.k1k5"
    );

    let old = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":predecessor["id"]}),
    )
    .await;
    assert_eq!(
        old["run"]["definition_version"],
        predecessor["definition_version"]
    );
    assert_eq!(
        old["run"]["definition_digest"],
        predecessor["definition_digest"]
    );
    assert_eq!(old["run"]["status"], "superseded");
    assert_eq!(
        old["run"]["revision"],
        predecessor["revision"].as_i64().unwrap() + 1
    );
    let successor = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":migrated["successor_run_id"]}),
    )
    .await;
    assert_eq!(successor["run"]["definition_version"], "0.7.0-native.k1k5");
    assert_eq!(successor["run"]["current_phase_id"], "K1");

    let k1 = v07_completion(
        &successor,
        json!({
            "fit":"bounded_understood","request":"exercise backend-derived proof",
            "parent":"current_confirmed","preflight":"current_clear",
            "authority":"authorized",
            "acceptance_checks":"K1 reaches K2 through the public MCP bridge",
            "route":"none"
        }),
    );
    assert!(k1.get("consumed_outputs").is_none());
    assert!(k1.get("consumed_inputs").is_none());
    assert!(k1["output"].get("body").is_none());

    let mut supplied = k1.clone();
    supplied["request_id"] = json!(Uuid::new_v4());
    supplied["consumed_outputs"] = json!([{
        "phase_id":"forged","output_revision":1,"digest":"caller-owned"
    }]);
    // Exercise the exact external tools/call envelope. This guards both the
    // stdio public decoder and the subsequent Unix-daemon decoder; an internal
    // route helper alone cannot detect a bridge normalization regression.
    let rejected_rpc = client
        .exchange(
            "tools/call",
            json!({"name":"command","arguments":{
                "route":"slice.pipeline.phase.complete","params":supplied
            }}),
        )
        .await;
    assert_eq!(rejected_rpc["result"]["isError"], true, "{rejected_rpc}");
    let rejected = recovery_support::tool_payload(&rejected_rpc);
    assert_complete_pipeline_refusal(&rejected);
    assert_eq!(
        rejected["error"]["refusal"]["code"],
        "BACKEND_DERIVED_PROOF_REQUIRED"
    );
    assert_eq!(rejected["error"]["refusal"]["rule"], "WP3-PROOF-01");
    assert_eq!(
        rejected["error"]["refusal"]["path"],
        "arguments.params.consumed_outputs"
    );

    let mut unknown = k1.clone();
    unknown["request_id"] = json!(Uuid::new_v4());
    unknown["caller_owned_proof"] = json!({"digest":"forged"});
    let unknown = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        unknown,
    )
    .await;
    assert_eq!(unknown["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
    assert_eq!(
        unknown["error"]["refusal"]["rule"],
        "WP6-SCHEMA-COMPLETE-01"
    );

    let completed_k1 = route(&mut client, "command", "slice.pipeline.phase.complete", k1).await;
    assert_eq!(completed_k1["context"]["run"]["current_phase_id"], "K2");
    let k2_context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":successor["run"]["id"],"refresh":true}),
    )
    .await;
    assert_eq!(k2_context["run"]["current_phase_id"], "K2");
    let persisted_k1 = k2_context["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["phase_id"] == "K1")
        .unwrap();
    assert!(persisted_k1.get("body").is_none());
    assert_eq!(
        persisted_k1["digest"],
        k2_context["bindings"][0]["output_digest"]
    );
    let raw_k1: (String, String) = sqlx::query_as(
        "SELECT body,body_digest FROM slice_pipeline_phase_outputs WHERE run_id=$1 AND phase_id='K1'",
    )
    .bind(successor["run"]["id"].as_str().unwrap().parse::<Uuid>().unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(raw_k1.0.is_empty());
    assert_eq!(raw_k1.1, persisted_k1["digest"]);

    let k2 = v07_completion(
        &k2_context,
        json!({
            "source_provenance":"Current source and contract read for the isolated migration fixture.",
            "worktree_provenance":"Temporary worktree owned by this disposable fixture.",
            "isolation":"confirmed","ownership":"confirmed","overlap":"clear",
            "target_proof_plan":"Public MCP flow proves migrated v0.7 K1 and K2 payload persistence.",
            "test_target":"pipeline_run_migration",
            "route":"none"
        }),
    );
    let completed_k2 = route(&mut client, "command", "slice.pipeline.phase.complete", k2).await;
    assert_eq!(completed_k2["context"]["run"]["current_phase_id"], "K3");
    let k2_attempt = completed_k2["context"]["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["phase_id"] == "K2")
        .unwrap();
    let k1_binding = completed_k2["context"]["bindings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|binding| binding["phase_id"] == "K1")
        .unwrap();
    assert!(k2_attempt["evidence_refs"].as_array().unwrap().iter().any(
        |reference| reference["kind"] == "output"
            && reference["reference"] == k1_binding["output_id"]
            && reference["digest"] == k1_binding["output_digest"]
    ));
    eprintln!(
        "v07-derived-proof refusal={} unknown_refusal={} omitted_k1=true omitted_k2=true k1_next={} k2_next={} derived_reference={}",
        rejected["error"],
        unknown["error"],
        k2_context["run"]["current_phase_id"],
        completed_k2["context"]["run"]["current_phase_id"],
        k1_binding["output_id"]
    );

    let replay = client.call("pipeline_run_migrate", params.clone()).await;
    assert_eq!(replay["status"], "replayed");
    assert_eq!(replay["successor_run_id"], migrated["successor_run_id"]);
    let mut conflict = params.clone();
    conflict["mappings"][0]["successor_obligation_id"] = json!("slice-lightweight-k2");
    let conflict = client.call_error("pipeline_run_migrate", conflict).await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    assert_complete_pipeline_refusal(&conflict);
    let mut stale = params.clone();
    stale["idempotency_key"] = json!(format!("stale-{}", Uuid::new_v4()));
    stale["expected_revision"] = json!(predecessor["revision"]);
    let stale = client.call_error("pipeline_run_migrate", stale).await;
    assert_eq!(stale["error"]["code"], "stale_revision");
    assert_complete_pipeline_refusal(&stale);
    let mut ambiguous = params.clone();
    ambiguous["idempotency_key"] = json!(format!("ambiguous-{}", Uuid::new_v4()));
    let duplicate_mapping = mapping()[0].clone();
    ambiguous["mappings"] = json!([duplicate_mapping.clone(), duplicate_mapping]);
    let ambiguous = client.call_error("pipeline_run_migrate", ambiguous).await;
    assert_eq!(
        ambiguous["error"]["refusal"]["code"],
        "LEGACY_MIGRATION_REQUIRED"
    );
    assert_complete_pipeline_refusal(&ambiguous);
    let mut missing = params;
    missing["idempotency_key"] = json!(format!("missing-{}", Uuid::new_v4()));
    missing["mappings"] = json!([]);
    let missing = client.call_error("pipeline_run_migrate", missing).await;
    assert_eq!(
        missing["error"]["refusal"]["code"],
        "LEGACY_MIGRATION_REQUIRED"
    );
    assert_complete_pipeline_refusal(&missing);
}
