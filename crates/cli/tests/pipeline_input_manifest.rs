#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use lifecycle_support::{complete, completion_request, lightweight_draft};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn input_request(context: &Value, text: &str) -> Value {
    json!({"request_id":Uuid::new_v4(),"run_id":context["run"]["id"],
        "run_revision":context["run"]["revision"],
        "phase_id":context["run"]["current_phase_id"],"input":text})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn ordinary_input_rebinds_current_manifest_and_completes_from_returned_context() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated DK admin URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("dedicated DK runtime URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("dedicated DK runtime role required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-input-manifest.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-input-manifest-{}", Uuid::new_v4()),
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
        &format!("pipeline-input-manifest-{}", Uuid::new_v4()),
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],
        "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(
        &mut client,
        &opened_scope["created"]["planning"],
        lightweight_draft(),
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({
        "request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":slice["created"]["id"],"slice_revision":slice["created"]["revision"],
        "delivery_mode":"phasewise",
        "qualification_reason":"Regression for input manifest revision binding."}),
    )
    .await;
    let initial = begun["created"].clone();
    assert_eq!(initial["knowledge_resource_status"]["state"], "current");
    assert!(
        initial["definition"]["phases"]
            .as_array()
            .unwrap()
            .iter()
            .any(|phase| phase["id"] == initial["run"]["current_phase_id"]),
        "initial phase absent: {initial}"
    );
    let (phase_one, _) =
        complete(&mut client, &initial, "completed", "continue", None, false).await;
    assert!(
        phase_one["context"].is_object(),
        "phase one response: {phase_one}"
    );
    let phase_two = phase_one["context"].clone();
    let (waiting, _) = complete(
        &mut client,
        &phase_two,
        "waiting_input",
        "continue",
        None,
        false,
    )
    .await;
    assert!(
        waiting["context"].is_object(),
        "waiting response: {waiting}"
    );
    let waiting = waiting["context"].clone();
    assert_eq!(waiting["run"]["status"], "waiting_input");

    let first_request = input_request(&waiting, "First bounded operator input.");
    let first = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        first_request.clone(),
    )
    .await;
    let first_context = &first["context"];
    assert_eq!(
        first_context["knowledge_resource_status"]["state"],
        "current"
    );
    assert_eq!(
        first_context["knowledge_resources"]["run_revision"],
        first_context["run"]["revision"]
    );
    assert_ne!(
        first_context["knowledge_resources"]["id"],
        waiting["knowledge_resources"]["id"]
    );
    let replay = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        first_request,
    )
    .await;
    assert_eq!(replay, first);
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.input",
            input_request(&waiting, "Stale operator input.")
        )
        .await["error"]["code"],
        "stale_revision"
    );

    let second = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        input_request(first_context, "Second bounded operator input."),
    )
    .await;
    let current = &second["context"];
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    assert_eq!(
        current["knowledge_resources"]["run_revision"],
        current["run"]["revision"]
    );
    assert_ne!(
        current["knowledge_resources"]["id"],
        first_context["knowledge_resources"]["id"]
    );
    assert_eq!(current["inputs"].as_array().unwrap().len(), 2);
    let stale_completion = completion_request(first_context, "completed", "continue", None, false);
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            stale_completion
        )
        .await["error"]["code"],
        "stale_revision"
    );
    let (completed, request) =
        complete(&mut client, current, "completed", "continue", None, false).await;
    assert_eq!(completed["context"]["run"]["current_phase_ordinal"], 3);
    assert_eq!(
        route(
            &mut client,
            "command",
            "slice.pipeline.phase.complete",
            request
        )
        .await,
        completed
    );
}
