use sqlx::PgPool;
use std::{fs, path::Path};
use tect_application::{Store, TransactionMode};
use tect_domain::{HostAuth, KnowledgeEmbeddingJobCompletion};
use tect_postgres::PgStore;
use uuid::Uuid;

pub async fn verify_blocked(
    pool: &PgPool,
    runtime: &str,
    completion: &KnowledgeEmbeddingJobCompletion,
) {
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT tect_dk_search_vector_ready()")
            .fetch_one(pool)
            .await
            .unwrap()
    );
    let restored_lease: (String, Option<Uuid>) =
        sqlx::query_as("SELECT state,lease_token FROM knowledge_search_embedding_jobs WHERE id=$1")
            .bind(completion.job_id)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(
        restored_lease,
        ("leased".into(), Some(completion.lease_token))
    );
    assert!(PgStore::connect(runtime, 1).await.is_err());
}

#[allow(clippy::too_many_arguments)]
pub async fn verify_requalified(
    pool: &PgPool,
    runtime: &str,
    config: &Path,
    workspace: Uuid,
    survivor: Uuid,
    restored: &KnowledgeEmbeddingJobCompletion,
    runtime_role: &str,
) {
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT tect_dk_search_vector_ready()")
            .fetch_one(pool)
            .await
            .unwrap()
    );
    let reset_lease: (String, Option<Uuid>, bool) = sqlx::query_as(
        "SELECT state,lease_token,lease_expires_at IS NULL FROM knowledge_search_embedding_jobs WHERE id=$1",
    )
    .bind(restored.job_id)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(reset_lease, ("pending".into(), None, true));
    tect_postgres::enable_knowledge_vector_search(pool, runtime_role)
        .await
        .unwrap();
    assert!(
        sqlx::query_scalar::<_, bool>("SELECT tect_dk_search_vector_ready()")
            .fetch_one(pool)
            .await
            .unwrap()
    );
    let auth: HostAuth = serde_json::from_slice(&fs::read(config).unwrap()).unwrap();
    let store = PgStore::connect(runtime, 2).await.unwrap();
    let mut stale_tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = stale_tx.authenticate(&auth).await.unwrap();
    stale_tx.set_tenant(identity.tenant_id).await.unwrap();
    assert!(
        !stale_tx
            .complete_knowledge_search_job(workspace, identity.principal_id, restored)
            .await
            .unwrap()
    );
    stale_tx.commit().await.unwrap();

    let mut claim_tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = claim_tx.authenticate(&auth).await.unwrap();
    claim_tx.set_tenant(identity.tenant_id).await.unwrap();
    let mut claims = claim_tx
        .claim_knowledge_search_jobs(workspace, identity.principal_id, 1, &restored.model)
        .await
        .unwrap();
    assert_eq!(claims.len(), 1);
    let claim = claims.pop().unwrap();
    assert_eq!(claim.job_id, restored.job_id);
    assert_ne!(claim.lease_token, restored.lease_token);
    claim_tx.commit().await.unwrap();

    let completion = KnowledgeEmbeddingJobCompletion {
        job_id: claim.job_id,
        lease_token: claim.lease_token,
        input_digest: claim.input_digest,
        model: claim.model,
        values: restored.values.clone(),
    };
    let mut complete_tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = complete_tx.authenticate(&auth).await.unwrap();
    complete_tx.set_tenant(identity.tenant_id).await.unwrap();
    assert!(
        complete_tx
            .complete_knowledge_search_job(workspace, identity.principal_id, &completion)
            .await
            .unwrap()
    );
    complete_tx.commit().await.unwrap();

    let mut duplicate_tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    let identity = duplicate_tx.authenticate(&auth).await.unwrap();
    duplicate_tx.set_tenant(identity.tenant_id).await.unwrap();
    assert!(
        !duplicate_tx
            .complete_knowledge_search_job(workspace, identity.principal_id, &completion)
            .await
            .unwrap()
    );
    duplicate_tx.commit().await.unwrap();
    let vectors: i64 =
        sqlx::query_scalar("SELECT count(*) FROM knowledge_search_vectors WHERE unit_id=$1")
            .bind(survivor)
            .fetch_one(pool)
            .await
            .unwrap();
    assert_eq!(vectors, 1);
    store.pool().close().await;
}
