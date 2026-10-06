use super::*;
use tect_application::RecordMatrixDisposition;
use tect_domain::*;

#[path = "selected_native/matrix.rs"]
mod matrix;
#[path = "selected_native/native.rs"]
mod native;
#[path = "selected_native/pipeline.rs"]
mod pipeline;

#[derive(Debug, PartialEq, Eq)]
struct Effects {
    drafts: i64,
    receipts: i64,
    links: i64,
    revision: i64,
    status: String,
    snapshot: Uuid,
    input_cursor: i64,
    latest_input: i64,
}

async fn effects(pool: &PgPool, tenant: Uuid, workspace: Uuid, set: Uuid) -> Effects {
    let mut tx = pool.begin().await.unwrap();
    let row: (i64, i64, i64, i64, String, Uuid, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM slice_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3),
         (SELECT count(*) FROM native_planning_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 AND operation='save_slice_draft'),
         (SELECT count(*) FROM matrix_planning_selection_links WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3),
         revision,status,current_snapshot_id,input_cursor,latest_input FROM slice_candidate_sets
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    ).bind(tenant).bind(workspace).bind(set).fetch_one(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    Effects {
        drafts: row.0,
        receipts: row.1,
        links: row.2,
        revision: row.3,
        status: row.4,
        snapshot: row.5,
        input_cursor: row.6,
        latest_input: row.7,
    }
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 18.6 and explicit separate admin/runtime identities"]
async fn selected_native_save_commits_and_current_semantic_drift_writes_nothing() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_ne!(admin_url, runtime_url);
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, "180006");
    let store = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
    let owner = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let context = context(&owner.auth, &format!("matrix-selected-{}", Uuid::new_v4()));
    let initial = service(Arc::clone(&store));
    let workspace = initial
        .open_workspace(&context)
        .await
        .unwrap()
        .workspace
        .unwrap()
        .id;
    let (program, source, effective) =
        bound_source(&initial, &pool, &owner, &context, workspace).await;
    let (service, selection, sends) = matrix::selected(
        store, &pool, &owner, &context, workspace, &source, &effective,
    )
    .await;
    let planning = native::planning(&service, &context, program).await;
    let mut request = native::save_request(&planning, selection);
    let before = effects(&pool, owner.tenant_id, workspace, request.candidate_set_id).await;
    let saved = service
        .save_slice_candidate_draft(&context, &request, &native::Guidance, &native::Guard)
        .await
        .unwrap();
    let after = effects(&pool, owner.tenant_id, workspace, request.candidate_set_id).await;
    assert_eq!(
        (after.drafts, after.receipts, after.links),
        (before.drafts + 1, before.receipts + 1, before.links + 1)
    );
    assert_eq!(after.revision, before.revision + 1);
    assert_eq!(after.status, "review_required");
    assert_eq!(saved.candidate_set.revision, after.revision);
    let link: (Uuid, Uuid, i64, serde_json::Value) = sqlx::query_as(
        "SELECT disposition_id,caller_request_id,result_revision,mapped_nodes FROM matrix_planning_selection_links
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND caller_request_id=$4",
    ).bind(owner.tenant_id).bind(workspace).bind(request.candidate_set_id)
        .bind(request.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        link.0,
        request.matrix_selection.as_ref().unwrap().disposition_id
    );
    assert_eq!(link.1, request.request_id);
    assert_eq!(link.2, saved.candidate_set.revision);
    assert_eq!(link.3.as_array().unwrap().len(), 1);
    let persisted: (serde_json::Value, serde_json::Value, serde_json::Value) = sqlx::query_as(
        "SELECT d.payload,r.request_payload,r.result_payload FROM slice_candidate_drafts d
         JOIN native_planning_receipts r ON (r.tenant_id,r.workspace_id,r.entity_id)=(d.tenant_id,d.workspace_id,d.candidate_set_id)
         WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.candidate_set_id=$3 AND d.set_revision=$4
         AND r.operation='save_slice_draft' AND r.request_id=$5",
    ).bind(owner.tenant_id).bind(workspace).bind(request.candidate_set_id)
        .bind(saved.candidate_set.revision).bind(request.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        persisted.0,
        serde_json::to_value(saved.draft.as_ref().unwrap()).unwrap()
    );
    assert_eq!(persisted.0, persisted.2["draft"]);
    assert_eq!(
        persisted.1["matrix_selection"],
        serde_json::to_value(request.matrix_selection.as_ref().unwrap()).unwrap()
    );
    matrix::change_semantics(&service, &context, program).await;
    assert_eq!(
        service
            .get_matrix_task_source(&context, source.revision.task_id)
            .await
            .unwrap(),
        source
    );
    request.request_id = Uuid::new_v4();
    request.revision = saved.candidate_set.revision;
    request.consumed_knowledge = native::knowledge(saved.planning_knowledge.as_ref());
    let stable = effects(&pool, owner.tenant_id, workspace, request.candidate_set_id).await;
    assert!(matches!(
        service
            .save_slice_candidate_draft(&context, &request, &native::Guidance, &native::Guard,)
            .await,
        Err(Error::StaleContext)
    ));
    assert_eq!(
        effects(&pool, owner.tenant_id, workspace, request.candidate_set_id).await,
        stable
    );
    assert_eq!(sends.load(Ordering::SeqCst), 0);
}

#[path = "selected_native/native_provider_pg.rs"]
mod native_provider_pg;
