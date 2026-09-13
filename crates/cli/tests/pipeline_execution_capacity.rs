#![allow(dead_code)]

#[path = "pipeline_execution/lifecycle_support.rs"]
mod lifecycle_support;
mod recovery_support;
#[path = "native_planning/support.rs"]
mod support;

use lifecycle_support::{lightweight_draft, phase_output};
use recovery_support::{
    Daemon, Mcp, action_name, action_params, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

const LARGE_BODY_BYTES: usize = 1_750_000;

fn consumed_bindings(context: &Value) -> Value {
    let current = context["run"]["current_phase_ordinal"].as_u64().unwrap();
    Value::Array(
        context["bindings"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|binding| {
                binding["stale"] == false && binding["phase_ordinal"].as_u64().unwrap() < current
            })
            .map(|binding| {
                json!({"phase_id":binding["phase_id"],
                    "output_revision":binding["output_revision"],
                    "digest":binding["output_digest"]})
            })
            .collect(),
    )
}

fn completion(context: &Value, body: String) -> Value {
    let phase_id = context["run"]["current_phase_id"].as_str().unwrap();
    let phase = &context["definition"]["phases"][0];
    let route = phase["verdict_routes"]
        .as_array()
        .unwrap()
        .iter()
        .find(|route| route["outcome"] == "completed" && route["transition"] == "continue")
        .unwrap();
    let mut output = phase_output(phase, phase_id, "completed", "continue");
    output["body"] = Value::String(body);
    let mut request = json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],"phase_id":phase_id,
        "outcome":"completed","transition":"continue","output":output,
        "consumed_outputs":consumed_bindings(context),"consumed_inputs":[],
        "publish_blocked_result":false});
    if let Some(revisit) = route["revisit_to"].as_array().and_then(|ids| ids.first()) {
        request["revisit_phase_id"] = revisit.clone();
    }
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
    let saved = save(
        &mut client,
        &opened["created"]["planning"],
        lightweight_draft(),
    )
    .await;
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
    let mut context = begun["created"].clone();
    let mut final_actions = Value::Null;
    for index in 0..5 {
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
        final_actions = response["actions"].clone();
        context = response["context"].clone();
    }
    assert_eq!(context["outputs_complete"], false);
    assert!(context["outputs"].as_array().unwrap().is_empty());
    assert_eq!(context["bindings"].as_array().unwrap().len(), 5);
    let output_actions = final_actions
        .as_array()
        .unwrap()
        .iter()
        .filter(|action| {
            action_name(action) == Some("slice.pipeline.context")
                && action_params(action)["view"] == "output"
        })
        .count();
    assert_eq!(output_actions, 5);

    for (index, binding) in context["bindings"].as_array().unwrap().iter().enumerate() {
        let exact = route(
            &mut client,
            "query",
            "slice.pipeline.context",
            json!({"run_id":context["run"]["id"],"view":"output",
                "output_id":binding["output_id"],"digest":binding["output_digest"]}),
        )
        .await;
        assert_eq!(exact["digest"], binding["output_digest"]);
        assert_eq!(exact["body"].as_str().unwrap().len(), LARGE_BODY_BYTES);
        assert!(
            exact["body"]
                .as_str()
                .unwrap()
                .bytes()
                .all(|byte| byte == b'a' + index as u8)
        );
    }

    let revision = context["run"]["revision"].clone();
    let oversized = completion(&context, "z".repeat(2 * 1024 * 1024));
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            oversized
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let current = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(current["run"]["revision"], revision);
    assert_eq!(current["attempts"].as_array().unwrap().len(), 5);
    assert_eq!(current["outputs_complete"], false);
}
