use sqlx::PgPool;
use std::{fs, path::Path};
use tect_application::{Store, TransactionMode};
use tect_domain::{Error, HostAuth, KnowledgeMaintenanceJobClaim};
use tect_postgres::PgStore;
use uuid::Uuid;

pub async fn verify_blocked(
    pool: &PgPool,
    target_task: Uuid,
    survivor_task: Uuid,
    old_lease: Uuid,
) {
    let survivor: (String, i32, Option<Uuid>) = sqlx::query_as(
        "SELECT state,attempts,lease_token FROM knowledge_maintenance_tasks WHERE id=$1",
    )
    .bind(survivor_task)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(survivor, ("leased".into(), 1, Some(old_lease)));
    let target: (String, bool) =
        sqlx::query_as("SELECT state,payload_erased FROM knowledge_maintenance_tasks WHERE id=$1")
            .bind(target_task)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(target, ("pending".into(), false));
}

#[allow(clippy::too_many_arguments)]
pub async fn verify_requalified(
    pool: &PgPool,
    runtime: &str,
    config: &Path,
    workspace: Uuid,
    target_unit: Uuid,
    target_task: Uuid,
    survivor_unit: Uuid,
    survivor_task: Uuid,
    survivor_revision: i64,
    old_lease: Uuid,
) {
    let survivor: (String, i32, Option<Uuid>, bool, Option<String>) = sqlx::query_as(
        "SELECT state,attempts,lease_token,lease_expires_at IS NULL,failure_code \
         FROM knowledge_maintenance_tasks WHERE id=$1",
    )
    .bind(survivor_task)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        survivor,
        (
            "pending".into(),
            1,
            None,
            true,
            Some("lease_expired".into())
        )
    );

    let auth: HostAuth = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let store = PgStore::connect(runtime, 1).await.unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = tx.authenticate(&auth).await.unwrap();
    tx.set_tenant(identity.tenant_id).await.unwrap();
    let stale = KnowledgeMaintenanceJobClaim {
        task_id: survivor_task,
        task_revision: survivor_revision,
        lease_token: old_lease,
        workspace_id: workspace,
        principal_id: identity.principal_id,
    };
    assert_eq!(
        tx.prepare_knowledge_maintenance_task(workspace, identity.principal_id, &stale)
            .await,
        Err(Error::ContextChanged)
    );
    drop(tx);
    store.pool().close().await;

    let target: (bool, String, Option<Uuid>, bool, bool, bool, bool, bool) = sqlx::query_as(
        "SELECT t.payload_erased,t.state,t.lease_token,t.current_review IS NULL,\
         t.affected_consumers IS NULL,t.terminal_evidence IS NULL,s.payload_erased,\
         s.basis IS NULL AND s.basis_digest IS NULL \
         FROM knowledge_maintenance_tasks t JOIN knowledge_maintenance_signals s ON s.id=t.signal_id \
         WHERE t.id=$1",
    )
    .bind(target_task)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(
        target,
        (true, "obsolete".into(), None, true, true, true, true, true)
    );
    let erased_receipts: (i64, i64) = sqlx::query_as(
        "SELECT count(*),count(*) FILTER (WHERE payload_erased AND request_payload IS NULL \
         AND result_payload IS NULL) FROM knowledge_maintenance_command_receipts WHERE unit_id=$1",
    )
    .bind(target_unit)
    .fetch_one(pool)
    .await
    .unwrap();
    assert!(erased_receipts.0 > 0);
    assert_eq!(erased_receipts.1, erased_receipts.0);
    let survivor_copies: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT relation_name FROM knowledge_owned_copies WHERE unit_id=$1 \
         AND relation_name LIKE 'knowledge_maintenance_%' AND NOT redacted ORDER BY relation_name",
    )
    .bind(survivor_unit)
    .fetch_all(pool)
    .await
    .unwrap();
    for relation in [
        "knowledge_maintenance_command_receipts",
        "knowledge_maintenance_signals",
        "knowledge_maintenance_tasks",
    ] {
        assert!(survivor_copies.iter().any(|value| value == relation));
    }
}
