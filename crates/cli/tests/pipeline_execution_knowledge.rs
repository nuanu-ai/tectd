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

#[path = "pipeline_execution/knowledge_backend_proof.rs"]
mod knowledge_backend_proof;

use knowledge_lifecycle_support::{
    advance_create_to_review, begin_create_request, complete_review, context, reads,
};
use knowledge_operation_support::{SingleOperation, commit_single};
use lifecycle_support::lightweight_draft;
use recovery_support::native_reads::ScopeOpenFixture;
use recovery_support::pipeline_reads::{ResolvedPipeline, resolve_pipeline};
use recovery_support::{
    Daemon, Mcp, action_params, find_action, host_file, private_temp, public_call, tagged_url,
    tool_payload,
};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, route_error, save};
use tect_postgres::admin;
use uuid::Uuid;

#[path = "pipeline_execution_knowledge/fixture_support.rs"]
mod fixture_support;
use fixture_support::{assert_compact, assert_disposition, assert_state, constraint, read_current};

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
    let begun = reads::resolve_current(&mut client, begun).await;
    assert_disposition(&begun.raw_response, "created");
    let change_id = context(&begun.value)["change_id"].clone();
    let replayed_begin = route(
        &mut client,
        "command",
        "knowledge.change_begin",
        begin_request.clone(),
    )
    .await;
    let replayed_begin = reads::resolve_current(&mut client, replayed_begin).await;
    assert_disposition(&replayed_begin.raw_response, "replay");
    assert_eq!(
        context(&replayed_begin.value)["run"]["id"],
        context(&begun.value)["run"]["id"]
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

    let reviewed = advance_create_to_review(&mut client, &document, begun.value).await;
    let reviewed = complete_review(&mut client, &reviewed, "ready").await;
    let publication = route(
        &mut client,
        "command",
        "knowledge.change_phase_complete",
        action_params(reads::phase_completion_action(&reviewed)).clone(),
    )
    .await;
    let publication = reads::resolve_current(&mut client, publication).await;
    let commit_request = action_params(reads::producer_action(
        &publication.value,
        "knowledge.change_commit",
    ))
    .clone();
    let mut publisher_peer = Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    publisher_peer.call("open_workspace", json!({})).await;
    let abandoned_snapshot = admin::begin_backup_snapshot(&pool).await.unwrap();
    drop(abandoned_snapshot);
    let mut held_snapshot = admin::begin_backup_snapshot(&pool).await.unwrap();
    let mut publishers = Box::pin(async {
        tokio::join!(
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
        )
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(250), &mut publishers)
            .await
            .is_err(),
        "publisher must wait while the backup snapshot holds the shared native graph lock"
    );
    assert!(
        held_snapshot
            .export_graphs()
            .await
            .unwrap()
            .iter()
            .all(|graph| !graph.iri.starts_with("urn:tect:dk:scratch:"))
    );
    held_snapshot.finish().await.unwrap();
    let (raw_a, raw_b) = tokio::time::timeout(std::time::Duration::from_secs(10), publishers)
        .await
        .expect("publisher must proceed after successful and abandoned backup guards release");
    let payload_a = tool_payload(&raw_a);
    let payload_b = tool_payload(&raw_b);
    let applied_a = payload_a.get("applied").is_some() || payload_a["outcome"] == "applied";
    let receipt_a = reads::resolve_commit_receipt(&mut client, payload_a).await;
    let receipt_b = reads::resolve_commit_receipt(&mut publisher_peer, payload_b).await;
    let (applied, replay) = if applied_a {
        (receipt_a, receipt_b)
    } else {
        (receipt_b, receipt_a)
    };
    assert_disposition(&applied.raw_response, "applied");
    assert_disposition(&replay.raw_response, "replay");
    assert!(applied.receipt.is_object());
    assert!(replay.receipt.is_object());
    assert_eq!(replay.receipt, applied.receipt);
    let receipt = applied.receipt;
    assert_eq!(receipt["workspace_generation"], 1);
    let repeated_commit = route(
        &mut client,
        "command",
        "knowledge.change_commit",
        commit_request.clone(),
    )
    .await;
    let repeated_commit = reads::resolve_commit_receipt(&mut client, repeated_commit).await;
    assert_disposition(&repeated_commit.raw_response, "replay");
    assert!(repeated_commit.receipt.is_object());
    assert_eq!(repeated_commit.receipt, receipt);
    publisher_peer.finish().await;
    let unit_id = receipt["applied_operations"][0]["unit_id"].clone();
    let unit_revision = receipt["applied_operations"][0]["revision"].clone();
    assert_eq!(unit_revision, 1);
    let exact = reads::read_unit(&mut client, &unit_id, &unit_revision).await;
    let exact = exact.value;
    assert_eq!(exact["document"]["document"], document);
    assert_eq!(
        exact["document"]["document"]["sources"][0]["snapshot"]["text"],
        injected
    );
    assert!(exact["document"]["rdf_digest"].as_str().unwrap().len() >= 32);
    let cold = reads::read_current(&mut client, &change_id).await;
    assert_eq!(context(&cold.value)["publisher_receipt"], receipt);

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
    let planning = ScopeOpenFixture::from_mutation(opened_scope, "created")
        .read_planning(&mut client)
        .await;
    let saved = save(&mut client, &planning.value, lightweight_draft()).await;
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
    let pipeline = resolve_pipeline(&mut client, begun_pipeline).await.unwrap();
    assert_state(&pipeline, "current");
    assert_eq!(
        pipeline.details_data()["knowledge_resources"]["selected"][0]["unit_id"],
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
    let revision = reads::resolve_commit_receipt(&mut client, revision).await;
    assert_disposition(&revision.raw_response, "applied");
    assert_eq!(revision.receipt["workspace_generation"], 2);
    let stale_default = read_current(&mut client, json!({"run_id":pipeline.run()["id"]})).await;
    assert_compact(&stale_default);
    assert_state(&stale_default, "stale");
    let stale = read_current(
        &mut client,
        json!({"run_id":pipeline.run()["id"],"refresh":true}),
    )
    .await;
    assert_compact(&stale);
    assert_state(&stale, "stale");
    let before_stale = knowledge_backend_proof::checkpoint(&pool, &stale).await;
    let refused = route_error(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        knowledge_backend_proof::k1_request(&stale),
    )
    .await;
    assert_eq!(refused["error"]["code"], "context_changed");
    assert_eq!(
        knowledge_backend_proof::checkpoint(&pool, &stale).await,
        before_stale
    );
    let refresh = find_action(&stale.raw_payload, "pipeline.knowledge_refresh").unwrap();
    route(
        &mut client,
        "command",
        "pipeline.knowledge_refresh",
        action_params(refresh).clone(),
    )
    .await;
    let refreshed_default = read_current(&mut client, json!({"run_id":pipeline.run()["id"]})).await;
    assert_compact(&refreshed_default);
    assert_state(&refreshed_default, "current");
    assert!(
        refreshed_default.run()["revision"].as_i64().unwrap()
            > stale.run()["revision"].as_i64().unwrap()
    );
    let refreshed = read_current(
        &mut client,
        json!({"run_id":pipeline.run()["id"],"refresh":true}),
    )
    .await;
    assert_compact(&refreshed);
    assert_eq!(
        refreshed_default.current_phase().unwrap(),
        refreshed.current_phase().unwrap()
    );
    assert_eq!(
        refreshed_default.run()["revision"],
        refreshed.run()["revision"]
    );
    assert_state(&refreshed, "current");
    assert!(
        refreshed.details_data()["knowledge_resources"]["run_revision"]
            .as_i64()
            .unwrap()
            > stale.details_data()["knowledge_resources"]["run_revision"]
                .as_i64()
                .unwrap()
    );
    assert_eq!(
        refreshed.details_data()["knowledge_resources"]["selected"][0]["revision"],
        2
    );
    knowledge_backend_proof::reject_agent_ack(&mut client, &pool, &refreshed).await;
    let completion_request = knowledge_backend_proof::k1_request(&refreshed);
    let completed = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        completion_request.clone(),
    )
    .await;
    knowledge_backend_proof::persisted_binding(&pool, &refreshed, &completion_request).await;
    let completed = resolve_pipeline(&mut client, completed).await.unwrap();
    assert_eq!(completed.run()["current_phase_ordinal"], 2);

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
    let retraction = reads::resolve_commit_receipt(&mut client, retraction).await;
    assert_disposition(&retraction.raw_response, "applied");
    assert_eq!(
        retraction.receipt["applied_operations"][0]["operation"],
        "retract"
    );
    let gap = read_current(&mut client, json!({"run_id":completed.run()["id"]})).await;
    assert_state(&gap, "needs_context");

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
