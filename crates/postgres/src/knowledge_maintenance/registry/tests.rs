use super::*;
use serde_json::{Value, json};
use sqlx::PgPool;

async fn fixture() -> Option<(PgPool, Uuid, Uuid, Uuid)> {
    if std::env::var("TECT_TEST_MANIFEST_REGISTRY").as_deref() != Ok("1") {
        return None;
    }
    let pool = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    let tenant = std::env::var("TECT_TEST_MANIFEST_TENANT")
        .unwrap()
        .parse()
        .unwrap();
    let workspace = std::env::var("TECT_TEST_MANIFEST_WORKSPACE")
        .unwrap()
        .parse()
        .unwrap();
    let manifest = std::env::var("TECT_TEST_MANIFEST_ID")
        .unwrap()
        .parse()
        .unwrap();
    Some((pool, tenant, workspace, manifest))
}

async fn transaction(pool: &PgPool, tenant: Uuid) -> Transaction<'_, Postgres> {
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    tx
}

async fn rows(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Value {
    sqlx::query_scalar(
        "SELECT COALESCE(jsonb_agg(to_jsonb(c) ORDER BY unit_id,unit_revision),'[]'::jsonb) \
         FROM knowledge_maintenance_consumers c WHERE tenant_id=$1 AND workspace_id=$2 \
         AND relation_name='pipeline_knowledge_manifests' AND row_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest)
    .fetch_one(&mut **tx)
    .await
    .unwrap()
}

#[tokio::test]
async fn manifest_batch_matches_scalar_conflicts_and_owned_copies() {
    let Some((pool, tenant, workspace, manifest)) = fixture().await else {
        return;
    };
    let mut tx = transaction(&pool, tenant).await;
    let original = rows(&mut tx, tenant, workspace, manifest).await;
    assert!(!original.as_array().unwrap().is_empty());
    // Scalar conflict replay is the existing oracle: IDs and required flags survive.
    for row in original.as_array().unwrap() {
        register_consumer(
            &mut tx,
            tenant,
            workspace,
            row["unit_id"].as_str().unwrap().parse().unwrap(),
            row["unit_revision"].as_i64().unwrap(),
            row["consumer_ref"].as_str().unwrap(),
            false,
            MANIFEST_RELATION,
            manifest,
        )
        .await
        .unwrap();
    }
    let scalar = rows(&mut tx, tenant, workspace, manifest).await;
    register_manifest_consumers(&mut tx, tenant, workspace, manifest)
        .await
        .unwrap();
    assert_eq!(rows(&mut tx, tenant, workspace, manifest).await, scalar);
    let missing_copies: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM knowledge_maintenance_consumers c WHERE c.tenant_id=$1 \
         AND c.workspace_id=$2 AND c.row_id=$3 AND NOT EXISTS(SELECT 1 FROM knowledge_owned_copies o \
         WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.unit_id=c.unit_id \
         AND o.copy_kind='maintenance_consumer' AND o.relation_name='knowledge_maintenance_consumers' \
         AND o.row_id=c.id AND o.source_revision=c.unit_revision)",
    ).bind(tenant).bind(workspace).bind(manifest).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(missing_copies, 0);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn manifest_batch_rejects_stale_run_and_preserves_scope() {
    let Some((pool, tenant, workspace, manifest)) = fixture().await else {
        return;
    };
    let mut tx = transaction(&pool, tenant).await;
    let before = rows(&mut tx, tenant, workspace, manifest).await;
    register_manifest_consumers(&mut tx, tenant, Uuid::new_v4(), manifest)
        .await
        .unwrap();
    assert_eq!(rows(&mut tx, tenant, workspace, manifest).await, before);
    sqlx::query("UPDATE slice_pipeline_runs SET origin_payload=NULL,origin_result=NULL,qualification_reason=NULL,inquiry=NULL,source_checkpoint_digest=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND knowledge_manifest_id=$3")
        .bind(tenant).bind(workspace).bind(manifest).execute(&mut *tx).await.unwrap();
    assert_eq!(
        register_manifest_consumers(&mut tx, tenant, workspace, manifest).await,
        Err(Error::InvalidArguments)
    );
    tx.rollback().await.unwrap();
    let mut tx = transaction(&pool, Uuid::new_v4()).await;
    register_manifest_consumers(&mut tx, tenant, workspace, manifest)
        .await
        .unwrap();
    assert_eq!(rows(&mut tx, tenant, workspace, manifest).await, json!([]));
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn manifest_batch_capacity_and_erased_manifest_are_bounded() {
    let Some((pool, tenant, workspace, manifest)) = fixture().await else {
        return;
    };
    let mut tx = transaction(&pool, tenant).await;
    let oversized = (0..513)
        .map(|_| json!({"unit_id":Uuid::new_v4(),"revision":1}))
        .collect::<Vec<_>>();
    sqlx::query("UPDATE pipeline_knowledge_manifests SET selected=$4,selected_resources='[]'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(manifest).bind(json!(oversized)).execute(&mut *tx).await.unwrap();
    assert_eq!(
        register_manifest_consumers(&mut tx, tenant, workspace, manifest).await,
        Err(Error::CapacityExceeded)
    );
    tx.rollback().await.unwrap();
    let mut tx = transaction(&pool, tenant).await;
    let before = rows(&mut tx, tenant, workspace, manifest).await;
    sqlx::query("UPDATE pipeline_knowledge_manifests SET digest=NULL,semantic_digest=NULL,selected=NULL,unresolved_needs=NULL,definition_version=NULL,definition_digest=NULL,method_requirements=NULL,selected_resources=NULL,resource_unresolved_needs=NULL,freshness_warnings=NULL,resource_semantic_digest=NULL,resource_inquiry=NULL,resource_projection_policy=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(manifest).execute(&mut *tx).await.unwrap();
    register_manifest_consumers(&mut tx, tenant, workspace, manifest)
        .await
        .unwrap();
    assert_eq!(rows(&mut tx, tenant, workspace, manifest).await, before);
    tx.rollback().await.unwrap();
}

#[tokio::test]
async fn manifest_batch_reactivates_required_consumers_and_retires_previous() {
    let Some((pool, tenant, workspace, manifest)) = fixture().await else {
        return;
    };
    let mut tx = transaction(&pool, tenant).await;
    let original = rows(&mut tx, tenant, workspace, manifest).await;
    sqlx::query("UPDATE knowledge_maintenance_consumers SET active=false WHERE tenant_id=$1 AND workspace_id=$2 AND row_id=$3")
        .bind(tenant).bind(workspace).bind(manifest).execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE knowledge_maintenance_consumers c SET active=true FROM pipeline_knowledge_manifests previous,pipeline_knowledge_manifests current WHERE current.id=$3 AND previous.tenant_id=$1 AND previous.workspace_id=$2 AND previous.run_id=current.run_id AND previous.id<>current.id AND c.row_id=previous.id AND c.tenant_id=$1 AND c.workspace_id=$2")
        .bind(tenant).bind(workspace).bind(manifest).execute(&mut *tx).await.unwrap();
    register_manifest_consumers(&mut tx, tenant, workspace, manifest)
        .await
        .unwrap();
    assert_eq!(rows(&mut tx, tenant, workspace, manifest).await, original);
    let active_previous: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_maintenance_consumers c JOIN pipeline_knowledge_manifests p ON p.id=c.row_id JOIN pipeline_knowledge_manifests m ON m.run_id=p.run_id WHERE m.id=$3 AND p.id<>m.id AND c.tenant_id=$1 AND c.workspace_id=$2 AND c.active")
        .bind(tenant).bind(workspace).bind(manifest).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(active_previous, 0);
    crate::knowledge_lifecycle::erase::register_pipeline_manifest_copies(
        &mut tx, tenant, workspace, manifest,
    )
    .await
    .unwrap();
    let missing_manifest_copies:i64=sqlx::query_scalar("SELECT count(*) FROM knowledge_maintenance_consumers c WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.row_id=$3 AND NOT EXISTS(SELECT 1 FROM knowledge_owned_copies o WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.unit_id=c.unit_id AND o.copy_kind='pipeline_manifest' AND o.row_id=$3 AND o.row_revision=c.unit_revision)")
        .bind(tenant).bind(workspace).bind(manifest).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(missing_manifest_copies, 0);
    tx.rollback().await.unwrap();
    let mut tx = transaction(&pool, tenant).await;
    assert_eq!(rows(&mut tx, tenant, workspace, manifest).await, original);
    tx.rollback().await.unwrap();
}

fn semantic_rows(mut rows: Value) -> Value {
    for row in rows.as_array_mut().unwrap() {
        row.as_object_mut().unwrap().remove("id");
        row.as_object_mut().unwrap().remove("created_at");
    }
    rows
}

#[tokio::test]
async fn manifest_batch_fresh_insert_matches_scalar_and_required_or() {
    let Some((pool, tenant, workspace, manifest)) = fixture().await else {
        return;
    };
    let mut tx = transaction(&pool, tenant).await;
    let original = rows(&mut tx, tenant, workspace, manifest).await;
    let new_manifest = Uuid::new_v4();
    sqlx::query("INSERT INTO pipeline_knowledge_manifests SELECT (pg_catalog.jsonb_populate_record(NULL::pipeline_knowledge_manifests,pg_catalog.to_jsonb(m)||pg_catalog.jsonb_build_object('id',$4::uuid))).* FROM pipeline_knowledge_manifests m WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(manifest).bind(new_manifest).execute(&mut *tx).await.unwrap();
    let manifest = new_manifest;
    let consumer_ref = format!("pipeline-manifest:{manifest}");
    sqlx::query("SAVEPOINT fresh_registry")
        .execute(&mut *tx)
        .await
        .unwrap();
    for row in original.as_array().unwrap() {
        register_consumer(
            &mut tx,
            tenant,
            workspace,
            row["unit_id"].as_str().unwrap().parse().unwrap(),
            row["unit_revision"].as_i64().unwrap(),
            &consumer_ref,
            row["required"].as_bool().unwrap(),
            MANIFEST_RELATION,
            manifest,
        )
        .await
        .unwrap();
    }
    let scalar = semantic_rows(rows(&mut tx, tenant, workspace, manifest).await);
    sqlx::query("ROLLBACK TO SAVEPOINT fresh_registry")
        .execute(&mut *tx)
        .await
        .unwrap();
    register_manifest_consumers(&mut tx, tenant, workspace, manifest)
        .await
        .unwrap();
    let inserted = rows(&mut tx, tenant, workspace, manifest).await;
    assert_eq!(semantic_rows(inserted.clone()), scalar);
    assert_eq!(inserted.as_array().unwrap().len(), 22);
    let first = &inserted[0];
    let id: Uuid = first["id"].as_str().unwrap().parse().unwrap();
    sqlx::query("UPDATE knowledge_maintenance_consumers SET required=false WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).execute(&mut *tx).await.unwrap();
    register_manifest_consumers(&mut tx, tenant, workspace, manifest)
        .await
        .unwrap();
    register_consumer(
        &mut tx,
        tenant,
        workspace,
        first["unit_id"].as_str().unwrap().parse().unwrap(),
        first["unit_revision"].as_i64().unwrap(),
        &consumer_ref,
        false,
        MANIFEST_RELATION,
        manifest,
    )
    .await
    .unwrap();
    let after = rows(&mut tx, tenant, workspace, manifest).await;
    assert_eq!(after[0]["id"], first["id"]);
    assert_eq!(after[0]["required"], true);
    let matched_copies:i64=sqlx::query_scalar("SELECT count(*) FROM knowledge_maintenance_consumers c JOIN knowledge_owned_copies o ON o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.unit_id=c.unit_id AND o.row_id=c.id AND o.source_revision=c.unit_revision AND o.copy_kind='maintenance_consumer' AND o.relation_name='knowledge_maintenance_consumers' WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.row_id=$3")
        .bind(tenant).bind(workspace).bind(manifest).fetch_one(&mut *tx).await.unwrap();
    assert_eq!(matched_copies, 22);
    tx.rollback().await.unwrap();
}
