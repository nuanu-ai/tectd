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
    // Ordinary input must verify the selected publication before committing,
    // and an idempotent replay must still verify its current context.
    let input_request = json!({"request_id":Uuid::new_v4(),"run_id":current["run"]["id"],
        "run_revision":current["run"]["revision"],"phase_id":current["run"]["current_phase_id"],
        "input":"Retain exact source provenance in this phase."});
    let input = route(
        &mut client,
        "command",
        "slice.pipeline.input",
        input_request.clone(),
    )
    .await;
    let current = input["context"].clone();
    assert_eq!(current["knowledge_resource_status"]["state"], "current");
    assert_eq!(
        current["knowledge_resources"]["run_revision"],
        current["run"]["revision"]
    );
    let (event_id, event_payload): (Uuid, Value) = sqlx::query_as(
        "SELECT e.id,e.event_payload FROM knowledge_publication_events e \
         JOIN knowledge_revisions r ON r.tenant_id=e.tenant_id AND r.workspace_id=e.workspace_id \
         AND r.publication_event_id=e.id WHERE r.unit_id=$1 AND r.revision=1",
    )
    .bind(unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    // This is the newly enrolled fixture's own publication, never shared data.
    sqlx::query("UPDATE knowledge_publication_events SET event_payload=pg_catalog.jsonb_set(event_payload,'{event_id}',pg_catalog.to_jsonb($3::text)) WHERE id=$1 AND unit_id=$2")
        .bind(event_id)
        .bind(unit_id)
        .bind(Uuid::new_v4().to_string())
        .execute(&pool)
        .await
        .unwrap();
    let before_corruption_refusal = run_manifest_checkpoint(&pool, run_id).await;
    let inputs_before: i64 =
        sqlx::query_scalar("SELECT count(*) FROM slice_pipeline_inputs WHERE run_id=$1")
            .bind(run_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    let fresh_input = json!({"request_id":Uuid::new_v4(),"run_id":current["run"]["id"],
        "run_revision":current["run"]["revision"],"phase_id":current["run"]["current_phase_id"],
        "input":"Fresh input must refuse corrupted selected publication."});
    for request in [fresh_input, input_request.clone()] {
        assert_eq!(
            route_error(&mut client, "command", "slice.pipeline.input", request).await["error"]["code"],
            "internal_invariant"
        );
        assert_eq!(
            run_manifest_checkpoint(&pool, run_id).await,
            before_corruption_refusal
        );
        let inputs_after: i64 =
            sqlx::query_scalar("SELECT count(*) FROM slice_pipeline_inputs WHERE run_id=$1")
                .bind(run_id)
                .fetch_one(&pool)
                .await
                .unwrap();
        assert_eq!(inputs_after, inputs_before);
    }
    sqlx::query(
        "UPDATE knowledge_publication_events SET event_payload=$3 WHERE id=$1 AND unit_id=$2",
    )
    .bind(event_id)
    .bind(unit_id)
    .bind(event_payload)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(
        route(
            &mut client,
            "command",
            "slice.pipeline.input",
            input_request
        )
        .await,
        input
    );
    // Whole-delivery mutations return compact state; completion requires an
    // explicit reread of the current definition after the new input revision.
    let completion_context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":current["run"]["id"],"refresh":true}),
    )
    .await;
    assert_eq!(completion_context["run"]["id"], current["run"]["id"]);
    assert_eq!(
        completion_context["run"]["revision"],
        current["run"]["revision"]
    );
    for phases in [
        &completion_context["definition"]["phases"],
        &completion_context["delivered_phases"],
    ] {
        assert!(
            phases
                .as_array()
                .unwrap()
                .iter()
                .any(|phase| phase["id"] == completion_context["run"]["current_phase_id"])
        );
    }
    let (completed, _) = complete(
        &mut client,
        &completion_context,
        "completed",
        "continue",
        None,
        false,
    )
    .await;
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

#[path = "pipeline_execution_knowledge_binding/erased_origin_replay.rs"]
mod erased_origin_replay;
