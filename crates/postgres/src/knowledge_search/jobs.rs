use super::*;

type JobBasis = (Uuid, i64, String, i64, String, String, String, i32, String);

pub(crate) async fn claim(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    limit: u32,
    model: &KnowledgeEmbeddingModelIdentity,
) -> Result<Vec<KnowledgeEmbeddingJobClaim>> {
    crate::knowledge_lifecycle::require_owner(tx, principal).await?;
    model.validate()?;
    if limit == 0 || limit > 64 {
        return Err(Error::InvalidArguments);
    }
    if !search_vector_ready(tx).await? {
        return Ok(Vec::new());
    }
    crate::durable_knowledge::publisher_gate(tx).await?;
    let _ = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND (model_name<>$3 OR model_revision<>$4 OR dimensions<>$5 OR recipe<>$6)")
        .bind(tenant).bind(workspace).bind(&model.name).bind(&model.revision)
        .bind(model.dimensions as i32).bind(&model.recipe)
        .execute(&mut **tx).await.map_err(storage_error)?;
    let ids: Vec<(Uuid, Uuid)> = sqlx::query_as("SELECT id,unit_id FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND available_at<=pg_catalog.clock_timestamp() AND (state='pending' OR lease_expires_at<=pg_catalog.clock_timestamp()) ORDER BY available_at,id LIMIT $3")
        .bind(tenant).bind(workspace).bind(i64::from(limit))
        .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut claims = Vec::new();
    for (job, unit) in ids {
        let Some(basis) = lock_basis(tx, tenant, workspace, job, unit).await? else {
            continue;
        };
        if basis.5 != model.name
            || basis.6 != model.revision
            || basis.7 != model.dimensions as i32
            || basis.8 != model.recipe
        {
            sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                .bind(tenant).bind(workspace).bind(job).execute(&mut **tx).await.map_err(storage_error)?;
            crate::knowledge_lifecycle::erase::register_search_copies(tx, tenant, workspace, unit)
                .await?;
            continue;
        }
        let Some((title, current_revision)) =
            verified_title(tx, tenant, workspace, principal, unit, &basis.2, &basis.4).await?
        else {
            sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                .bind(tenant).bind(workspace).bind(job).execute(&mut **tx).await.map_err(storage_error)?;
            crate::knowledge_lifecycle::erase::register_search_copies(tx, tenant, workspace, unit)
                .await?;
            continue;
        };
        let lease = Uuid::new_v4();
        let updated = sqlx::query("UPDATE knowledge_search_embedding_jobs SET revision=$4,state='leased',lease_token=$5,lease_expires_at=pg_catalog.clock_timestamp()+interval '60 seconds',attempts=attempts+1,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND (state='pending' OR lease_expires_at<=pg_catalog.clock_timestamp())")
            .bind(tenant).bind(workspace).bind(job).bind(current_revision).bind(lease)
            .execute(&mut **tx).await.map_err(storage_error)?.rows_affected();
        if updated == 1 {
            claims.push(KnowledgeEmbeddingJobClaim {
                job_id: job,
                lease_token: lease,
                workspace_id: workspace,
                principal_id: principal,
                unit_id: unit,
                revision: current_revision,
                title,
                input_digest: basis.4,
                model: model.clone(),
            });
        }
    }
    Ok(claims)
}

