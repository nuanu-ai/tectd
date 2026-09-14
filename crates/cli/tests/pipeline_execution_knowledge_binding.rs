#[path = "pipeline_execution/knowledge_test_support.rs"]
#[allow(dead_code)]
mod knowledge_test_support;
#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_test_support::{approve, draft, enabled, prepare, publish};
use lifecycle_support::{complete, lightweight_draft};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::json;
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn exact_slice_phase_binding_is_validated_and_does_not_flow_to_next_phase() {
    if !enabled() {
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
        .expect("explicit DK activation must validate the pinned native dependency");

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("knowledge-binding.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk-binding-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("knowledge-binding-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope = route(
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
        &opened_scope["created"]["planning"],
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
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
            "slice_id":opened_slice["created"]["id"],"slice_revision":opened_slice["created"]["revision"],
            "qualification_reason":"Exact Slice-phase DK binding fixture."}),
    )
    .await;
    let context = &begun["created"];
    assert!(
        context["knowledge"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let binding = json!({"kind":"slice_phase","scope_id":reviewed["scope"]["id"],
        "slice_id":opened_slice["created"]["id"],"phase_id":context["run"]["current_phase_id"]});

    let invalid_bindings = [
        (
            json!({"kind":"slice_phase","scope_id":Uuid::new_v4(),
                "slice_id":opened_slice["created"]["id"],"phase_id":context["run"]["current_phase_id"]}),
            "forbidden",
        ),
        (
            json!({"kind":"slice_phase","scope_id":reviewed["scope"]["id"],
                "slice_id":opened_slice["created"]["id"],"phase_id":"not-a-real-phase"}),
            "invalid_arguments",
        ),
    ];
    for (invalid_binding, expected_code) in invalid_bindings {
        let error = route_error(
            &mut client,
            "command",
            "knowledge.change_prepare",
            json!({"request_id":Uuid::new_v4(),"operation":"create","expected_generation":0,
                "draft":draft(invalid_binding,"Only the exact bound phase must retain provenance.","Exact source."),
                "reason":"Reject invalid binding.","authority_basis":"Authenticated owner fixture."}),
        )
        .await;
        assert_eq!(error["error"]["code"], expected_code);
    }

    let (prepared, _) = prepare(
        &mut client,
        "create",
        0,
        None,
        None,
        Some(draft(
            binding,
            "Only the exact bound phase must retain source provenance.",
            "Exact source.",
        )),
    )
    .await;
    let change = &prepared["prepared"];
    assert_eq!(
        change["binding_provenance"]["definition_kind"],
        "slice.lightweight-tdd-development"
    );
    assert_eq!(
        change["binding_provenance"]["definition_digest"],
        context["definition"]["digest"]
    );
    let (approved, _) = approve(&mut client, change).await;
    let (published, _) = publish(&mut client, &approved["approved"]).await;
    assert_eq!(published["published"]["workspace_generation"], 1);
    let unit_id = Uuid::parse_str(published["published"]["unit_id"].as_str().unwrap()).unwrap();
    sqlx::query(
        "UPDATE knowledge_bindings SET definition_digest='changed-definition-pin' WHERE unit_id=$1",
    )
    .bind(unit_id)
    .execute(&pool)
    .await
    .unwrap();
    let changed_pin = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(changed_pin["knowledge_status"]["state"], "needs_context");
    assert_eq!(
        changed_pin["knowledge_status"]["changed_unit_ids"],
        json!([unit_id])
    );
    sqlx::query("UPDATE knowledge_bindings SET definition_digest=$2 WHERE unit_id=$1")
        .bind(unit_id)
        .bind(context["definition"]["digest"].as_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();

    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_status"]["state"], "stale");
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    route(
        &mut client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    let current = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(
        current["knowledge"]["selected"].as_array().unwrap().len(),
        1
    );
    assert!(current["knowledge"]["selected"][0].get("source").is_none());
    let (completed, _) =
        complete(&mut client, &current, "completed", "continue", None, false).await;
    let next = &completed["context"];
    assert_eq!(next["run"]["current_phase_ordinal"], 2);
    assert!(next["knowledge"]["selected"].as_array().unwrap().is_empty());
    let action = find_action(&completed, "slice.pipeline.phase.complete").unwrap();
    assert!(action_params(action).get("consumed_knowledge").is_none());

    client.finish().await;
    pool.close().await;
}
