use super::*;

const MANIFEST_RELATION: &str = "pipeline_knowledge_manifests";
const PLANNING_MANIFEST_RELATION: &str = "planning_knowledge_manifests";
const MAX_CONSUMERS_PER_REVISION: usize = 512;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn register_consumer(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
    consumer_ref: &str,
    required: bool,
    relation_name: &str,
    row_id: Uuid,
) -> Result<()> {
    if !matches!(
        relation_name,
        MANIFEST_RELATION | PLANNING_MANIFEST_RELATION
    ) || consumer_ref.is_empty()
        || consumer_ref.len() > 4096
        || !consumer_row_current(tx, tenant, workspace, relation_name, row_id).await?
    {
        return Err(Error::InvalidArguments);
    }
    let candidate_id = Uuid::new_v4();
    let id: Uuid = sqlx::query_scalar(
        "INSERT INTO knowledge_maintenance_consumers \
         (id,tenant_id,workspace_id,unit_id,unit_revision,consumer_ref,required,relation_name,row_id) \
         VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9) \
         ON CONFLICT(tenant_id,workspace_id,unit_id,unit_revision,consumer_ref,relation_name,row_id) \
         DO UPDATE SET active=true,required=(knowledge_maintenance_consumers.required OR EXCLUDED.required) RETURNING id",
    )
    .bind(candidate_id)
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .bind(consumer_ref)
    .bind(required)
    .bind(relation_name)
    .bind(row_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    sqlx::query(
        "INSERT INTO knowledge_owned_copies \
         (id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,source_revision) \
         VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,'maintenance_consumer', \
         'knowledge_maintenance_consumers',$4,$5) ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(id)
    .bind(revision)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn registered_consumers(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
) -> Result<Vec<KnowledgeMaintenanceConsumer>> {
    let rows: Vec<(String, bool, String, Uuid)> = sqlx::query_as(
        "SELECT consumer_ref,required,relation_name,row_id \
         FROM knowledge_maintenance_consumers WHERE tenant_id=$1 AND workspace_id=$2 \
         AND unit_id=$3 AND unit_revision=$4 AND active ORDER BY consumer_ref,relation_name,row_id \
         LIMIT 513",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .bind(revision)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    if rows.len() > MAX_CONSUMERS_PER_REVISION {
        return Err(Error::NeedsContext);
    }
    let mut consumers = Vec::with_capacity(rows.len());
    for (consumer_ref, required, relation_name, row_id) in rows {
        if consumer_row_current(tx, tenant, workspace, &relation_name, row_id).await? {
            consumers.push(KnowledgeMaintenanceConsumer {
                consumer_ref,
                required,
                relation_name,
                row_id,
            });
        }
    }
    Ok(consumers)
}

pub(crate) async fn retire_planning_owner_consumers(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    stage: &str,
    owner: Uuid,
    keep_manifest: Uuid,
) -> Result<()> {
    if !matches!(stage, "program" | "scope" | "slice_candidates")
        || owner.is_nil()
        || keep_manifest.is_nil()
    {
        return Err(Error::InvalidArguments);
    }
    sqlx::query(
        "UPDATE knowledge_maintenance_consumers c SET active=false \
         FROM planning_knowledge_manifests m WHERE c.tenant_id=$1 AND c.workspace_id=$2 \
          AND c.relation_name='planning_knowledge_manifests' AND c.row_id=m.id \
          AND m.tenant_id=c.tenant_id AND m.workspace_id=c.workspace_id \
          AND m.stage=$3 AND m.owner_id=$4 AND m.id<>$5 AND NOT m.payload_erased AND c.active",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(stage)
    .bind(owner)
    .bind(keep_manifest)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

async fn consumer_row_current(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    relation: &str,
    row: Uuid,
) -> Result<bool> {
    let query = match relation {
        MANIFEST_RELATION => {
            "SELECT EXISTS(SELECT 1 FROM pipeline_knowledge_manifests m \
             JOIN slice_pipeline_runs r ON r.tenant_id=m.tenant_id AND r.workspace_id=m.workspace_id \
              AND r.id=m.run_id WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 \
              AND NOT m.payload_erased AND NOT r.payload_erased \
              AND (r.knowledge_manifest_id=m.id OR (r.revision=m.run_revision \
               AND r.current_phase_id=m.phase_id)))"
        }
        PLANNING_MANIFEST_RELATION => {
            "SELECT EXISTS(SELECT 1 FROM planning_knowledge_manifests \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT payload_erased)"
        }
        _ => return Ok(false),
    };
    sqlx::query_scalar(query)
        .bind(tenant)
        .bind(workspace)
        .bind(row)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)
}

pub(crate) async fn register_manifest_consumers(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Result<()> {
    sqlx::query(
        "UPDATE knowledge_maintenance_consumers c SET active=false \
         FROM pipeline_knowledge_manifests previous,pipeline_knowledge_manifests current \
         WHERE current.tenant_id=$1 AND current.workspace_id=$2 AND current.id=$3 \
          AND previous.tenant_id=current.tenant_id AND previous.workspace_id=current.workspace_id \
          AND previous.run_id=current.run_id AND previous.id<>current.id \
          AND c.tenant_id=current.tenant_id AND c.workspace_id=current.workspace_id \
          AND c.relation_name='pipeline_knowledge_manifests' AND c.row_id=previous.id AND c.active",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    let rows: Vec<(Uuid, i64, bool)> = sqlx::query_as(
        "SELECT (item->>'unit_id')::uuid,(item->>'revision')::bigint, \
         pg_catalog.bool_or(COALESCE(item#>>'{binding,purpose}','required')<>'reference') \
         FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL \
          pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb) \
           ||COALESCE(m.selected_resources,'[]'::jsonb)) item \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND NOT m.payload_erased \
          AND item?'unit_id' AND item?'revision' GROUP BY 1,2 ORDER BY 1,2 LIMIT 513",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(manifest)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    if rows.len() > MAX_CONSUMERS_PER_REVISION {
        return Err(Error::CapacityExceeded);
    }
    let consumer_ref = format!("pipeline-manifest:{manifest}");
    for (unit, revision, required) in rows {
        register_consumer(
            tx,
            tenant,
            workspace,
            unit,
            revision,
            &consumer_ref,
            required,
            MANIFEST_RELATION,
            manifest,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn reconcile_unit_consumers(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let manifests: Vec<Uuid> = sqlx::query_scalar(
        "SELECT DISTINCT m.id FROM pipeline_knowledge_manifests m \
          JOIN slice_pipeline_runs r ON r.tenant_id=m.tenant_id AND r.workspace_id=m.workspace_id \
           AND r.id=m.run_id CROSS JOIN LATERAL \
          pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb) \
           ||COALESCE(m.selected_resources,'[]'::jsonb)) item \
         WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND NOT m.payload_erased \
          AND NOT r.payload_erased AND (r.knowledge_manifest_id=m.id OR (r.revision=m.run_revision \
           AND r.current_phase_id=m.phase_id)) \
          AND item->>'unit_id'=$3::text ORDER BY m.id LIMIT 513",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    if manifests.len() > MAX_CONSUMERS_PER_REVISION {
        return Err(Error::CapacityExceeded);
    }
    for manifest in manifests {
        register_manifest_consumers(tx, tenant, workspace, manifest).await?;
    }
    Ok(())
}
