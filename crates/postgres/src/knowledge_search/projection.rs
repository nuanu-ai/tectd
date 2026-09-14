use super::*;

pub(crate) async fn apply_dk2_operation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    generation: i64,
) -> Result<()> {
    project_current(tx, tenant, workspace, principal, unit, generation, "dk-2").await
}

pub(crate) async fn project_legacy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    generation: i64,
) -> Result<()> {
    project_current(tx, tenant, workspace, principal, unit, generation, "dk-1").await
}

async fn project_current(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    generation: i64,
    expected_contract: &str,
) -> Result<()> {
    let head: Option<(String, i64, bool, String)> = sqlx::query_as(
        "SELECT lifecycle,accepted_revision,payload_erased,contract_version FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
    ).bind(tenant).bind(workspace).bind(unit).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((lifecycle, revision, erased, contract)) = head else {
        return invalidate_unit(tx, tenant, workspace, unit).await;
    };
    if lifecycle != "active" || erased {
        return invalidate_unit(tx, tenant, workspace, unit).await;
    }
    if contract != expected_contract {
        return Err(Error::InternalInvariant);
    }
    let resource = super::corpus::load_one(
        tx, tenant, workspace, principal, unit, revision, &contract, false,
    )
    .await?
    .ok_or(Error::InternalInvariant)?;
    upsert(tx, tenant, workspace, generation, &resource).await
}

