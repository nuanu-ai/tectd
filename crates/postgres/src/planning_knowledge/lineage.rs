use super::status::stale_reasons_for_manifest;
use super::*;

pub(crate) async fn register_manifest_lineage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Result<()> {
    let row: Option<(String, Uuid, Option<serde_json::Value>, bool)> = sqlx::query_as(
        "SELECT stage,owner_id,selected,payload_erased FROM planning_knowledge_manifests \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((stage, owner, selected, erased)) = row else {
        return Err(Error::NotFound);
    };
    if erased {
        return Err(Error::KnowledgePayloadErased);
    }
    let items: Vec<PlanningKnowledgeItem> = decode(selected.ok_or(Error::InternalInvariant)?)?;
    for item in items {
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_revision) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,'planning_manifest','planning_knowledge_manifests',$4,$5,0) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace).bind(item.unit_id).bind(manifest).bind(item.unit_revision)
            .execute(&mut **tx).await.map_err(storage_error)?;
        crate::knowledge_maintenance::register_consumer(
            tx,
            tenant,
            workspace,
            item.unit_id,
            item.unit_revision,
            &format!("{stage}:{owner}:{}", item.brief_local_id),
            item.purposes.iter().copied().any(blocking),
            "planning_knowledge_manifests",
            manifest,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn register_consumption(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
    relation: &str,
    row: Uuid,
    row_revision: i64,
) -> Result<()> {
    if row.is_nil() || row_revision < 1 {
        return Err(Error::InvalidArguments);
    }
    let consumption: Uuid = sqlx::query_scalar("INSERT INTO planning_knowledge_consumptions(id,tenant_id,workspace_id,manifest_id,relation_name,row_id,row_revision) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,$4,$5,$6) ON CONFLICT(tenant_id,workspace_id,manifest_id,relation_name,row_id,row_revision) DO UPDATE SET redacted=planning_knowledge_consumptions.redacted RETURNING id")
        .bind(tenant).bind(workspace).bind(manifest).bind(relation).bind(row).bind(row_revision)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    let units:Vec<(Uuid,i64)>=sqlx::query_as("SELECT (item->>'unit_id')::uuid,(item->>'unit_revision')::bigint FROM planning_knowledge_manifests m CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)) item WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND NOT m.payload_erased ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(manifest).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (unit, source_revision) in units {
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_revision) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,'planning_derived',$4,$5,$6,$7) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace).bind(unit).bind(relation).bind(row).bind(source_revision).bind(row_revision)
            .execute(&mut **tx).await.map_err(storage_error)?;
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_revision) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,'planning_consumption','planning_knowledge_consumptions',$4,$5,$6) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace).bind(unit).bind(consumption).bind(source_revision).bind(row_revision)
            .execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}

type ProgramRefreshReplayRow = (
    Option<serde_json::Value>,
    Option<serde_json::Value>,
    Option<Uuid>,
    bool,
);

pub(crate) async fn program_refresh_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: &RefreshProgramKnowledge,
) -> Result<Option<Program>> {
    let payload = json(request)?;
    let row: Option<ProgramRefreshReplayRow> = sqlx::query_as(
        "SELECT request_payload,result_payload,manifest_id,payload_erased \
             FROM program_knowledge_refresh_receipts WHERE tenant_id=$1 AND workspace_id=$2 \
             AND program_id=$3 AND request_id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.program_id)
    .bind(request.request_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    match row {
        None => Ok(None),
        Some((_, _, _, true)) => Err(Error::KnowledgePayloadErased),
        Some((Some(stored), Some(result), Some(manifest), false)) if stored == payload => {
            let erased: bool = sqlx::query_scalar(
                "SELECT payload_erased FROM planning_knowledge_manifests \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(manifest)
            .fetch_optional(&mut **tx)
            .await
            .map_err(storage_error)?
            .ok_or(Error::InternalInvariant)?;
            if erased {
                return Err(Error::KnowledgePayloadErased);
            }
            require_manifest_access(tx, tenant, workspace, principal, manifest).await?;
            Ok(Some(decode(result)?))
        }
        Some((Some(_), Some(_), Some(_), false)) => Err(Error::InputConflict),
        Some(_) => Err(Error::InternalInvariant),
    }
}

pub(crate) async fn save_program_refresh_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    request: &RefreshProgramKnowledge,
    program: &Program,
) -> Result<()> {
    let manifest = program
        .planning_knowledge
        .as_ref()
        .and_then(|v| v.manifest.as_ref())
        .ok_or(Error::InternalInvariant)?;
    sqlx::query(
        "INSERT INTO program_knowledge_refresh_receipts \
         (tenant_id,workspace_id,program_id,request_id,request_payload,result_revision,manifest_id,result_payload) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8)",
    )
    .bind(tenant).bind(workspace).bind(request.program_id).bind(request.request_id)
    .bind(json(request)?).bind(program.revision).bind(manifest.id).bind(json(program)?)
    .execute(&mut **tx).await.map_err(storage_error)?;
    let units: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT unit_id,MAX(source_revision) FROM ( \
           SELECT unit_id,source_revision FROM knowledge_owned_copies \
            WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='programs' \
              AND row_id=$3 AND NOT redacted \
           UNION ALL \
           SELECT (item->>'unit_id')::uuid,(item->>'unit_revision')::bigint \
            FROM planning_knowledge_manifests m CROSS JOIN LATERAL \
             pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)) item \
            WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$4 AND NOT m.payload_erased \
         ) owned GROUP BY unit_id ORDER BY unit_id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(program.id)
    .bind(manifest.id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for (unit_id, source_revision) in units {
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_revision,row_operation,row_request_id) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,'planning_refresh_receipt','program_knowledge_refresh_receipts',$4,$5,$6,'refresh',$7) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace).bind(unit_id).bind(program.id).bind(source_revision)
            .bind(program.revision).bind(request.request_id).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}

