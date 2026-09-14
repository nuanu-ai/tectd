#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{
    advance_create_to_baseline, advance_create_to_prepare, begin_create_request, commit_create,
    complete_agent, complete_review, context, create_prepare_params, output_data,
    run_manifest_checkpoint,
};
use knowledge_operation_support::{SingleOperation, commit_single};
use lifecycle_support::{complete, lightweight_draft};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn bound_constraint(binding: Value, statement: &str) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    fixture["document"]["canonical_text"] = json!(statement);
    fixture["document"]["bindings"][0]["target"] = binding;
    fixture["document"].clone()
}

async fn binding_prepare(client: &mut Mcp, document: &Value) -> (Value, Value) {
    let request = begin_create_request(document, json!({"kind":"workspace"}), Uuid::new_v4());
    let begun = route(client, "command", "knowledge.change_begin", request).await;
    let current = advance_create_to_baseline(client, begun).await;
    let candidate = context(&current)["candidate_baseline"].clone();
    let current = complete_agent(
        client,
        &current,
        json!({"phase":"kc-resolve-baseline","data":candidate}),
    )
    .await;
    let current = advance_create_to_prepare(client, document, current).await;
    let params = create_prepare_params(&current, document);
    (current, params)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn exact_slice_phase_binding_is_validated_and_does_not_flow_to_next_phase() {
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
    let pipeline_context = &begun["created"];
    assert!(
        pipeline_context["knowledge"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let binding = json!({"kind":"slice_phase","scope_id":reviewed["scope"]["id"],
        "slice_id":opened_slice["created"]["id"],"phase_id":pipeline_context["run"]["current_phase_id"]});

    let document = bound_constraint(
        binding,
        "Only the exact bound phase must retain source provenance.",
    );
    let (_prepared_context, prepare_params) = binding_prepare(&mut client, &document).await;
    let mut wrong_scope = prepare_params.clone();
    wrong_scope["request_id"] = json!(Uuid::new_v4());
    wrong_scope["output"]["data"]["data"]["operations"][0]["document"]["bindings"][0]["target"]["scope_id"] =
        json!(Uuid::new_v4());
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_phase_complete",
            wrong_scope,
        )
        .await["error"]["code"],
        "forbidden"
    );
    let mut missing_phase = prepare_params.clone();
    missing_phase["request_id"] = json!(Uuid::new_v4());
    missing_phase["output"]["data"]["data"]["operations"][0]["document"]["bindings"][0]["target"]
        ["phase_id"] = json!("not-a-real-phase");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_phase_complete",
            missing_phase,
        )
        .await["error"]["code"],
        "invalid_arguments"
    );
    let mut current_change = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        prepare_params,
    )
    .await;
    let change = context(&current_change);
    let digest = output_data(change, "kc-prepare-change")["digest"].clone();
    let receipts = change["plan"]["obligations"]
        .as_array()
        .unwrap()
        .iter()
        .map(|obligation| {
            let methods = obligation["method_refs"]
                .as_array()
                .unwrap()
                .iter()
                .map(|method| {
                    json!({"instruction_id":method["id"],"version":method["version"],
                    "digest":method["digest"]})
                })
                .collect::<Vec<_>>();
            json!({"operation_id":obligation["operation_id"],
                "profile_id":obligation["profile_id"],"obligation_id":obligation["obligation_id"],
                "disposition":"satisfied","reason":"Exact binding and source checked.",
                "changeset_digest":digest,"method_reads":methods,"input_digests":[]})
        })
        .collect::<Vec<_>>();
    current_change = complete_agent(
        &mut client,
        &current_change,
        json!({"phase":"kc-domain-checks",
        "data":{"receipts":receipts,"unresolved_obligation_ids":[]}}),
    )
    .await;
    let impact = context(&current_change)["candidate_impact"].clone();
    current_change = complete_agent(
        &mut client,
        &current_change,
        json!({"phase":"kc-impact-plan","data":impact}),
    )
    .await;
    current_change = complete_review(&mut client, &current_change, "ready").await;
    let publication = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&current_change["actions"][0]).clone(),
    )
    .await;
    let committed = route(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await;
    assert_eq!(committed["applied"]["workspace_generation"], 1);
    let unit_id = Uuid::parse_str(
        committed["applied"]["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let (definition_kind, definition_digest): (String, String) = sqlx::query_as(
        "SELECT definition_kind,definition_digest FROM knowledge_bindings WHERE unit_id=$1",
    )
    .bind(unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(definition_kind, "slice.lightweight-tdd-development");
    assert_eq!(definition_digest, pipeline_context["definition"]["digest"]);
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
        json!({"run_id":pipeline_context["run"]["id"]}),
    )
    .await;
    assert_eq!(
        changed_pin["knowledge_resource_status"]["state"],
        "needs_context"
    );
    let refused_refresh = find_action(&changed_pin, "pipeline.knowledge_refresh").unwrap();
    let run_id = Uuid::parse_str(pipeline_context["run"]["id"].as_str().unwrap()).unwrap();
    let before_refusal = run_manifest_checkpoint(&pool, run_id).await;
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "pipeline.knowledge_refresh",
            action_params(refused_refresh).clone(),
        )
        .await["error"]["code"],
        "needs_context"
    );
    assert_eq!(run_manifest_checkpoint(&pool, run_id).await, before_refusal);
    sqlx::query("UPDATE knowledge_bindings SET definition_digest=$2 WHERE unit_id=$1")
        .bind(unit_id)
        .bind(pipeline_context["definition"]["digest"].as_str().unwrap())
        .execute(&pool)
        .await
        .unwrap();

    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":pipeline_context["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
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
        json!({"run_id":pipeline_context["run"]["id"]}),
    )
    .await;
    assert_eq!(
        current["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .len(),
        1
    );
    assert!(
        current["knowledge_resources"]["selected"][0]
            .get("source")
            .is_none()
    );
    let (completed, _) =
        complete(&mut client, &current, "completed", "continue", None, false).await;
    let next = &completed["context"];
    assert_eq!(next["run"]["current_phase_ordinal"], 2);
    assert!(
        next["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let action = find_action(&completed, "slice.pipeline.phase.complete").unwrap();
    assert!(action_params(action).get("consumed_knowledge").is_none());

    client.finish().await;
    pool.close().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn erased_origin_begin_replay_refuses_the_frozen_knowledge_copy() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("erased-origin.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk2-erased-origin-{}", Uuid::new_v4()),
    );
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("origin-{}", Uuid::new_v4()),
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope=route(&mut client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],"candidate_revision":candidate["revision"]})).await;
    let mut pipeline_draft = lightweight_draft();
    pipeline_draft["nodes"][0]["pipeline"] = json!("slice.custom-procedure-capture");
    pipeline_draft["nodes"][0]["pipeline_reason"] =
        json!("Exercise frozen origin replay without invoking Promotion routing.");
    let saved = save(
        &mut client,
        &opened_scope["created"]["planning"],
        pipeline_draft,
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
    let fixture: serde_json::Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    let committed = commit_create(&mut client, fixture["document"].clone()).await;
    let unit = committed.receipt["applied_operations"][0]["unit_id"].clone();
    let persisted_pipeline: String =
        sqlx::query_scalar("SELECT pipeline FROM native_slices WHERE id=$1")
            .bind(Uuid::parse_str(opened_slice["created"]["id"].as_str().unwrap()).unwrap())
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(persisted_pipeline, "slice.custom-procedure-capture");
    let begin_request = json!({"request_id":Uuid::new_v4(),"scope_id":reviewed["scope"]["id"],
        "slice_id":opened_slice["created"]["id"],"slice_revision":opened_slice["created"]["revision"],
        "qualification_reason":"Frozen erased-origin replay fixture."});
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        begin_request.clone(),
    )
    .await;
    assert_eq!(
        begun["created"]["knowledge_resources"]["selected"][0]["unit_id"],
        unit
    );
    let origin_manifest = begun["created"]["knowledge_resources"]["id"].clone();
    let retracted = commit_single(
        &mut client,
        SingleOperation {
            operation: "retract",
            unit_id: Some(unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        retracted["applied"]["applied_operations"][0]["operation"],
        "retract"
    );
    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":begun["created"]["run"]["id"]}),
    )
    .await;
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    let refreshed = route(
        &mut client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    assert_ne!(refreshed["refreshed"]["id"], origin_manifest);
    assert!(
        refreshed["refreshed"]["selected"]
            .as_array()
            .unwrap()
            .is_empty()
    );
    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(unit),
            expected_revision: Some(1),
            expected_lifecycle: Some("retracted"),
            document: None,
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: json!([]),
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "owned_live_copies",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(
        erased["applied_erased"]["operations"][0]["state"],
        "payload_erased"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "slice.pipeline.begin",
            begin_request
        )
        .await["error"]["code"],
        "knowledge_payload_erased"
    );
    client.finish().await;
    pool.close().await;
}