async fn upsert(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    generation: i64,
    value: &super::corpus::SearchResource,
) -> Result<()> {
    let input_digest = sha256(&format!("passage: {}", value.title));
    sqlx::query("INSERT INTO knowledge_search_resources(tenant_id,workspace_id,unit_id,revision,resource_iri,revision_iri,title,canonical_text,knowledge_kind,lifecycle,access_scope,source_digests,freshness_warnings,contract_version,workspace_generation,embedding_input_digest,verified_payload_bytes) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17) ON CONFLICT(tenant_id,workspace_id,unit_id) DO UPDATE SET revision=EXCLUDED.revision,resource_iri=EXCLUDED.resource_iri,revision_iri=EXCLUDED.revision_iri,title=EXCLUDED.title,canonical_text=EXCLUDED.canonical_text,knowledge_kind=EXCLUDED.knowledge_kind,lifecycle=EXCLUDED.lifecycle,access_scope=EXCLUDED.access_scope,source_digests=EXCLUDED.source_digests,freshness_warnings=EXCLUDED.freshness_warnings,contract_version=EXCLUDED.contract_version,workspace_generation=EXCLUDED.workspace_generation,embedding_input_digest=EXCLUDED.embedding_input_digest,verified_payload_bytes=EXCLUDED.verified_payload_bytes,updated_at=pg_catalog.clock_timestamp()")
        .bind(tenant).bind(workspace).bind(value.unit_id).bind(value.revision)
        .bind(&value.resource_iri).bind(&value.revision_iri).bind(&value.title)
        .bind(&value.canonical_text).bind(enum_text(&value.kind)?)
        .bind(enum_text(&value.lifecycle)?).bind(enum_text(&value.access_scope)?)
        .bind(sqlx::types::Json(&value.source_digests))
        .bind(sqlx::types::Json(&value.freshness_warnings)).bind(&value.contract_version)
        .bind(generation).bind(&input_digest)
        .bind(i64::try_from(value.verified_payload_bytes).map_err(|_|Error::CapacityExceeded)?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    reconcile_embedding(
        tx,
        tenant,
        workspace,
        value.unit_id,
        value.revision,
        value.access_scope,
        generation,
        &input_digest,
    )
    .await?;
    crate::knowledge_lifecycle::erase::register_search_copies(tx, tenant, workspace, value.unit_id)
        .await
}

#[allow(clippy::too_many_arguments)]
async fn reconcile_embedding(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    access: KnowledgeAccessScope,
    generation: i64,
    input_digest: &str,
) -> Result<()> {
    let access = enum_text(&access)?;
    let table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (access_scope<>$4 OR input_digest<>$5 OR model_name<>$6 OR model_revision<>$7 OR dimensions<>$8 OR recipe<>$9)")
        .bind(tenant).bind(workspace).bind(unit).bind(&access).bind(input_digest)
        .bind(KNOWLEDGE_EMBEDDING_MODEL).bind(KNOWLEDGE_EMBEDDING_REVISION)
        .bind(KNOWLEDGE_EMBEDDING_DIMENSIONS as i32).bind(KNOWLEDGE_EMBEDDING_RECIPE)
        .execute(&mut **tx).await.map_err(storage_error)?;
    if table {
        sqlx::query("DELETE FROM knowledge_search_vectors WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (access_scope<>$4 OR input_digest<>$5 OR model_name<>$6 OR model_revision<>$7 OR dimensions<>$8 OR recipe<>$9)")
            .bind(tenant).bind(workspace).bind(unit).bind(&access).bind(input_digest)
            .bind(KNOWLEDGE_EMBEDDING_MODEL).bind(KNOWLEDGE_EMBEDDING_REVISION)
            .bind(KNOWLEDGE_EMBEDDING_DIMENSIONS as i32).bind(KNOWLEDGE_EMBEDDING_RECIPE)
            .execute(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("UPDATE knowledge_search_vectors SET revision=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND access_scope=$5 AND input_digest=$6 AND model_name=$7 AND model_revision=$8 AND dimensions=$9 AND recipe=$10")
            .bind(tenant).bind(workspace).bind(unit).bind(revision).bind(&access).bind(input_digest)
            .bind(KNOWLEDGE_EMBEDDING_MODEL).bind(KNOWLEDGE_EMBEDDING_REVISION)
            .bind(KNOWLEDGE_EMBEDDING_DIMENSIONS as i32).bind(KNOWLEDGE_EMBEDDING_RECIPE)
            .execute(&mut **tx).await.map_err(storage_error)?;
    }
    let vector_ready: bool = sqlx::query_scalar("SELECT tect_dk_search_vector_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !vector_ready {
        return Ok(());
    }
    if !table {
        return Err(Error::InvalidConfiguration);
    }
    let current: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_search_vectors WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND revision=$4 AND access_scope=$5 AND input_digest=$6 AND model_name=$7 AND model_revision=$8 AND dimensions=$9 AND recipe=$10)")
        .bind(tenant).bind(workspace).bind(unit).bind(revision).bind(&access).bind(input_digest)
        .bind(KNOWLEDGE_EMBEDDING_MODEL).bind(KNOWLEDGE_EMBEDDING_REVISION)
        .bind(KNOWLEDGE_EMBEDDING_DIMENSIONS as i32).bind(KNOWLEDGE_EMBEDDING_RECIPE)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if current {
        sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
        return Ok(());
    }
    sqlx::query("INSERT INTO knowledge_search_embedding_jobs(id,tenant_id,workspace_id,unit_id,revision,access_scope,workspace_generation,model_name,model_revision,dimensions,recipe,input_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) ON CONFLICT(tenant_id,workspace_id,unit_id,access_scope,model_name,model_revision,recipe,input_digest) DO UPDATE SET revision=EXCLUDED.revision,workspace_generation=EXCLUDED.workspace_generation,updated_at=pg_catalog.clock_timestamp()")
        .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(unit).bind(revision)
        .bind(access).bind(generation).bind(KNOWLEDGE_EMBEDDING_MODEL)
        .bind(KNOWLEDGE_EMBEDDING_REVISION).bind(KNOWLEDGE_EMBEDDING_DIMENSIONS as i32)
        .bind(KNOWLEDGE_EMBEDDING_RECIPE).bind(input_digest)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn invalidate_unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    let table: bool = sqlx::query_scalar(
        "SELECT pg_catalog.to_regclass('public.knowledge_search_vectors') IS NOT NULL",
    )
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if table {
        sqlx::query("DELETE FROM knowledge_search_vectors WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    }
    sqlx::query("DELETE FROM knowledge_search_resources WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE knowledge_owned_copies SET redacted=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND relation_name IN('knowledge_search_resources','knowledge_search_embedding_jobs','knowledge_search_vectors')")
        .bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}