pub(crate) async fn consumption_status(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    relation: &str,
    row_id: Uuid,
    row_revision: i64,
) -> Result<Option<PlanningKnowledgeStatus>> {
    let row = sqlx::query(
        "SELECT m.id,m.stage,m.owner_id,m.owner_revision,m.input_revision,m.request_id, \
         m.policy_id,m.policy_version,m.task_context_digest,m.task_context,m.workspace_generation, \
         m.needs,m.selected,m.unresolved_needs,m.digest,m.payload_erased \
         FROM planning_knowledge_consumptions c JOIN planning_knowledge_manifests m \
           ON m.tenant_id=c.tenant_id AND m.workspace_id=c.workspace_id AND m.id=c.manifest_id \
         WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.relation_name=$3 \
           AND c.row_id=$4 AND c.row_revision=$5 ORDER BY c.created_at DESC,c.id DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(relation)
    .bind(row_id)
    .bind(row_revision)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    if row.try_get::<bool, _>(15).map_err(storage_error)? {
        return Err(Error::KnowledgePayloadErased);
    }
    let id: Uuid = row.try_get(0).map_err(storage_error)?;
    require_manifest_access(tx, tenant, workspace, principal, id).await?;
    let stage: PlanningStage = decode(serde_json::Value::String(
        row.try_get(1).map_err(storage_error)?,
    ))?;
    let manifest = PlanningKnowledgeManifest {
        id,
        stage,
        owner_id: row.try_get(2).map_err(storage_error)?,
        owner_revision: row.try_get(3).map_err(storage_error)?,
        input_revision: row.try_get(4).map_err(storage_error)?,
        request_id: row.try_get(5).map_err(storage_error)?,
        policy_id: row.try_get(6).map_err(storage_error)?,
        policy_version: row.try_get(7).map_err(storage_error)?,
        task_context_digest: row.try_get(8).map_err(storage_error)?,
        task_context: decode(row.try_get(9).map_err(storage_error)?)?,
        workspace_generation: row.try_get(10).map_err(storage_error)?,
        needs: decode(row.try_get(11).map_err(storage_error)?)?,
        selected: decode(row.try_get(12).map_err(storage_error)?)?,
        unresolved_needs: decode(row.try_get(13).map_err(storage_error)?)?,
        digest: row.try_get(14).map_err(storage_error)?,
    };
    let (stale_reasons, warnings) =
        stale_reasons_for_manifest(tx, tenant, workspace, principal, &manifest).await?;
    Ok(Some(PlanningKnowledgeStatus {
        manifest: Some(manifest),
        stale_reasons,
        warnings,
    }))
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn register_receipt_copy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
    relation: &str,
    row_id: Uuid,
    row_revision: i64,
    operation: &str,
    request: Uuid,
) -> Result<()> {
    if !matches!(
        relation,
        "scope_candidate_receipts" | "native_planning_receipts"
    ) || row_id.is_nil()
        || row_revision < 1
        || operation.is_empty()
        || request.is_nil()
    {
        return Err(Error::InvalidArguments);
    }
    let units: Vec<(Uuid,i64)> = sqlx::query_as(
        "SELECT (item->>'unit_id')::uuid,(item->>'unit_revision')::bigint \
         FROM planning_knowledge_manifests m CROSS JOIN LATERAL \
         pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)) item \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND NOT m.payload_erased ORDER BY 1,2",
    )
    .bind(tenant).bind(workspace).bind(manifest).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (unit, source_revision) in units {
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision,row_revision,row_operation,row_request_id) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,'planning_derived_receipt',$4,$5,$6,$7,$8,$9) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace).bind(unit).bind(relation).bind(row_id).bind(source_revision)
            .bind(row_revision).bind(operation).bind(request).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}
