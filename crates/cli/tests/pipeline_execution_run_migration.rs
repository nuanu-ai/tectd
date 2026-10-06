#[path = "pipeline_execution/full_support.rs"]
#[allow(dead_code)]
mod full_support;
#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use projection::assert_slice_projection;
use recovery_support::native_reads::ScopeOpenFixture;
use recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
#[path = "pipeline_execution_run_migration/historical_manifest.rs"]
mod historical_manifest;
#[path = "pipeline_execution_run_migration/historical_seed.rs"]
mod historical_seed;
#[path = "pipeline_execution_run_migration/refresh_refusal.rs"]
mod refresh_refusal;
use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::Path;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
#[path = "pipeline_execution_run_migration/projection.rs"]
mod projection;
use uuid::Uuid;

fn mapping(context: &ResolvedPipeline) -> Value {
    let output = &context.details_data()["outputs"][0];
    for field in ["phase_id", "id", "digest"] {
        assert!(
            output[field]
                .as_str()
                .is_some_and(|value| !value.is_empty())
        );
    }
    json!([{"legacy_obligation_id":output["phase_id"],
        "successor_obligation_id":"K1",
        "evidence_refs":[{"reference":output["id"],"digest":output["digest"]}]}])
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

use historical_seed::v07_completion;

async fn run_fixture(client: &mut Mcp, repo: &Path, pool: &PgPool) -> (ResolvedPipeline, Value) {
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
    let opened_scope = ScopeOpenFixture::from_mutation(opened_scope, "created");
    let planning = opened_scope.read_planning(client).await.value;
    let saved = save(client, &planning, lifecycle_support::lightweight_draft()).await;
    let reviewed = review(client, &saved).await;
    let opened_slice = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = opened_slice["created"].clone();
    assert_slice_projection(
        client,
        &reviewed["scope"]["id"],
        &slice["id"],
        &Value::Null,
        "not_started",
    )
    .await;
    let run_id = historical_seed::seed(pool, client, &reviewed["scope"]["id"], &slice).await;
    let raw = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id}),
    )
    .await;
    let context = resolve_pipeline(client, raw)
        .await
        .expect("resolve stored historical run");
    assert_slice_projection(
        client,
        &reviewed["scope"]["id"],
        &slice["id"],
        &json!(run_id),
        "active",
    )
    .await;
    (context, reviewed["scope"]["id"].clone())
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn pipeline_run_migration_is_atomic_idempotent_and_preserves_predecessor() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = if std::env::var("TECT_TEST_KEEP_FAILURE_EVIDENCE").as_deref() == Ok("1") {
        temp.keep().canonicalize().unwrap()
    } else {
        temp.path().canonicalize().unwrap()
    };
    eprintln!("migration_fixture_root={}", root.display());
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-run-migration.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-run-migration-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
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

    let (context, scope_id) = run_fixture(&mut client, &repo, &pool).await;
    refresh_refusal::retired(&pool, &mut client, &context).await;
    let predecessor = context.run().clone();
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
        "successor_definition_version":"0.7.1-native.k1k5","mappings":[]});
    historical_seed::assert_retired_boundaries(&pool, &mut client, &context, &params).await;
    let before = historical_seed::rows(
        &pool,
        Uuid::parse_str(predecessor["id"].as_str().unwrap()).unwrap(),
    )
    .await;
    let mut nonempty = params.clone();
    nonempty["mappings"] = mapping(&context);
    let denied = client.call_error("pipeline_run_migrate", nonempty).await;
    assert_eq!(
        denied["error"]["refusal"]["code"],
        "LEGACY_MIGRATION_REQUIRED"
    );
    assert_eq!(
        denied["error"]["refusal"]["rule"],
        "WP6-MIGRATION-MAPPING-01"
    );
    assert_eq!(
        denied["error"]["refusal"]["path"],
        "arguments.params.mappings"
    );
    assert_eq!(
        before,
        historical_seed::rows(
            &pool,
            Uuid::parse_str(predecessor["id"].as_str().unwrap()).unwrap()
        )
        .await
    );
    let mut stale = params.clone();
    stale["idempotency_key"] = json!(format!("stale-{}", Uuid::new_v4()));
    stale["expected_revision"] = json!(predecessor["revision"].as_i64().unwrap() - 1);
    let stale = client.call_error("pipeline_run_migrate", stale).await;
    assert_eq!(stale["error"]["code"], "stale_revision");
    assert_complete_pipeline_refusal(&stale);
    let migrated = client.call("pipeline_run_migrate", params.clone()).await;
    assert_eq!(migrated["status"], "committed");
    assert_eq!(migrated["predecessor_run_id"], predecessor["id"]);
    assert_ne!(migrated["successor_run_id"], predecessor["id"]);
    assert_eq!(
        migrated["successor_definition_version"],
        "0.7.1-native.k1k5"
    );

    assert_slice_projection(
        &mut client,
        &scope_id,
        &predecessor["slice_id"],
        &migrated["successor_run_id"],
        "active",
    )
    .await;

    let old = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":predecessor["id"]}),
    )
    .await;
    let old = resolve_pipeline(&mut client, old)
        .await
        .expect("resolve superseded historical run");
    assert_eq!(
        old.run()["definition_version"],
        predecessor["definition_version"]
    );
    assert_eq!(
        old.run()["definition_digest"],
        predecessor["definition_digest"]
    );
    assert_eq!(old.run()["status"], "superseded");
    historical_seed::assert_superseded_write(&pool, &mut client, &old).await;
    assert_eq!(
        old.run()["revision"],
        predecessor["revision"].as_i64().unwrap() + 1
    );
    let mut successor = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":migrated["successor_run_id"]}),
    )
    .await;
    if let Some(action) = successor["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|action| action_name(action) == Some("pipeline.knowledge_refresh"))
    {
        route(
            &mut client,
            "command",
            "pipeline.knowledge_refresh",
            action_params(action).clone(),
        )
        .await;
        successor = route(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":migrated["successor_run_id"]}),
        )
        .await;
    }
    let successor = resolve_pipeline(&mut client, successor)
        .await
        .expect("resolve current successor");
    for field in ["attempts", "outputs", "bindings"] {
        assert!(
            successor.details_data()[field]
                .as_array()
                .unwrap()
                .is_empty()
        );
    }
    historical_seed::assert_retained(
        &pool,
        &before,
        Uuid::parse_str(predecessor["id"].as_str().unwrap()).unwrap(),
    )
    .await;
    assert_eq!(successor.run()["definition_version"], "0.7.1-native.k1k5");
    assert_eq!(successor.run()["current_phase_id"], "K1");

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
    let completed_k1 = resolve_pipeline(&mut client, completed_k1)
        .await
        .expect("resolve K1 completion");
    assert_eq!(completed_k1.run()["current_phase_id"], "K2");
    let k2_context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":successor.run()["id"],"refresh":true}),
    )
    .await;
    let k2_context = resolve_pipeline(&mut client, k2_context)
        .await
        .expect("resolve actual K2 current");
    assert_eq!(k2_context.run()["current_phase_id"], "K2");
    let persisted_k1 = k2_context.details_data()["outputs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|output| output["phase_id"] == "K1")
        .unwrap();
    assert!(persisted_k1.get("body").is_none());
    assert_eq!(
        persisted_k1["digest"],
        k2_context.details_data()["bindings"][0]["output_digest"]
    );
    let raw_k1: (String, String) = sqlx::query_as(
        "SELECT body,body_digest FROM slice_pipeline_phase_outputs WHERE run_id=$1 AND phase_id='K1'",
    )
    .bind(successor.run()["id"].as_str().unwrap().parse::<Uuid>().unwrap())
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
    let completed_k2 = resolve_pipeline(&mut client, completed_k2)
        .await
        .expect("resolve K2 completion");
    assert_eq!(completed_k2.run()["current_phase_id"], "K3");
    let k2_attempt = completed_k2.details_data()["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["phase_id"] == "K2")
        .unwrap();
    let k1_binding = completed_k2.details_data()["bindings"]
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
        k2_context.run()["current_phase_id"],
        completed_k2.run()["current_phase_id"],
        k1_binding["output_id"]
    );

    let replay = client.call("pipeline_run_migrate", params.clone()).await;
    assert_eq!(replay["status"], "replayed");
    assert_eq!(replay["successor_run_id"], migrated["successor_run_id"]);
    let mut conflict = params.clone();
    conflict["mappings"] = mapping(&context);
    let conflict = client.call_error("pipeline_run_migrate", conflict).await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    assert_complete_pipeline_refusal(&conflict);
    let mut ambiguous = params.clone();
    ambiguous["idempotency_key"] = json!(format!("ambiguous-{}", Uuid::new_v4()));
    let duplicate_mapping = mapping(&context)[0].clone();
    ambiguous["mappings"] = json!([duplicate_mapping.clone(), duplicate_mapping]);
    let ambiguous = client.call_error("pipeline_run_migrate", ambiguous).await;
    assert_eq!(
        ambiguous["error"]["refusal"]["code"],
        "LEGACY_MIGRATION_REQUIRED"
    );
    assert_complete_pipeline_refusal(&ambiguous);

    historical_seed::assert_old_receipt_replay(
        &pool,
        &mut client,
        &root,
        &migrated["successor_run_id"],
    )
    .await;
    refresh_refusal::current_replay_and_superseded(
        &pool,
        &mut client,
        &migrated["successor_run_id"],
    )
    .await;
    projection::assert_persisted_status_boundaries(
        &pool,
        &mut client,
        &scope_id,
        &predecessor["slice_id"],
        &migrated["successor_run_id"],
    )
    .await;
    recovery_support::finish_and_stop(client, &mut daemon).await;
}

#[path = "pipeline_execution_run_migration/full_engineering.rs"]
mod full_engineering;

#[path = "pipeline_execution_run_migration/current_binding.rs"]
mod current_binding;
