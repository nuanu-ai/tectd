#![allow(dead_code)]

#[path = "pipeline_execution/lifecycle_support.rs"]
mod lifecycle_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use lifecycle_support::{completion_request, lightweight_draft, terminal};
use recovery_support::native_reads::ScopeOpenFixture;
use recovery_support::pipeline_reads::{
    ResolvedPipeline, read_pipeline_output, resolve_pipeline, resolve_pipeline_metadata,
};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::collections::BTreeSet;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

type StoredBindingRow = (
    String,
    i32,
    i64,
    Uuid,
    String,
    Option<String>,
    bool,
    Option<String>,
);

const LARGE_BODY_BYTES: usize = 1_750_000;

fn completion(context: &ResolvedPipeline, body: String) -> Value {
    assert_eq!(
        context.run()["definition_kind"],
        "slice.lightweight-tdd-development"
    );
    assert!(matches!(
        context.run()["definition_version"].as_str(),
        Some("0.7.1-native.k1k5" | "0.7.0-native.k1k5")
    ));
    let phase_id = context.run()["current_phase_id"].as_str().unwrap();
    let transition = if phase_id == "K5" {
        "complete"
    } else {
        "continue"
    };
    let terminal_result = (transition == "complete").then(|| terminal("Caller reports the structural capacity fixture complete; no actual source command executed."));
    let mut request = completion_request(context, "completed", transition, terminal_result, false);
    request["output"]["body"] = Value::String(body);
    request
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn large_current_context_falls_back_to_exact_immutable_output_reads() {
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
    let socket = root.join("pipeline-capacity.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-capacity-{}", Uuid::new_v4()),
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
        &format!("pipeline-capacity-{}", Uuid::new_v4()),
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened = route(
        &mut client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let opened = ScopeOpenFixture::from_mutation(opened, "created");
    let planning_read = opened.read_planning(&mut client).await;
    let saved = save(&mut client, &planning_read.value, lightweight_draft()).await;
    let reviewed = review(&mut client, &saved).await;
    let opened_slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let slice = &opened_slice["created"];
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":slice["id"],"slice_revision":slice["revision"],
            "delivery_mode":"phasewise",
            "qualification_reason":"Capacity fixture uses allowed phasewise delivery."}),
    )
    .await;
    let mut context = resolve_pipeline(&mut client, begun).await.unwrap();
    for index in 0..4 {
        let body = char::from(b'a' + index)
            .to_string()
            .repeat(LARGE_BODY_BYTES);
        let response = route(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            completion(&context, body),
        )
        .await;
        context = resolve_pipeline(&mut client, response).await.unwrap();
    }
    assert_eq!(context.run()["current_phase_id"], "K5");
    assert_eq!(context.run()["status"], "active");
    let oversized = completion(&context, "z".repeat(2 * 1024 * 1024));
    let oversized_error = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        oversized,
    )
    .await;
    assert_eq!(oversized_error["error"]["code"], "PAYLOAD_TOO_LARGE");
    let refusal = &oversized_error["error"]["refusal"];
    for (field, expected) in [
        ("code", "PAYLOAD_TOO_LARGE"),
        ("rule", "WP6-OUTPUT-SIZE-01"),
        ("path", "arguments.params.output"),
        ("expected", "at most 2097152 encoded bytes"),
        ("actual", "encoded output exceeds limit"),
        ("next_action", "reduce_output"),
        ("required", "pipeline_output"),
    ] {
        assert_eq!(refusal[field], expected, "payload refusal {field}");
    }
    let current = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context.run()["id"]}),
    )
    .await;
    assert_eq!(current["run"], *context.run());
    let unchanged = resolve_pipeline(&mut client, current).await.unwrap();
    for field in ["attempts", "outputs", "bindings"] {
        assert_eq!(
            unchanged.details_data()[field],
            context.details_data()[field],
            "oversized refusal changed {field}"
        );
    }
    assert_eq!(
        unchanged.details_data()["attempts"]
            .as_array()
            .unwrap()
            .len(),
        4
    );
    assert_eq!(
        unchanged.compact_context["output_availability"]["complete"],
        true
    );
    let response = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion(&context, "e".repeat(LARGE_BODY_BYTES)),
    )
    .await;
    let context = resolve_pipeline_metadata(&mut client, response)
        .await
        .unwrap();
    assert_eq!(context.run()["status"], "completed");
    assert!(
        context
            .run()
            .get("current_phase_id")
            .is_some_and(Value::is_null)
    );
    assert!(context.phase_contract.is_none());
    assert_eq!(
        context.compact_context["output_availability"]["complete"],
        true
    );
    assert!(context.compact_context.get("outputs").is_none());
    assert!(context.compact_context.get("bindings").is_none());
    for field in ["outputs", "bindings", "attempts"] {
        assert_eq!(context.compact_context["counts"][field], 5);
    }
    // Read-only owned test-PG oracle, distinct from native API-delivered History.
    let run_id = Uuid::parse_str(context.run()["id"].as_str().unwrap()).unwrap();
    let workspace_id: Uuid = sqlx::query_scalar(
        "SELECT workspace_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(run_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let stored_bindings: Vec<StoredBindingRow> = sqlx::query_as(
        "SELECT b.phase_id,b.phase_ordinal,b.output_revision,o.id,o.body_digest,o.reference,b.stale,b.stale_reason FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.run_id=b.run_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND NOT o.payload_erased ORDER BY b.phase_ordinal",
    ).bind(enrollment.tenant_id).bind(workspace_id).bind(run_id).fetch_all(&pool).await.unwrap();
    assert_eq!(stored_bindings.len(), 5);
    let attempts = context.history_data()["attempts"].as_array().unwrap();
    assert_eq!(attempts.len(), 5);
    assert_eq!(
        attempts
            .iter()
            .map(|attempt| attempt["phase_id"].as_str().unwrap())
            .collect::<Vec<_>>(),
        ["K1", "K2", "K3", "K4", "K5"]
    );
    let mut output_ids = BTreeSet::new();
    let mut output_digests = BTreeSet::new();
    for (index, (attempt, binding)) in attempts.iter().zip(&stored_bindings).enumerate() {
        assert_eq!(binding.0, attempt["phase_id"].as_str().unwrap());
        assert_eq!(
            i64::from(binding.1),
            attempt["phase_ordinal"].as_i64().unwrap()
        );
        assert_eq!(binding.2, attempt["output_revision"].as_i64().unwrap());
        assert_eq!(
            binding.3.to_string(),
            attempt["output_id"].as_str().unwrap()
        );
        assert_eq!(binding.4, attempt["output_digest"].as_str().unwrap());
        assert_eq!(json!(&binding.5), attempt["output_reference"]);
        assert!(!binding.6 && binding.7.is_none());
        assert_eq!(attempt["run_id"], context.run()["id"]);
        assert_eq!(attempt["outcome"], "completed");
        assert_eq!(attempt["attempt"], 1);
        assert_eq!(attempt["phase_ordinal"], index + 1);
        assert!(output_ids.insert(attempt["output_id"].as_str().unwrap()));
        assert!(output_digests.insert(attempt["output_digest"].as_str().unwrap()));
        let exact = read_pipeline_output(
            &mut client,
            run_id,
            Uuid::parse_str(attempt["output_id"].as_str().unwrap()).unwrap(),
            attempt["output_digest"].as_str().unwrap(),
        )
        .await
        .unwrap();
        assert_eq!(exact.value["id"], attempt["output_id"]);
        assert_eq!(exact.value["run_id"], context.run()["id"]);
        assert_eq!(exact.value["digest"], attempt["output_digest"]);
        assert_eq!(exact.value["phase_id"], attempt["phase_id"]);
        assert_eq!(exact.value["phase_ordinal"], attempt["phase_ordinal"]);
        assert_eq!(exact.value["revision"], attempt["output_revision"]);
        assert_eq!(exact.value["reference"], json!(&binding.5));
        assert_eq!(
            exact.value["body"].as_str().unwrap().len(),
            LARGE_BODY_BYTES
        );
        assert!(
            exact.value["body"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|byte| byte == b'a' + index as u8)
        );
        assert!(exact.provenance.representation_digest.is_some());
        assert!(exact.provenance.pages > 1);
    }
    recovery_support::finish_and_stop(client, &mut daemon).await;
}
