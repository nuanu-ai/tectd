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
    advance_create_to_review, begin_create_request, complete_review, context,
};
use knowledge_operation_support::{SingleOperation, commit_single};
use lifecycle_support::{consumed_inputs, consumed_outputs, lightweight_draft, phase_output};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, public_call, tagged_url,
    tool_payload,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

fn constraint(source_text: &str) -> Value {
    let mut fixture: Value = serde_json::from_str(include_str!(
        "../../postgres/src/knowledge_lifecycle/rdf/fixtures/general-constraint.json"
    ))
    .unwrap();
    fixture["document"]["canonical_text"] =
        json!("Every active phase must retain exact source provenance.");
    fixture["document"]["sources"][0]["snapshot"]["text"] = json!(source_text);
    fixture["document"].clone()
}

fn knowledge_ack(context: &Value) -> Value {
    json!({"manifest_id":context["knowledge_resources"]["id"],
        "digest":context["knowledge_resources"]["digest"]})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn durable_knowledge_lifecycle_is_bound_to_real_pipeline_and_access() {
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
        .expect("explicit DK activation must fail when the pinned native dependency is absent");

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-knowledge.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk2-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-knowledge-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    let (_source, _candidate) = ready_source_candidate(&mut client, &repo).await;

    let initial = route(&mut client, "query", "knowledge.context", json!({})).await;
    assert_eq!(initial["generation"], 0);
    assert_eq!(initial["capability"]["ready"], true);
    let injected = "Literal source: \"; DROP GRAPH <urn:attack>; # remains data.";
    let document = constraint(injected);
    let begin_id = Uuid::new_v4();
    let begin_request = begin_create_request(&document, json!({"kind":"workspace"}), begin_id);
    let begun = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_request.clone(),
    )
    .await;
    let change_id = context(&begun)["change_id"].clone();
    assert_eq!(
        context(
            &route(
                &mut client,
                "command",
                "knowledge.change_begin",
                begin_request.clone(),
            )
            .await
        )["run"]["id"],
        context(&begun)["run"]["id"]
    );
    let mut conflicting_begin = begin_request;
    conflicting_begin["intent"] = json!("Different intent under the same request identity.");
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_begin",
            conflicting_begin,
        )
        .await["error"]["code"],
        "input_conflict"
    );

    let reviewed = advance_create_to_review(&mut client, &document, begun).await;
    let reviewed = complete_review(&mut client, &reviewed, "ready").await;
    let publication = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        action_params(&reviewed["actions"][0]).clone(),
    )
    .await;
    let commit_request = action_params(&publication["actions"][0]).clone();
    let mut publisher_peer = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    publisher_peer.call("open_workspace", json!({})).await;
    let (raw_a, raw_b) = tokio::join!(
        client.exchange(
            "tools/call",
            public_call(
                "command",
                json!({"route":"knowledge.change_commit","params":commit_request.clone()}),
            ),
        ),
        publisher_peer.exchange(
            "tools/call",
            public_call(
                "command",
                json!({"route":"knowledge.change_commit","params":commit_request.clone()}),
            ),
        )
    );
    let payload_a = tool_payload(&raw_a);
    let payload_b = tool_payload(&raw_b);
    let (applied, replay) = if payload_a.get("applied").is_some() {
        (payload_a, payload_b)
    } else {
        (payload_b, payload_a)
    };
    assert_eq!(replay["replay"], applied["applied"]);
    let receipt = applied["applied"].clone();
    assert_eq!(receipt["workspace_generation"], 1);
    assert_eq!(
        route(
            &mut client,
            "command",
            "knowledge.change_commit",
            commit_request.clone(),
        )
        .await["replay"],
        receipt
    );
    publisher_peer.finish().await;
    let unit_id = receipt["applied_operations"][0]["unit_id"].clone();
    let exact = route(
        &mut client,
        "query",
        "knowledge.unit",
        json!({"unit_id":unit_id,"revision":1}),
    )
    .await;
    assert_eq!(exact["document"]["document"], document);
    assert_eq!(
        exact["document"]["document"]["sources"][0]["snapshot"]["text"],
        injected
    );
    assert!(exact["document"]["rdf_digest"].as_str().unwrap().len() >= 32);
    let cold = route(
        &mut client,
        "query",
        "knowledge.lifecycle",
        json!({"change_id":change_id,"view":"current"}),
    )
    .await;
    assert_eq!(context(&cold)["publisher_receipt"], receipt);

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
    let reviewed_scope = review(&mut client, &saved).await;
    let opened_slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(
            &reviewed_scope,
            &reviewed_scope["draft"]["nodes"][0],
            Uuid::new_v4(),
        ),
    )
    .await;
    let begun_pipeline = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),"scope_id":reviewed_scope["scope"]["id"],
            "slice_id":opened_slice["created"]["id"],
            "slice_revision":opened_slice["created"]["revision"],
            "qualification_reason":"Bounded generic knowledge pipeline fixture."}),
    )
    .await;
    let pipeline = &begun_pipeline["created"];
    assert_eq!(pipeline["knowledge_resource_status"]["state"], "current");
    assert_eq!(
        pipeline["knowledge_resources"]["selected"][0]["unit_id"],
        unit_id
    );

    let mut revised = document.clone();
    revised["canonical_text"] =
        json!("Every active phase must retain exact source and binding provenance.");
    let revised_sources = revised["sources"].clone();
    let revision = commit_single(
        &mut client,
        SingleOperation {
            operation: "revise",
            unit_id: Some(unit_id.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
            document: Some(revised),
            revalidation: None,
            successor: None,
            replacement_bindings: json!([]),
            sources: revised_sources,
            knowledge_kind: json!("constraint"),
            profiles: json!(["general"]),
            erasure: "not_required",
            authored_followup: false,
        },
    )
    .await;
    assert_eq!(revision["applied"]["workspace_generation"], 2);
    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":pipeline["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_resource_status"]["state"], "stale");
    let phase_id = stale["run"]["current_phase_id"].as_str().unwrap();
    let phase = stale["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == phase_id)
        .unwrap();
    let refused = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":Uuid::new_v4(),"run_id":stale["run"]["id"],
            "run_revision":stale["run"]["revision"],"phase_id":phase_id,
            "outcome":"completed","transition":"continue",
            "output":phase_output(phase,"stale-dk2","completed","continue"),
            "consumed_outputs":consumed_outputs(&stale),"consumed_inputs":consumed_inputs(&stale),
            "consumed_knowledge":knowledge_ack(pipeline),"publish_blocked_result":false}),
    )
    .await;
    assert_eq!(refused["error"]["code"], "context_changed");
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    route(
        &mut client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    let refreshed = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":pipeline["run"]["id"]}),
    )
    .await;
    assert_eq!(refreshed["knowledge_resource_status"]["state"], "current");
    assert_eq!(
        refreshed["knowledge_resources"]["selected"][0]["revision"],
        2
    );
    let phase_id = refreshed["run"]["current_phase_id"].as_str().unwrap();
    let phase = refreshed["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == phase_id)
        .unwrap();
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":Uuid::new_v4(),"run_id":refreshed["run"]["id"],
            "run_revision":refreshed["run"]["revision"],"phase_id":phase_id,
            "outcome":"completed","transition":"continue",
            "output":phase_output(phase,"current-dk2","completed","continue"),
            "consumed_outputs":consumed_outputs(&refreshed),"consumed_inputs":consumed_inputs(&refreshed),
            "consumed_knowledge":knowledge_ack(&refreshed),"publish_blocked_result":false}),
    )
    .await;
    assert_eq!(completed["context"]["run"]["current_phase_ordinal"], 2);

    let retraction = commit_single(
        &mut client,
        SingleOperation {
            operation: "retract",
            unit_id: Some(unit_id.clone()),
            expected_revision: Some(2),
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
        retraction["applied"]["applied_operations"][0]["operation"],
        "retract"
    );
    let gap = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":completed["context"]["run"]["id"]}),
    )
    .await;
    assert_eq!(gap["knowledge_resource_status"]["state"], "needs_context");

    let sibling = admin::enroll_host(&pool, Some(enrollment.tenant_id), Vec::new())
        .await
        .unwrap();
    let sibling_config = root.join("sibling.json");
    host_file(&sibling_config, &sibling.auth);
    let mut sibling_client = Mcp::start(
        &socket,
        &sibling_config,
        &Uuid::new_v4().to_string(),
        &format!("sibling-{}", Uuid::new_v4()),
    )
    .await;
    sibling_client.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut sibling_client,
            "query",
            "knowledge.unit",
            json!({"unit_id":unit_id}),
        )
        .await["error"]["code"],
        "not_found"
    );
    sibling_client.finish().await;
    let foreign = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let foreign_config = root.join("foreign.json");
    host_file(&foreign_config, &foreign.auth);
    let mut foreign_client = Mcp::start(
        &socket,
        &foreign_config,
        &Uuid::new_v4().to_string(),
        &format!("foreign-{}", Uuid::new_v4()),
    )
    .await;
    foreign_client.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut foreign_client,
            "query",
            "knowledge.unit",
            json!({"unit_id":unit_id}),
        )
        .await["error"]["code"],
        "not_found"
    );
    foreign_client.finish().await;

    let runtime_pool = PgPool::connect(&runtime).await.unwrap();
    assert!(
        sqlx::query("SELECT pgrdf.graph_id('urn:tect:dk:workspace:forbidden')")
            .execute(&runtime_pool)
            .await
            .is_err()
    );
    runtime_pool.close().await;
    let session_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM agent_sessions WHERE tenant_id=$1 AND native_session_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(&native)
    .fetch_one(&pool)
    .await
    .unwrap();
    admin::revoke_session(&pool, session_id).await.unwrap();
    assert_eq!(
        route_error(
            &mut client,
            "query",
            "knowledge.unit",
            json!({"unit_id":unit_id,"revision":1}),
        )
        .await["error"]["code"],
        "session_revoked"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_commit",
            commit_request,
        )
        .await["error"]["code"],
        "session_revoked"
    );
    client.finish().await;
    pool.close().await;
}
