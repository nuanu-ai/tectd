#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "pipeline_execution/knowledge_operation_support.rs"]
#[allow(dead_code)]
mod knowledge_operation_support;
#[path = "pipeline_execution/knowledge_suppression_backup_support.rs"]
#[allow(dead_code)]
mod knowledge_suppression_backup_support;
#[path = "pipeline_execution/promotion_support.rs"]
#[allow(dead_code)]
mod promotion_support;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use knowledge_lifecycle_support::{commit_create, commit_create_from_current, settle_and_finish};
use knowledge_operation_support::{SingleOperation, commit_single};
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use support::{open_slice, ready_source_candidate, repository, review, route, save};
use tect_postgres::admin;
use uuid::Uuid;

fn consumer_draft() -> Value {
    json!({"coverage_summary":"Capture exact knowledge in a frozen pipeline origin.","nodes":[{
        "kind":"work","identity":{"local":"recovery-consumer"},
        "title":"Consume the exact recovery target","outcome":"The target is frozen in run origin",
        "includes":["typed knowledge"],"excludes":["implicit repin"],"dependencies":[],
        "proof":["Exact captured manifest"],"pipeline":"slice.custom-procedure-capture",
        "pipeline_reason":"Exercise the owned run-origin recovery boundary","source_result_ids":[]}],
        "supersessions":[]})
}

async fn begin_consumer(client: &mut Mcp, repo: &std::path::Path) -> Value {
    let (source, candidate) = ready_source_candidate(client, repo).await;
    let scope=route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":source["candidate_set"]["id"],"candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await;
    let saved = save(client, &scope["created"]["planning"], consumer_draft()).await;
    let reviewed = review(client, &saved).await;
    let opened = route(
        client,
        "command",
        "slice.open",
        open_slice(&reviewed, &reviewed["draft"]["nodes"][0], Uuid::new_v4()),
    )
    .await;
    route(
        client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
        "scope_id":reviewed["scope"]["id"],"slice_id":opened["created"]["id"],
        "slice_revision":opened["created"]["revision"],"delivery_mode":"phasewise",
        "qualification_reason":"Freeze actual typed resources in the run origin."}),
    )
    .await["created"]
        .clone()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn managed_restore_reapplies_complete_suppression_and_preserves_survivor() {
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
    let socket = root.join("knowledge-recovery.sock");
    let runtime = tagged_url(&runtime_url, &format!("dk2-recovery-{}", Uuid::new_v4()));
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("dk2-recovery-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;

    let marker = format!("recovery-erased-marker-{}", Uuid::new_v4());
    let seed = promotion_support::open(&mut client, &repo, &marker).await;
    let target = commit_create_from_current(&mut client, seed.document.clone(), seed.begun).await;
    settle_and_finish(&mut client, &target).await;
    let target_unit = target.receipt["applied_operations"][0]["unit_id"].clone();
    let target_change = Uuid::parse_str(target.receipt["change_id"].as_str().unwrap()).unwrap();
    let target_run = Uuid::parse_str(target.receipt["run_id"].as_str().unwrap()).unwrap();

    let mut survivor_document = seed.document.clone();
    survivor_document["title"] = json!("Independent recovery survivor");
    survivor_document["sources"][0]["snapshot"]["text"] = json!("independent survivor text");
    survivor_document["sources"][0]["snapshot"]["uri"] = json!("urn:independent-survivor");
    let survivor = commit_create(&mut client, survivor_document).await;
    let survivor_unit = survivor.receipt["applied_operations"][0]["unit_id"].clone();
    let origin = begin_consumer(&mut client, &repo).await;
    assert!(
        origin["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value["unit_id"] == target_unit)
    );

    let managed: (Uuid, bool, i64) = sqlx::query_as(
        "SELECT r.id,r.payload_erased,(SELECT count(*) FROM slice_planning_inputs i WHERE i.source_result_id=r.id) \
         FROM slice_results r WHERE r.knowledge_change_id=$1 AND r.knowledge_run_id=$2")
        .bind(target_change).bind(target_run).fetch_one(&pool).await.unwrap();
    assert!(!managed.1);
    assert_eq!(managed.2, 1);
    let target_uuid = Uuid::parse_str(target_unit.as_str().unwrap()).unwrap();
    let owned_before: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT relation_name FROM knowledge_owned_copies WHERE unit_id=$1 ORDER BY relation_name")
        .bind(target_uuid).fetch_all(&pool).await.unwrap();
    for relation in [
        "slice_results",
        "slice_planning_inputs",
        "slice_pipeline_runs",
    ] {
        assert!(
            owned_before.iter().any(|value| value == relation),
            "missing owned {relation}"
        );
    }
    let older_manifest = tect_postgres::prepare_knowledge_suppression_manifest(&pool)
        .await
        .unwrap();
    assert_eq!(older_manifest.high_water_erasure_sequence, 0);
    let older_checkpoint =
        tect_postgres::record_knowledge_suppression_export(&pool, &older_manifest)
            .await
            .unwrap();
    let backup = knowledge_suppression_backup_support::capture(&pool, &admin_url, &root).await;

    let erased = commit_single(
        &mut client,
        SingleOperation {
            operation: "erase",
            unit_id: Some(target_unit.clone()),
            expected_revision: Some(1),
            expected_lifecycle: Some("active"),
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
    let manifest = tect_postgres::prepare_knowledge_suppression_manifest(&pool)
        .await
        .unwrap();
    assert_eq!(manifest.high_water_erasure_sequence, 1);
    let checkpoint = tect_postgres::record_knowledge_suppression_export(&pool, &manifest)
        .await
        .unwrap();
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
    knowledge_suppression_backup_support::restore_apply_and_verify(
        backup,
        &admin_url,
        &runtime_url,
        &root,
        &config,
        &native,
        &workspace_key,
        &role,
        &manifest,
        &checkpoint,
        &older_manifest,
        &older_checkpoint,
        &target_unit,
        &seed.begin_request,
        &origin["run"]["id"],
        &marker,
        &survivor_unit,
        &survivor.exact,
    )
    .await;
    pool.close().await;
}
