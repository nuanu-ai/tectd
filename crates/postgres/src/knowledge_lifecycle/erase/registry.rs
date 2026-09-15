use super::*;

pub(super) use super::relation::CopyRelation;

#[allow(clippy::too_many_arguments)]
pub(super) async fn register(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: CopyRelation,
    row: Uuid,
    revision: i64,
) -> Result<()> {
    register_exact(
        tx, tenant, workspace, unit, kind, relation, row, revision, None, None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register_exact(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: CopyRelation,
    row: Uuid,
    revision: i64,
    operation: Option<&str>,
    request: Option<Uuid>,
) -> Result<()> {
    sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision,row_operation,row_request_id) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(unit).bind(kind).bind(relation.name()).bind(row).bind(revision)
        .bind(operation).bind(request)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: CopyRelation,
    owner: Uuid,
    operation: &str,
    request: Uuid,
) -> Result<()> {
    register_exact(
        tx,
        tenant,
        workspace,
        unit,
        kind,
        relation,
        owner,
        0,
        Some(operation),
        Some(request),
    )
    .await
}

async fn units_for_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Result<Vec<(Uuid, i64)>> {
    sqlx::query_as("SELECT DISTINCT (item->>'unit_id')::uuid,(item->>'revision')::bigint FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)||COALESCE(m.selected_resources,'[]'::jsonb)) item WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND NOT m.payload_erased AND item ? 'unit_id' AND item ? 'revision' ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(manifest).fetch_all(&mut **tx).await.map_err(storage_error)
}

pub(crate) async fn register_pipeline_manifest_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Result<()> {
    for (unit, revision) in units_for_manifest(tx, tenant, workspace, manifest).await? {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_manifest",
            CopyRelation::Manifest,
            manifest,
            revision,
        )
        .await?;
    }
    crate::knowledge_maintenance::register_manifest_consumers(tx, tenant, workspace, manifest)
        .await?;
    Ok(())
}

pub(crate) async fn register_pipeline_run_origin_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    manifest: Uuid,
) -> Result<()> {
    for (unit, revision) in units_for_manifest(tx, tenant, workspace, manifest).await? {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_run_origin",
            CopyRelation::PipelineRun,
            run,
            revision,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn register_checkpoint_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    checkpoint: Uuid,
    attempt: Uuid,
) -> Result<()> {
    let units: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT DISTINCT unit_id,COALESCE(source_revision,0) FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='slice_pipeline_phase_attempts' AND row_id=$3 AND NOT redacted ORDER BY 1,2",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(attempt)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for (unit, revision) in units {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "research_checkpoint",
            CopyRelation::ResearchCheckpoint,
            checkpoint,
            revision,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn register_checkpoint_consumer_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    checkpoint: Uuid,
    run: Uuid,
) -> Result<()> {
    let units: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT DISTINCT unit_id,COALESCE(source_revision,0) FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='pipeline_research_checkpoints' AND row_id=$3 AND NOT redacted ORDER BY 1,2",
    )
    .bind(tenant).bind(workspace).bind(checkpoint)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (unit, revision) in units {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "checkpoint_consumer",
            CopyRelation::PipelineRun,
            run,
            revision,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn register_checkpoint_resolution_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    checkpoint: Uuid,
    input: Uuid,
    request: Uuid,
) -> Result<()> {
    let units: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT DISTINCT unit_id,source_revision FROM (SELECT c.unit_id,COALESCE(c.source_revision,0) source_revision FROM knowledge_owned_copies c WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.relation_name='pipeline_research_checkpoints' AND c.row_id=$3 AND NOT c.redacted UNION SELECT c.unit_id,COALESCE(c.source_revision,0) FROM pipeline_research_checkpoints p JOIN knowledge_owned_copies c ON c.tenant_id=p.tenant_id AND c.workspace_id=p.workspace_id AND ((c.relation_name='slice_results' AND c.row_id=p.consumer_result_id) OR (c.relation_name='slice_pipeline_phase_outputs' AND c.row_id=p.consumer_terminal_output_id)) AND NOT c.redacted WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND p.id=$3) owned ORDER BY 1,2",
    )
    .bind(tenant).bind(workspace).bind(checkpoint)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let owners = units.iter().map(|value| value.0).collect::<Vec<_>>();
    for (unit, revision) in units {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "checkpoint_terminal",
            CopyRelation::ResearchCheckpoint,
            checkpoint,
            revision,
        )
        .await?;
        register(
            tx,
            tenant,
            workspace,
            unit,
            "checkpoint_return",
            CopyRelation::PipelineInput,
            input,
            revision,
        )
        .await?;
        register_receipt(
            tx,
            tenant,
            workspace,
            unit,
            "checkpoint_resolution",
            CopyRelation::CheckpointReceipt,
            checkpoint,
            "resolve",
            request,
        )
        .await?;
    }
    sqlx::query("UPDATE slice_pipeline_inputs SET owner_unit_ids=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(input).bind(&owners).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE pipeline_checkpoint_receipts SET owner_unit_ids=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request).bind(&owners).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

mod extended;

pub(super) use extended::register_propagated;
pub(crate) use extended::{
    register_knowledge_change_input_copies, register_knowledge_change_output_copies,
    register_pipeline_input_copies, register_pipeline_phase_copies,
    register_pipeline_receipt_copies,
};
