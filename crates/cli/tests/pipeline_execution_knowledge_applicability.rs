#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::commit_create;
use knowledge_operation_support::{SingleOperation, commit_single};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, tagged_url,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, save};
use tect_postgres::admin;
use uuid::Uuid;

fn draft() -> Value {
    json!({"coverage_summary":"Exercise exact generic binding applicability.","nodes":[{
        "kind":"work","identity":{"local":"applicability"},"title":"Consume exact bindings",
        "outcome":"Only bindings matching this Program, Scope, Slice and phase are selected",
        "includes":["typed knowledge"],"excludes":["implicit repin"],"dependencies":[],
        "proof":["Exact captured resource manifest"],"pipeline":"slice.custom-procedure-capture",
        "pipeline_reason":"Exercise exact generic binding selection","source_result_ids":[]}],
        "supersessions":[]})
}

fn document(label: &str, bindings: Value) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    value["document"]["title"] = json!(label);
    value["document"]["sources"][0]["snapshot"]["uri"] = json!(format!("urn:{label}"));
    value["document"]["sources"][0]["snapshot"]["text"] = json!(label);
    value["document"]["bindings"] = bindings;
    value["document"].clone()
}

async fn open_target(client: &mut Mcp, repo: &std::path::Path) -> (Value, Value) {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope=route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(client, &scope["created"]["planning"], draft()).await;
    let reviewed = review(client, &saved).await;
    let slice = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    (reviewed["scope"].clone(), slice["created"].clone())
}

async fn begin(client: &mut Mcp, scope: &Value, slice: &Value) -> Value {
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
        "scope_id":scope["id"],"slice_id":slice["id"],"slice_revision":slice["revision"],
        "delivery_mode":"phasewise","qualification_reason":"Exact applicability fixture."}),
    )
    .await["created"]
        .clone()
}

async fn refresh(client: &mut Mcp, context: &Value) -> Value {
    let stale = route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    let action = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    let params = action_params(action);
    assert_eq!(params["run_id"], stale["run"]["id"]);
    assert_eq!(params["run_revision"], stale["run"]["revision"]);
    assert_eq!(params["phase_id"], stale["run"]["current_phase_id"]);
    route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        params.clone(),
    )
    .await;
    route(
        client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await
}

fn binding(target: Value, purpose: &str, resolution: Value) -> Value {
    json!({"target":target,"purpose":purpose,"version_resolution":resolution})
}

