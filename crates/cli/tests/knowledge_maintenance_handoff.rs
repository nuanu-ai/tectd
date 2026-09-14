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

use knowledge_lifecycle_support::{commit_create, complete_agent, context};
use knowledge_operation_support::{SingleOperation, ready_single_from_baseline_with_reviewed};
use recovery_support::{Daemon, Mcp, action_params, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use tect_application::WorkspaceService;
use tect_domain::{KnowledgeMaintenanceTaskState, RequestContext};
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn document(title: &str) -> Value {
    let mut value: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/runbook.json"
    ))
    .unwrap();
    value["document"]["title"] = json!(title);
    value["document"]["canonical_text"] = json!(format!("Canonical body for {title}."));
    value["document"]["sources"][0]["snapshot"]["uri"] =
        json!(format!("urn:dk4-maintenance-handoff:{title}"));
    value["document"]["sources"][0]["snapshot"]["text"] = json!(title);
    value["document"].clone()
}

fn revise(unit: Uuid, revision: i64, document: Value) -> SingleOperation {
    SingleOperation {
        operation: "revise",
        unit_id: Some(json!(unit)),
        expected_revision: Some(revision),
        expected_lifecycle: Some("active"),
        document: Some(document.clone()),
        revalidation: None,
        successor: None,
        replacement_bindings: json!([]),
        sources: document["sources"].clone(),
        knowledge_kind: json!("procedure"),
        profiles: json!(["general", "runbook"]),
        erasure: "not_required",
        authored_followup: false,
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn exhausted_task_handoff_requires_exact_reviewed_basis_and_resolves_once() {
    if std::env::var("TECT_TEST_DK4_MAINTENANCE").as_deref() != Ok("1") {
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
    support::repository(&root.join("source"));
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 12).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let workspace_key = format!("dk4-maintenance-handoff-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let request_context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native.clone(),
        workspace_key: workspace_key.clone(),
    };
    service.open_workspace(&request_context).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let socket = root.join("dk4-maintenance-handoff.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("dk4-maintenance-handoff-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;

    let original = document("handoff-original");
    let committed = commit_create(&mut client, original).await;
    let unit = committed.receipt["applied_operations"][0]["unit_id"]
        .as_str()
        .unwrap()
        .to_owned();
    let unit_id = Uuid::parse_str(&unit).unwrap();
    let (source_iri, accepted_digest, source_uri): (String, String, String) = sqlx::query_as(
        "SELECT e.event_payload#>>'{resolved_sources,0,pin,source_iri}', \
         e.event_payload#>>'{resolved_sources,0,pin,digest}', \
         e.event_payload#>>'{resolved_sources,0,uri}' \
         FROM knowledge_revisions r JOIN knowledge_publication_events e \
          ON e.id=r.publication_event_id WHERE r.unit_id=$1 AND r.revision=1",
    )
    .bind(unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let observed_digest = "3d748f34f7a214c4c606373671fe56dac440ee97ebd58fe3c66cc3daf25b2832";
    let observed = support::route(
        &mut client,
        "command",
        "knowledge.maintenance_observe",
        json!({"request_id":Uuid::new_v4(),"unit_id":unit,"unit_revision":1,
            "basis":{"kind":"source_changed","source_iri":source_iri,
                "accepted_digest":accepted_digest,"observed_digest":observed_digest}}),
    )
    .await;
    let task = &observed["created"];
    let task_id = task["id"].clone();
    let basis_digest = task["signal"]["basis_digest"].clone();
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state='exhausted',attempts=5, \
         failure_code='storage_unavailable' WHERE id=$1",
    )
    .bind(Uuid::parse_str(task_id.as_str().unwrap()).unwrap())
    .execute(&pool)
    .await
    .unwrap();

    let maintenance = support::route(
        &mut client,
        "query",
        "knowledge.maintenance",
        json!({"unit_id":unit,"states":["exhausted"],"limit":10}),
    )
    .await;
    assert_eq!(maintenance["tasks"][0]["attempts"], 5);
    assert_eq!(
        maintenance["tasks"][0]["failure_code"],
        "storage_unavailable"
    );
    let maintenance_body = include_str!("../../host/knowledge-methods/maintenance.md");
    assert_eq!(
        maintenance["method"]["id"],
        "tect:knowledge-maintenance:method"
    );
    assert_eq!(maintenance["method"]["version"], "0.4.0-dk4.1");
    assert_eq!(
        maintenance["method"]["digest"],
        "7c2ea809977c5bc9053bf632824943d773b6834cd10eb753416ef1a969e7d42d"
    );
    assert_eq!(maintenance["method"]["body"], maintenance_body);
    let action = &maintenance["actions"][0];
    assert_eq!(action["kind"], "needs_context");
    let mut begin = action["arguments"]["params"].clone();
    let request_id = begin["request_id"].clone();
    let mut replacement = document("handoff-revised");
    replacement["sources"][0]["snapshot"]["uri"] = json!(source_uri);
    begin["change"]["intent"] =
        json!("Resolve this exact exhausted maintenance task through reviewed revision.");
    begin["change"]["desired_outcome"] =
        json!("Publish one exact reviewed revision resolving the maintenance basis.");
    begin["change"]["sources"] = replacement["sources"].clone();
    begin["change"]["operation_hints"] = json!([{
        "client_label":"maintenance-revision","operation":"revise","unit_id":unit,
        "expected_revision":1,"expected_lifecycle":"active",
        "reason":"The exact maintenance basis requires a reviewed revision.",
        "authority_basis":"Current authenticated workspace owner."
    }]);
    begin["change"]["completion"] = json!({"canonical_result":true,"exact_delivery":true,
        "impact_recorded":true,"search":"not_required","erasure":"not_required"});
    begin["change"]["delivery_mode"] = json!("phasewise");
    let begun = support::route(
        &mut client,
        "command",
        "knowledge.maintenance_begin",
        begin.clone(),
    )
    .await;
    let change_id = begun["created"]["change"]["created"]["change_id"].clone();
    let change = support::route(
        &mut client,
        "query",
        "knowledge.lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )
    .await;
    assert_eq!(begun["created"]["task"]["attempts"], 5);
    assert_eq!(begun["created"]["task"]["state"], "linked");
    assert_eq!(context(&change)["maintenance_tasks"][0]["id"], task_id);
    let origin = &context(&change)["origin"];
    let baseline = complete_agent(
        &mut client,
        &change,
        json!({"phase":"kc-intake","data":{"bounded_outcome":origin["desired_outcome"],
            "operation_hints":origin["operation_hints"],
            "authority_boundary":"Current authenticated workspace owner.",
            "completion":origin["completion"]}}),
    )
    .await;
    let baseline_current = support::route(
        &mut client,
        "query",
        "knowledge.lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )
    .await;
    assert_eq!(
        context(&baseline)["candidate_baseline"],
        context(&baseline_current)["candidate_baseline"]
    );
    let publication = ready_single_from_baseline_with_reviewed(
        &mut client,
        revise(unit_id, 1, replacement),
        baseline_current,
        std::slice::from_ref(&basis_digest),
    )
    .await;
    let committed = support::route(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&publication["actions"][0]).clone(),
    )
    .await;
    assert_eq!(committed["applied"]["applied_operations"][0]["revision"], 2);
    let replacement_source_iri: String = sqlx::query_scalar(
        "SELECT e.event_payload#>>'{resolved_sources,0,pin,source_iri}' \
         FROM knowledge_revisions r JOIN knowledge_publication_events e \
          ON e.id=r.publication_event_id WHERE r.unit_id=$1 AND r.revision=2",
    )
    .bind(unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_ne!(replacement_source_iri, source_iri);
    let resolved = support::route(
        &mut client,
        "query",
        "knowledge.maintenance",
        json!({"unit_id":unit,"states":["resolved"],"limit":10}),
    )
    .await;
    assert_eq!(resolved["tasks"][0]["id"], task_id);
    assert_eq!(resolved["tasks"][0]["attempts"], 5);
    assert_eq!(
        resolved["tasks"][0]["terminal_evidence"]["unit_revision"],
        2
    );
    assert_eq!(
        resolved["tasks"][0]["terminal_evidence"]["change_id"],
        committed["applied"]["change_id"]
    );

    let replay = support::route(
        &mut client,
        "command",
        "knowledge.maintenance_begin",
        begin.clone(),
    )
    .await;
    assert_eq!(replay["replay"]["task"]["id"], task_id);
    assert_eq!(
        context(&replay["replay"]["change"])["change_id"],
        committed["applied"]["change_id"]
    );
    let pending: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_maintenance_tasks WHERE id=$1 AND state IN ('pending','leased')",
    )
    .bind(Uuid::parse_str(task_id.as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(pending, 0);
    begin["change"]["intent"] = json!("Conflicting replay must fail.");
    let error =
        support::route_error(&mut client, "command", "knowledge.maintenance_begin", begin).await;
    assert_eq!(error["error"]["code"], "input_conflict");

    let (source_iri_v2, accepted_digest_v2): (String, String) = sqlx::query_as(
        "SELECT e.event_payload#>>'{resolved_sources,0,pin,source_iri}', \
         e.event_payload#>>'{resolved_sources,0,pin,digest}' \
         FROM knowledge_revisions r JOIN knowledge_publication_events e \
          ON e.id=r.publication_event_id WHERE r.unit_id=$1 AND r.revision=2",
    )
    .bind(unit_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    let wrong_observation = support::route(
        &mut client,
        "command",
        "knowledge.maintenance_observe",
        json!({"request_id":Uuid::new_v4(),"unit_id":unit,"unit_revision":2,
            "basis":{"kind":"source_changed","source_iri":source_iri_v2,
                "accepted_digest":accepted_digest_v2,
                "observed_digest":"eed83898905320c14831f7fad09f67f7040d3cb320b4146c812eee903a381177"}}),
    )
    .await;
    let wrong_task_id = wrong_observation["created"]["id"].clone();
    sqlx::query(
        "UPDATE knowledge_maintenance_tasks SET state='exhausted',attempts=5, \
         failure_code='storage_unavailable' WHERE id=$1",
    )
    .bind(Uuid::parse_str(wrong_task_id.as_str().unwrap()).unwrap())
    .execute(&pool)
    .await
    .unwrap();
    let wrong_context = support::route(
        &mut client,
        "query",
        "knowledge.maintenance",
        json!({"unit_id":unit,"states":["exhausted"],"limit":10}),
    )
    .await;
    let mut wrong_begin = wrong_context["actions"][0]["arguments"]["params"].clone();
    let mut wrong_document = document("handoff-wrong-uri");
    wrong_document["sources"][0]["snapshot"]["uri"] = json!("urn:wrong-logical-source");
    wrong_begin["change"]["intent"] = json!("Test the exact logical source identity guard.");
    wrong_begin["change"]["desired_outcome"] =
        json!("Reject a replacement whose digest matches but logical source URI does not.");
    wrong_begin["change"]["sources"] = wrong_document["sources"].clone();
    wrong_begin["change"]["operation_hints"] = json!([{
        "client_label":"wrong-logical-source","operation":"revise","unit_id":unit,
        "expected_revision":2,"expected_lifecycle":"active",
        "reason":"Exercise the logical source identity guard.",
        "authority_basis":"Current authenticated workspace owner."
    }]);
    wrong_begin["change"]["completion"] = json!({"canonical_result":true,
        "exact_delivery":true,"impact_recorded":true,"search":"not_required",
        "erasure":"not_required"});
    wrong_begin["change"]["delivery_mode"] = json!("phasewise");
    let wrong_change = support::route(
        &mut client,
        "command",
        "knowledge.maintenance_begin",
        wrong_begin,
    )
    .await;
    let wrong_change_id = wrong_change["created"]["change"]["created"]["change_id"].clone();
    let mut wrong_current = support::route(
        &mut client,
        "query",
        "knowledge.lifecycle",
        json!({"change_id":wrong_change_id,"view":"current"}),
    )
    .await;
    let wrong_origin = &context(&wrong_current)["origin"];
    wrong_current = complete_agent(
        &mut client,
        &wrong_current,
        json!({"phase":"kc-intake","data":{"bounded_outcome":wrong_origin["desired_outcome"],
            "operation_hints":wrong_origin["operation_hints"],
            "authority_boundary":"Current authenticated workspace owner.",
            "completion":wrong_origin["completion"]}}),
    )
    .await;
    let wrong_publication = ready_single_from_baseline_with_reviewed(
        &mut client,
        revise(unit_id, 2, wrong_document),
        wrong_current,
        &[wrong_observation["created"]["signal"]["basis_digest"].clone()],
    )
    .await;
    let wrong_commit = support::route_error(
        &mut client,
        "command",
        "knowledge.change_commit",
        action_params(&wrong_publication["actions"][0]).clone(),
    )
    .await;
    assert_eq!(wrong_commit["error"]["code"], "needs_context");
    let current_revision: i64 =
        sqlx::query_scalar("SELECT accepted_revision FROM knowledge_unit_heads WHERE unit_id=$1")
            .bind(unit_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(current_revision, 2);

    let state: KnowledgeMaintenanceTaskState =
        serde_json::from_value(resolved["tasks"][0]["state"].clone()).unwrap();
    assert_eq!(state, KnowledgeMaintenanceTaskState::Resolved);
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    pool.close().await;
    let _ = request_id;
}