async fn lock_basis(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    job: Uuid,
    unit: Uuid,
) -> Result<Option<JobBasis>> {
    let head: Option<(Uuid,)> = sqlx::query_as("SELECT unit_id FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(unit).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if head.is_none() {
        return Ok(None);
    }
    sqlx::query_as("SELECT unit_id,revision,access_scope,workspace_generation,input_digest,model_name,model_revision,dimensions,recipe FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(job).fetch_optional(&mut **tx).await.map_err(storage_error)
}

async fn verified_title(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    unit: Uuid,
    access: &str,
    input_digest: &str,
) -> Result<Option<(String, i64)>> {
    let row: Option<(String, i64, String, String, bool)> = sqlx::query_as("SELECT contract_version,accepted_revision,access_scope,lifecycle,payload_erased FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((contract, revision, head_access, lifecycle, erased)) = row else {
        return Ok(None);
    };
    if lifecycle != "active" || erased {
        return Ok(None);
    }
    let title = if contract == "dk-2" {
        let response = crate::knowledge_lifecycle::unit(
            tx,
            tenant,
            workspace,
            principal,
            &KnowledgeUnitQuery {
                unit_id: unit,
                revision: Some(revision),
                fragment: None,
            },
        )
        .await?
        .ok_or(Error::InternalInvariant)?;
        let KnowledgeUnitResponse::Document(value) = response else {
            return Ok(None);
        };
        if enum_text(&value.document.access_scope)? != head_access {
            return Err(Error::InternalInvariant);
        }
        value.document.title.clone()
    } else if contract == "dk-1" {
        let value = crate::durable_knowledge::context::load_revision(
            tx,
            tenant,
            workspace,
            unit,
            Some(revision),
            true,
        )
        .await?
        .ok_or(Error::InternalInvariant)?;
        value.constraint.title
    } else {
        return Err(Error::InternalInvariant);
    };
    let revision_access: String = sqlx::query_scalar("SELECT access_scope FROM knowledge_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND revision=$4")
        .bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let effective_access = if head_access == "owners_only" || revision_access == "owners_only" {
        "owners_only"
    } else {
        "workspace_members"
    };
    if access != effective_access || sha256(&format!("passage: {title}")) != input_digest {
        return Ok(None);
    }
    Ok(Some((title, revision)))
}

pub(crate) async fn complete(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    completion: &KnowledgeEmbeddingJobCompletion,
) -> Result<bool> {
    crate::knowledge_lifecycle::require_owner(tx, principal).await?;
    completion.model.validate()?;
    validate_values(&completion.values)?;
    if !search_vector_ready(tx).await? {
        return Ok(false);
    }
    crate::durable_knowledge::publisher_gate(tx).await?;
    let _ = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let identity: Option<(Uuid,)> = sqlx::query_as("SELECT unit_id FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(completion.job_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((unit,)) = identity else {
        return Ok(false);
    };
    let Some(basis) = lock_basis(tx, tenant, workspace, completion.job_id, unit).await? else {
        return Ok(false);
    };
    let lease: Option<(Uuid, bool)> = sqlx::query_as("SELECT lease_token,lease_expires_at>pg_catalog.clock_timestamp() FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='leased'")
        .bind(tenant).bind(workspace).bind(completion.job_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((lease, live)) = lease else {
        return Ok(false);
    };
    if lease != completion.lease_token
        || !live
        || basis.4 != completion.input_digest
        || basis.5 != completion.model.name
        || basis.6 != completion.model.revision
        || basis.7 != completion.model.dimensions as i32
        || basis.8 != completion.model.recipe
    {
        return Ok(false);
    }
    let Some((_, current_revision)) =
        verified_title(tx, tenant, workspace, principal, unit, &basis.2, &basis.4).await?
    else {
        return Ok(false);
    };
    let literal = format!(
        "[{}]",
        completion
            .values
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let inserted = sqlx::query("INSERT INTO knowledge_search_vectors(tenant_id,workspace_id,unit_id,revision,access_scope,model_name,model_revision,dimensions,recipe,input_digest,embedding) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11::vector) ON CONFLICT(tenant_id,workspace_id,unit_id) DO UPDATE SET revision=EXCLUDED.revision,access_scope=EXCLUDED.access_scope,model_name=EXCLUDED.model_name,model_revision=EXCLUDED.model_revision,dimensions=EXCLUDED.dimensions,recipe=EXCLUDED.recipe,input_digest=EXCLUDED.input_digest,embedding=EXCLUDED.embedding,updated_at=pg_catalog.clock_timestamp()")
        .bind(tenant).bind(workspace).bind(unit).bind(current_revision).bind(&basis.2)
        .bind(&completion.model.name).bind(&completion.model.revision)
        .bind(completion.model.dimensions as i32).bind(&completion.model.recipe)
        .bind(&completion.input_digest).bind(literal)
        .execute(&mut **tx).await.map_err(storage_error)?.rows_affected();
    if inserted != 1 {
        return Ok(false);
    }
    let deleted = sqlx::query("DELETE FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND lease_token=$4")
        .bind(tenant).bind(workspace).bind(completion.job_id).bind(completion.lease_token)
        .execute(&mut **tx).await.map_err(storage_error)?.rows_affected();
    crate::knowledge_lifecycle::erase::register_search_copies(tx, tenant, workspace, unit).await?;
    Ok(deleted == 1)
}

fn validate_values(values: &[f32]) -> Result<()> {
    if values.len() != KNOWLEDGE_EMBEDDING_DIMENSIONS as usize
        || values.iter().any(|value| !value.is_finite())
    {
        return Err(Error::InvalidArguments);
    }
    let norm = values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    ((norm - 1.0).abs() <= 0.001)
        .then_some(())
        .ok_or(Error::InvalidArguments)
}

pub(crate) async fn fail(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    job: Uuid,
    lease: Uuid,
) -> Result<bool> {
    crate::knowledge_lifecycle::require_owner(tx, principal).await?;
    crate::durable_knowledge::publisher_gate(tx).await?;
    let _ = crate::durable_knowledge::lock_state(tx, tenant, workspace).await?;
    let identity: Option<(Uuid,)> = sqlx::query_as("SELECT unit_id FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(job).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((unit,)) = identity else {
        return Ok(false);
    };
    let Some(_) = lock_basis(tx, tenant, workspace, job, unit).await? else {
        return Ok(false);
    };
    let rows = sqlx::query("UPDATE knowledge_search_embedding_jobs SET state='pending',lease_token=NULL,lease_expires_at=NULL,available_at=pg_catalog.clock_timestamp()+LEAST(attempts,6)*interval '5 seconds',updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='leased' AND lease_token=$4")
        .bind(tenant).bind(workspace).bind(job).bind(lease)
        .execute(&mut **tx).await.map_err(storage_error)?.rows_affected();
    Ok(rows == 1)
}

pub(crate) async fn pending(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    model: &KnowledgeEmbeddingModelIdentity,
) -> Result<u32> {
    crate::knowledge_lifecycle::require_owner(tx, principal).await?;
    model.validate()?;
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2 AND model_name=$3 AND model_revision=$4 AND dimensions=$5 AND recipe=$6")
        .bind(tenant).bind(workspace).bind(&model.name).bind(&model.revision)
        .bind(model.dimensions as i32).bind(&model.recipe)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    u32::try_from(count).map_err(|_| Error::CapacityExceeded)
}