fn selected_for<'a>(context: &'a Value, unit: &Value) -> Vec<&'a Value> {
    context["knowledge_resources"]["selected"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|value| value["unit_id"] == *unit)
        .collect()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn generic_binding_applicability_freshness_and_supersession_are_exact() {
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
    let socket = root.join("applicability.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk2-applicability-{}", Uuid::new_v4()),
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
        &format!("dk2-applicability-{}", Uuid::new_v4()),
    )
    .await;
    let (scope, slice) = open_target(&mut client, &repo).await;
    let scope_id = Uuid::parse_str(scope["id"].as_str().unwrap()).unwrap();
    let slice_id = Uuid::parse_str(slice["id"].as_str().unwrap()).unwrap();
    let program_id: Uuid = sqlx::query_scalar(
        "SELECT sc.program_id FROM native_scopes ns JOIN scope_candidate_sets sc \
         ON sc.tenant_id=ns.tenant_id AND sc.workspace_id=ns.workspace_id \
         AND sc.id=ns.source_candidate_set_id WHERE ns.id=$1",
    )
    .bind(scope_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let current = json!({"kind":"current_accepted"});
    let bindings = json!([
        binding(
            json!({"kind":"program","program_id":program_id}),
            "reference",
            current.clone()
        ),
        binding(
            json!({"kind":"scope","scope_id":scope_id}),
            "reference",
            current.clone()
        ),
        binding(
            json!({"kind":"slice","scope_id":scope_id,"slice_id":slice_id}),
            "reference",
            current.clone()
        )
    ]);
    let mut applicable_document = document("applicable-future-review-due", bindings);
    applicable_document["valid_from"] = json!("2020-01-01T00:00:00Z");
    applicable_document["valid_until"] = json!("2035-01-01T00:00:00Z");
    applicable_document["review_due_at"] = json!("2021-01-01T00:00:00Z");
    let applicable = commit_create(&mut client, applicable_document).await;
    let applicable_unit = applicable.receipt["applied_operations"][0]["unit_id"].clone();
    let captured = begin(&mut client, &scope, &slice).await;
    let selected = selected_for(&captured, &applicable_unit);
    assert_eq!(selected.len(), 3);
    let advertised = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":captured["run"]["id"]}),
    )
    .await;
    assert_eq!(advertised["run"]["id"], captured["run"]["id"]);
    assert_eq!(advertised["run"]["revision"], captured["run"]["revision"]);
    assert_eq!(
        advertised["knowledge_resources"]["id"],
        captured["knowledge_resources"]["id"]
    );
    let completion = find_action(&advertised, "slice.pipeline.phase.complete").unwrap();
    assert_eq!(
        action_params(completion)["consumed_knowledge"],
        json!({"manifest_id":captured["knowledge_resources"]["id"],
            "digest":captured["knowledge_resources"]["digest"]})
    );
    let kinds = selected
        .iter()
        .map(|value| value["binding"]["target"]["kind"].as_str().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(kinds, ["program", "scope", "slice"].into_iter().collect());
    assert!(
        captured["knowledge_resources"]["freshness_warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value
                == &json!(format!("review_due:{}", applicable_unit.as_str().unwrap())))
    );

    let mut future = document(
        "not-yet-valid-reference",
        json!([binding(
            json!({"kind":"workspace"}),
            "reference",
            current.clone()
        )]),
    );
    future["valid_from"] = json!("2035-01-01T00:00:00Z");
    let future = commit_create(&mut client, future).await;
    let future_unit = future.receipt["applied_operations"][0]["unit_id"].clone();
    let future_context = refresh(&mut client, &captured).await;
    assert!(selected_for(&future_context, &future_unit).is_empty());
    assert!(
        future_context["knowledge_resources"]["freshness_warnings"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "optional_resource_expired")
    );

    let workspace = binding(json!({"kind":"workspace"}), "required", current.clone());
    let slice_binding = binding(
        json!({"kind":"slice","scope_id":scope_id,"slice_id":slice_id}),
        "required",
        current.clone(),
    );
    let predecessor_document = document(
        "partial-predecessor",
        json!([workspace.clone(), slice_binding.clone()]),
    );
    let predecessor = commit_create(&mut client, predecessor_document).await;
    let predecessor_unit = predecessor.receipt["applied_operations"][0]["unit_id"].clone();
    let first_successor = commit_create(
        &mut client,
        document("workspace-successor", json!([workspace.clone()])),
    )
    .await;
    let first_successor_unit = first_successor.receipt["applied_operations"][0]["unit_id"].clone();
    commit_single(
        &mut client,
        SingleOperation {
            operation: "supersede",
            unit_id: Some(predecessor_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: Some(json!({"unit_id":first_successor_unit})),
            replacement_bindings: json!([workspace]),
            sources: json!([]),
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    let partial = refresh(&mut client, &future_context).await;
    let remaining = selected_for(&partial, &predecessor_unit);
    assert_eq!(remaining.len(), 1);
    assert_eq!(remaining[0]["binding"]["target"]["kind"], "slice");
    assert_eq!(selected_for(&partial, &first_successor_unit).len(), 1);

    let second_successor = commit_create(
        &mut client,
        document("slice-successor", json!([slice_binding.clone()])),
    )
    .await;
    let second_successor_unit =
        second_successor.receipt["applied_operations"][0]["unit_id"].clone();
    commit_single(
        &mut client,
        SingleOperation {
            operation: "supersede",
            unit_id: Some(predecessor_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: Some(json!({"unit_id":second_successor_unit})),
            replacement_bindings: json!([slice_binding]),
            sources: json!([]),
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    let full = refresh(&mut client, &partial).await;
    assert!(selected_for(&full, &predecessor_unit).is_empty());
    assert_eq!(selected_for(&full, &first_successor_unit).len(), 1);
    assert_eq!(selected_for(&full, &second_successor_unit).len(), 1);

    let pinned_binding = binding(
        json!({"kind":"workspace"}),
        "required",
        json!({"kind":"pinned_revision","revision":1}),
    );
    let current_binding = binding(json!({"kind":"workspace"}), "required", current.clone());
    let pinned_document = document(
        "immutable-pinned-predecessor",
        json!([pinned_binding.clone(), current_binding.clone()]),
    );
    let pinned = commit_create(&mut client, pinned_document.clone()).await;
    let pinned_unit = pinned.receipt["applied_operations"][0]["unit_id"].clone();
    let mut revision_two = pinned_document.clone();
    revision_two["title"] = json!("immutable pin revision two");
    let sources = revision_two["sources"].clone();
    commit_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(pinned_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(revision_two),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources,
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    let pinned_current = refresh(&mut client, &full).await;
    let revisions = selected_for(&pinned_current, &pinned_unit)
        .into_iter()
        .map(|value| value["revision"].as_i64().unwrap())
        .collect::<std::collections::BTreeSet<_>>();
    assert_eq!(revisions, [1, 2].into_iter().collect());

    let pinned_successor = commit_create(
        &mut client,
        document("pinned-current-successor", json!([current_binding.clone()])),
    )
    .await;
    let pinned_successor_unit =
        pinned_successor.receipt["applied_operations"][0]["unit_id"].clone();
    commit_single(
        &mut client,
        SingleOperation {
            operation: "supersede",
            unit_id: Some(pinned_unit.clone()),
            expected_revision: Some(2),
            expected_lifecycle: Some("active"),
            document: None,
            revalidation: None,
            successor: Some(json!({"unit_id":pinned_successor_unit})),
            replacement_bindings: json!([current_binding]),
            sources: json!([]),
            knowledge_kind: json!("procedure"),
            profiles: json!(["general", "runbook"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    let superseded_pin = refresh(&mut client, &pinned_current).await;
    let retained = selected_for(&superseded_pin, &pinned_unit);
    assert_eq!(retained.len(), 1);
    assert_eq!(retained[0]["revision"], 1);
    assert_eq!(
        selected_for(&superseded_pin, &pinned_successor_unit).len(),
        1
    );
    let run_id = Uuid::parse_str(superseded_pin["run"]["id"].as_str().unwrap()).unwrap();
    let current_manifest = Uuid::parse_str(
        superseded_pin["knowledge_resources"]["id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    let consumer_states: Vec<(Uuid, bool)> = sqlx::query_as(
        "SELECT m.id,COALESCE(pg_catalog.bool_or(c.active),false) \
         FROM pipeline_knowledge_manifests m LEFT JOIN knowledge_maintenance_consumers c \
          ON c.tenant_id=m.tenant_id AND c.workspace_id=m.workspace_id \
         AND c.relation_name='pipeline_knowledge_manifests' AND c.row_id=m.id \
         WHERE m.run_id=$1 GROUP BY m.id ORDER BY m.created_at,m.id",
    )
    .bind(run_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert!(consumer_states.len() > 1);
    assert!(consumer_states.contains(&(current_manifest, true)));
    assert!(
        consumer_states
            .iter()
            .all(|(manifest, active)| *manifest == current_manifest || !active)
    );

    let (unrelated_scope, unrelated_slice) = open_target(&mut client, &repo).await;
    let unrelated = begin(&mut client, &unrelated_scope, &unrelated_slice).await;
    assert!(selected_for(&unrelated, &applicable_unit).is_empty());
    client.finish().await;
    pool.close().await;
}
