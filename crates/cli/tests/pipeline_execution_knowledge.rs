#[path = "pipeline_execution/knowledge_backup_support.rs"]
mod knowledge_backup_support;
#[path = "pipeline_execution/knowledge_test_support.rs"]
mod knowledge_test_support;
#[path = "pipeline_execution/lifecycle_support.rs"]
#[allow(dead_code)]
mod lifecycle_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_test_support::{approve, enabled, prepare, publish, workspace_draft};
use lifecycle_support::{
    complete, consumed_inputs, consumed_outputs, lightweight_draft, phase_output,
};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, public_call, tagged_url,
    tool_payload,
};
use serde_json::json;
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn durable_knowledge_lifecycle_is_bound_to_real_pipeline_and_access() {
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
        .expect("explicit DK activation must fail when the pinned native dependency is absent");

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-knowledge.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-dk-{}", Uuid::new_v4()));
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-knowledge-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;

    let initial = route(&mut client, "query", "knowledge.context", json!({})).await;
    assert_eq!(initial["generation"], 0);
    assert_eq!(initial["capability"]["ready"], true);
    let injected = "Literal source: \"; DROP GRAPH <urn:attack>; # remains data.";
    let (prepared, prepare_request) = prepare(
        &mut client,
        "create",
        0,
        None,
        None,
        Some(workspace_draft(
            "Every active phase must retain exact source provenance.",
            injected,
        )),
    )
    .await;
    let change = &prepared["prepared"];
    assert_eq!(change["stage"], "review_required");
    assert_eq!(
        route(
            &mut client,
            "command",
            "knowledge.change_prepare",
            prepare_request.clone()
        )
        .await["replay"],
        *change
    );
    let mut conflict = prepare_request;
    conflict["reason"] = json!("Different payload under the same request identity.");
    assert_eq!(
        route_error(&mut client, "command", "knowledge.change_prepare", conflict).await["error"]["code"],
        "input_conflict"
    );

    let mut wrong_method = json!({
        "request_id":Uuid::new_v4(),"change_id":change["id"],
        "change_revision":change["change_revision"],"proposal_digest":change["proposal_digest"],
        "verdict":"approve","review_summary":"Exact semantic review.",
        "method_read":{"id":change["review_method"]["id"],
            "version":change["review_method"]["version"],"digest":"wrong-proof"}
    });
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_review",
            wrong_method.take()
        )
        .await["error"]["code"],
        "stale_context"
    );
    let (approved, _) = approve(&mut client, change).await;
    let ready = &approved["approved"];
    let mut publisher_peer = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    publisher_peer.call("open_workspace", json!({})).await;
    let publish_a = json!({"request_id":Uuid::new_v4(),"change_id":ready["id"],
        "change_revision":ready["change_revision"],"proposal_digest":ready["proposal_digest"]});
    let publish_b = publish_a.clone();
    let (raw_a, raw_b) = tokio::join!(
        client.exchange(
            "tools/call",
            public_call(
                "command",
                json!({"route":"knowledge.change_publish","params":publish_a.clone()})
            )
        ),
        publisher_peer.exchange(
            "tools/call",
            public_call(
                "command",
                json!({"route":"knowledge.change_publish","params":publish_b.clone()})
            )
        )
    );
    let payload_a = tool_payload(&raw_a);
    let payload_b = tool_payload(&raw_b);
    let (published, replayed) = if payload_a.get("published").is_some() {
        (payload_a, payload_b)
    } else {
        (payload_b, payload_a)
    };
    assert!(published.get("published").is_some());
    assert_eq!(replayed["replay"], published["published"]);
    let publish_request = publish_a;
    let receipt = &published["published"];
    assert_eq!(receipt["workspace_generation"], 1);
    assert_eq!(receipt["delivery_eligible"], true);
    let unit_id = receipt["unit_id"]
        .as_str()
        .expect("publication receipt must carry a unit UUID");
    let unit_revision = receipt["unit_revision"]
        .as_i64()
        .expect("publication receipt must carry a unit revision");
    assert_eq!(
        route(
            &mut client,
            "command",
            "knowledge.change_publish",
            publish_request.clone()
        )
        .await["replay"],
        *receipt
    );
    publisher_peer.finish().await;
    let committed = route(
        &mut client,
        "query",
        "knowledge.change",
        json!({"change_id":receipt["change_id"]}),
    )
    .await;
    assert_eq!(committed["stage"], "committed");
    assert_eq!(committed["publication_receipt"], *receipt);
    let exact = route(
        &mut client,
        "query",
        "knowledge.context",
        json!({"unit_id":unit_id,"revision":unit_revision}),
    )
    .await;
    assert_eq!(
        exact["exact_revision"]["constraint"]["source"]["text"],
        injected
    );
    assert!(
        exact["exact_revision"]["rdf_digest"]
            .as_str()
            .unwrap()
            .len()
            >= 32
    );
    knowledge_backup_support::assert_application_roundtrip(
        &pool,
        &admin_url,
        &runtime_url,
        &root,
        &config,
        &native,
        &key,
        &role,
        &receipt["unit_id"],
        &exact["exact_revision"],
    )
    .await;

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
            "qualification_reason":"Bounded DK-1 pipeline fixture."}),
    )
    .await;
    let context = &begun["created"];
    assert_eq!(context["knowledge_status"]["state"], "current");
    assert_eq!(
        context["knowledge"]["selected"].as_array().unwrap().len(),
        1
    );
    assert!(context["knowledge"]["selected"][0].get("source").is_none());
    let completion = find_action(&begun, "slice.pipeline.phase.complete").unwrap();
    assert_eq!(
        action_params(completion)["consumed_knowledge"]["manifest_id"],
        context["knowledge"]["id"]
    );

    let (revision, _) = prepare(
        &mut client,
        "revise",
        1,
        Some(&receipt["unit_id"]),
        Some(1),
        Some(workspace_draft(
            "Every active phase must retain exact source and binding provenance.",
            injected,
        )),
    )
    .await;
    let (revision, _) = approve(&mut client, &revision["prepared"]).await;
    let (revision, _) = publish(&mut client, &revision["approved"]).await;
    assert_eq!(revision["published"]["workspace_generation"], 2);

    let stale = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(stale["knowledge_status"]["state"], "stale");
    let refresh = find_action(&stale, "pipeline.knowledge_refresh").unwrap();
    assert_eq!(
        action_params(refresh)["run_revision"],
        stale["run"]["revision"]
    );
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
            "output":phase_output(phase,"stale-dk","completed","continue"),
            "consumed_outputs":consumed_outputs(&stale),"consumed_inputs":consumed_inputs(&stale),
            "consumed_knowledge":{"manifest_id":stale["knowledge"]["id"],"digest":stale["knowledge"]["digest"]},
            "publish_blocked_result":false}),
    )
    .await;
    assert_eq!(refused["error"]["code"], "context_changed");
    let refreshed = route(
        &mut client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    assert_eq!(refreshed["refreshed"]["selected"][0]["revision"], 2);
    assert_eq!(
        route(
            &mut client,
            "command",
            "knowledge.change_publish",
            publish_request.clone()
        )
        .await["replay"],
        *receipt
    );
    let refreshed_context = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":context["run"]["id"]}),
    )
    .await;
    assert_eq!(refreshed_context["knowledge_status"]["state"], "current");
    assert_eq!(
        refreshed_context["knowledge"]["id"],
        refreshed["refreshed"]["id"]
    );
    let (completed, _) = complete(
        &mut client,
        &refreshed_context,
        "completed",
        "continue",
        None,
        false,
    )
    .await;
    let next_context = &completed["context"];
    assert_eq!(next_context["run"]["current_phase_ordinal"], 2);
    assert_eq!(
        next_context["knowledge"]["phase_id"],
        next_context["run"]["current_phase_id"]
    );
    assert_ne!(
        next_context["knowledge"]["id"],
        refreshed_context["knowledge"]["id"]
    );
    assert_eq!(next_context["knowledge"]["selected"][0]["revision"], 2);

    let (retraction, _) = prepare(
        &mut client,
        "retract",
        2,
        Some(&receipt["unit_id"]),
        Some(2),
        None,
    )
    .await;
    let (retraction, _) = approve(&mut client, &retraction["prepared"]).await;
    let (retraction, _) = publish(&mut client, &retraction["approved"]).await;
    assert_eq!(retraction["published"]["delivery_eligible"], false);
    let gap = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":next_context["run"]["id"]}),
    )
    .await;
    assert_eq!(gap["knowledge_status"]["state"], "needs_context");

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
            "knowledge.context",
            json!({"unit_id":receipt["unit_id"]})
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
            "knowledge.context",
            json!({"unit_id":receipt["unit_id"]})
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
        route_error(&mut client, "query", "knowledge.context", json!({})).await["error"]["code"],
        "session_revoked"
    );
    assert_eq!(
        route_error(
            &mut client,
            "command",
            "knowledge.change_publish",
            publish_request
        )
        .await["error"]["code"],
        "session_revoked"
    );
    client.finish().await;
    pool.close().await;
}
