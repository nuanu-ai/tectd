use crate::storage_error;
use sha2::{Digest, Sha256};
use sqlx::{Postgres, Transaction};
use tect_domain::*;
use uuid::Uuid;

mod corpus;
mod graph;
mod jobs;
mod projection;
mod query;
mod resource;

pub(crate) use jobs::{claim, complete, fail, pending};
pub(crate) use projection::{apply_dk2_operation, invalidate_unit, project_legacy};
pub(crate) use query::search;

pub(crate) fn sha256(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

pub(crate) fn normalized(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

pub(crate) fn enum_text<T: serde::Serialize>(value: &T) -> Result<String> {
    crate::knowledge_lifecycle::enum_text(value)
}

pub(crate) async fn search_vector_ready(tx: &mut Transaction<'_, Postgres>) -> Result<bool> {
    sqlx::query_scalar("SELECT tect_dk_search_vector_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)
}

pub(crate) async fn preflight(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: &KnowledgeSearchQuery,
    _provider: Option<&KnowledgeEmbeddingModelIdentity>,
) -> Result<KnowledgeSearchPreflight> {
    crate::durable_knowledge::require_identity_ready(tx).await?;
    corpus::validate_binding(tx, tenant, workspace, request.binding.as_ref()).await?;
    let generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let vector_capability_ready: bool = sqlx::query_scalar("SELECT tect_dk_search_vector_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    let normalized_query = request.query.as_deref().map(normalized);
    let query_input_digest = normalized_query
        .as_deref()
        .map(|value| sha256(&format!("query: {value}")));
    let _ = principal;
    Ok(KnowledgeSearchPreflight {
        workspace_generation: generation,
        vector_capability_ready,
        normalized_query,
        query_input_digest,
    })
}

pub(crate) async fn search_effect_status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit_ids: &[Uuid],
    required: bool,
) -> Result<KnowledgeEffectStatus> {
    let configured: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_resources') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if !configured {
        return Ok(KnowledgeEffectStatus::NotConfigured);
    }
    if !required {
        return Ok(KnowledgeEffectStatus::NotApplicable);
    }
    let closed: bool = sqlx::query_scalar(
        "SELECT NOT EXISTS(SELECT 1 FROM knowledge_unit_heads h WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=ANY($3) AND ((h.lifecycle='active' AND NOT h.payload_erased AND NOT EXISTS(SELECT 1 FROM knowledge_search_resources r WHERE r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision AND r.lifecycle='active')) OR (h.lifecycle<>'active' AND (EXISTS(SELECT 1 FROM knowledge_search_resources r WHERE r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id) OR EXISTS(SELECT 1 FROM knowledge_search_embedding_jobs j WHERE j.tenant_id=h.tenant_id AND j.workspace_id=h.workspace_id AND j.unit_id=h.unit_id)))))",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit_ids)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if !closed {
        return Ok(KnowledgeEffectStatus::Pending);
    }
    let table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if table {
        let clean:bool=sqlx::query_scalar("SELECT NOT EXISTS(SELECT 1 FROM knowledge_search_vectors v LEFT JOIN knowledge_search_resources r ON r.tenant_id=v.tenant_id AND r.workspace_id=v.workspace_id AND r.unit_id=v.unit_id LEFT JOIN knowledge_unit_heads h ON h.tenant_id=v.tenant_id AND h.workspace_id=v.workspace_id AND h.unit_id=v.unit_id WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=ANY($3) AND (r.unit_id IS NULL OR h.unit_id IS NULL OR h.lifecycle<>'active' OR NOT h.active OR h.payload_erased OR r.revision<>h.accepted_revision OR v.revision<>r.revision OR v.access_scope<>r.access_scope OR v.input_digest<>r.embedding_input_digest OR v.model_name<>'intfloat/multilingual-e5-small' OR v.model_revision<>'614241f622f53c4eeff9890bdc4f31cfecc418b3' OR v.dimensions<>384 OR v.recipe<>'title_v1'))")
            .bind(tenant).bind(workspace).bind(unit_ids).fetch_one(&mut **tx).await.map_err(storage_error)?;
        if !clean {
            return Ok(KnowledgeEffectStatus::Pending);
        }
    }
    let vector_ready = search_vector_ready(tx).await?;
    if !vector_ready {
        return Ok(KnowledgeEffectStatus::Ready);
    }
    if !table {
        return Ok(KnowledgeEffectStatus::Pending);
    }
    let sql = "SELECT NOT EXISTS(SELECT 1 FROM knowledge_unit_heads h JOIN knowledge_search_resources r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=ANY($3) AND h.lifecycle='active' AND h.active AND NOT h.payload_erased AND (EXISTS(SELECT 1 FROM knowledge_search_embedding_jobs j WHERE j.tenant_id=h.tenant_id AND j.workspace_id=h.workspace_id AND j.unit_id=h.unit_id) OR NOT EXISTS(SELECT 1 FROM knowledge_search_vectors v WHERE v.tenant_id=h.tenant_id AND v.workspace_id=h.workspace_id AND v.unit_id=h.unit_id AND v.revision=r.revision AND v.input_digest=r.embedding_input_digest AND v.access_scope=r.access_scope AND v.model_name='intfloat/multilingual-e5-small' AND v.model_revision='614241f622f53c4eeff9890bdc4f31cfecc418b3' AND v.dimensions=384 AND v.recipe='title_v1')))";
    let ready: bool = sqlx::query_scalar(sql)
        .bind(tenant)
        .bind(workspace)
        .bind(unit_ids)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    Ok(if ready {
        KnowledgeEffectStatus::Ready
    } else {
        KnowledgeEffectStatus::Pending
    })
}
