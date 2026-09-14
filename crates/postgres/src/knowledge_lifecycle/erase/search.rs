use super::registry::{CopyRelation, register};
use super::*;

pub(crate) async fn register_search_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let resource:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_search_resources WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3)")
        .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if resource {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "search_resource",
            CopyRelation::SearchResource,
            unit,
            0,
        )
        .await?;
    }
    let jobs:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 ORDER BY id")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for job in jobs {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "search_job",
            CopyRelation::SearchJob,
            job,
            0,
        )
        .await?;
    }
    let vector_table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if vector_table {
        let vector:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_search_vectors WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3)")
            .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if vector {
            register(
                tx,
                tenant,
                workspace,
                unit,
                "search_vector",
                CopyRelation::SearchVector,
                unit,
                0,
            )
            .await?;
        }
    }
    sqlx::query("UPDATE knowledge_owned_copies c SET redacted=NOT ((c.relation_name='knowledge_search_resources' AND EXISTS(SELECT 1 FROM knowledge_search_resources r WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.unit_id=c.row_id)) OR (c.relation_name='knowledge_search_embedding_jobs' AND EXISTS(SELECT 1 FROM knowledge_search_embedding_jobs j WHERE j.tenant_id=$1 AND j.workspace_id=$2 AND j.id=c.row_id))) WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name IN('knowledge_search_resources','knowledge_search_embedding_jobs')")
        .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    if vector_table {
        sqlx::query("UPDATE knowledge_owned_copies c SET redacted=NOT EXISTS(SELECT 1 FROM knowledge_search_vectors v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=c.row_id) WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name='knowledge_search_vectors'")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    } else {
        sqlx::query("UPDATE knowledge_owned_copies SET redacted=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND relation_name='knowledge_search_vectors'")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}

pub(crate) async fn reconcile_absent_search_copies(
    tx: &mut Transaction<'_, Postgres>,
) -> Result<()> {
    sqlx::query("UPDATE knowledge_owned_copies c SET redacted=true WHERE c.relation_name='knowledge_search_resources' AND NOT EXISTS(SELECT 1 FROM knowledge_search_resources r WHERE r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.unit_id=c.row_id)")
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE knowledge_owned_copies c SET redacted=true WHERE c.relation_name='knowledge_search_embedding_jobs' AND NOT EXISTS(SELECT 1 FROM knowledge_search_embedding_jobs j WHERE j.tenant_id=c.tenant_id AND j.workspace_id=c.workspace_id AND j.id=c.row_id)")
        .execute(&mut **tx).await.map_err(storage_error)?;
    let vector_table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if vector_table {
        sqlx::query("UPDATE knowledge_owned_copies c SET redacted=true WHERE c.relation_name='knowledge_search_vectors' AND NOT EXISTS(SELECT 1 FROM knowledge_search_vectors v WHERE v.tenant_id=c.tenant_id AND v.workspace_id=c.workspace_id AND v.unit_id=c.row_id)")
            .execute(&mut **tx).await.map_err(storage_error)?;
    } else {
        sqlx::query("UPDATE knowledge_owned_copies SET redacted=true WHERE relation_name='knowledge_search_vectors'")
            .execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}
