use super::registry::{CopyRelation, register, register_receipt};
use super::*;

const MAX_OWNED_COPIES: i64 = 4096;

async fn register_change_for_unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    unit: Uuid,
) -> Result<()> {
    register(
        tx,
        tenant,
        workspace,
        unit,
        "change_control",
        CopyRelation::LifecycleChange,
        change,
        0,
    )
    .await?;
    let operations:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3")
        .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for row in operations {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleOperation,
            row,
            0,
        )
        .await?;
    }
    let runs:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3")
        .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for run in &runs {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleRun,
            *run,
            0,
        )
        .await?;
        let rows:Vec<(Uuid,Uuid)>=sqlx::query_as("SELECT o.id,a.id FROM knowledge_change_outputs o JOIN knowledge_change_attempts a ON a.tenant_id=o.tenant_id AND a.workspace_id=o.workspace_id AND a.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3")
            .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
        for (output, attempt) in rows {
            register(
                tx,
                tenant,
                workspace,
                unit,
                "change_control",
                CopyRelation::LifecycleOutput,
                output,
                0,
            )
            .await?;
            register(
                tx,
                tenant,
                workspace,
                unit,
                "change_control",
                CopyRelation::LifecycleAttempt,
                attempt,
                0,
            )
            .await?;
        }
        let inputs:Vec<(Uuid,Uuid)>=sqlx::query_as("SELECT id,request_id FROM knowledge_change_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3")
            .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
        for (input, request) in inputs {
            register(
                tx,
                tenant,
                workspace,
                unit,
                "change_control",
                CopyRelation::LifecycleInput,
                input,
                0,
            )
            .await?;
            register_receipt(
                tx,
                tenant,
                workspace,
                unit,
                "change_control",
                CopyRelation::LifecycleReceipt,
                change,
                "record_input",
                request,
            )
            .await?;
        }
    }
    let receipts:Vec<(String,Uuid)>=sqlx::query_as("SELECT operation,request_id FROM knowledge_lifecycle_command_receipts r WHERE tenant_id=$1 AND workspace_id=$2 AND (erased_change_id=$3 OR request_payload->>'change_id'=$3::text OR request_payload->>'run_id'=ANY($4::text[]) OR (operation='begin' AND request_id=(SELECT request_id FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)))")
        .bind(tenant).bind(workspace).bind(change).bind(runs.iter().map(Uuid::to_string).collect::<Vec<_>>()).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (operation, request) in receipts {
        register_receipt(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleReceipt,
            change,
            &operation,
            request,
        )
        .await?;
    }
    Ok(())
}

async fn direct_change_rows(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
) -> Result<Vec<Uuid>> {
    let units:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT unit_id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 ORDER BY unit_id")
        .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for unit in &units {
        register_change_for_unit(tx, tenant, workspace, change, *unit).await?;
    }
    Ok(units)
}

async fn register_legacy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let changes: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_changes WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for change in changes {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "legacy_change",
            CopyRelation::LegacyChange,
            change,
            0,
        )
        .await?;
        let receipts:Vec<(String,Uuid)>=sqlx::query_as("SELECT operation,request_id FROM knowledge_command_receipts r WHERE tenant_id=$1 AND workspace_id=$2 AND (request_payload->>'change_id'=$3::text OR result_payload->'prepared'->>'id'=$3::text OR result_payload->'approved'->>'id'=$3::text OR result_payload->'rejected'->>'id'=$3::text OR result_payload->'replay'->>'id'=$3::text OR result_payload->'published'->>'change_id'=$3::text OR result_payload->'replay'->>'change_id'=$3::text)")
            .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
        for (operation, request) in receipts {
            register_receipt(
                tx,
                tenant,
                workspace,
                unit,
                "legacy_receipt",
                CopyRelation::LegacyReceipt,
                change,
                &operation,
                request,
            )
            .await?;
        }
    }
    Ok(())
}

pub(super) async fn reconcile_unit_direct(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let target:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT change_id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for change in target {
        direct_change_rows(tx, tenant, workspace, change).await?;
    }
    let sourced:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT change_id FROM (SELECT ch.id change_id FROM knowledge_lifecycle_changes ch CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(ch.sources,'[]'::jsonb)) source JOIN slice_pipeline_phase_outputs o ON o.tenant_id=ch.tenant_id AND o.workspace_id=ch.workspace_id AND o.run_id=NULLIF(source->'output'->>'run_id','')::uuid AND o.id=NULLIF(source->'output'->>'output_id','')::uuid AND o.body_digest=source->'output'->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_pipeline_phase_outputs' AND c.row_id=o.id WHERE ch.tenant_id=$1 AND ch.workspace_id=$2 AND source->>'kind'='pipeline_output' UNION ALL SELECT ch.id FROM knowledge_lifecycle_changes ch CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(ch.sources,'[]'::jsonb)) source JOIN knowledge_change_outputs o ON o.tenant_id=ch.tenant_id AND o.workspace_id=ch.workspace_id AND o.run_id=NULLIF(source->'output'->>'run_id','')::uuid AND o.id=NULLIF(source->'output'->>'output_id','')::uuid AND o.digest=source->'output'->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.unit_id=$3 AND c.relation_name='knowledge_change_outputs' AND c.row_id=o.id WHERE ch.tenant_id=$1 AND ch.workspace_id=$2 AND source->>'kind'='pipeline_output') inherited")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for change in sourced {
        register_change_for_unit(tx, tenant, workspace, change, unit).await?;
    }
    let pipeline_runs:Vec<(Uuid,Uuid)>=sqlx::query_as("SELECT DISTINCT r.id,m.id FROM slice_pipeline_runs r JOIN pipeline_knowledge_manifests m ON m.tenant_id=r.tenant_id AND m.workspace_id=r.workspace_id AND m.id=COALESCE(NULLIF(r.origin_result->'created'->'knowledge_resources'->>'id','')::uuid,NULLIF(r.origin_result->'created'->'knowledge'->>'id','')::uuid,NULLIF(r.origin_result->'replay'->'knowledge_resources'->>'id','')::uuid,NULLIF(r.origin_result->'replay'->'knowledge'->>'id','')::uuid) JOIN knowledge_owned_copies c ON c.tenant_id=m.tenant_id AND c.workspace_id=m.workspace_id AND c.unit_id=$3 AND c.relation_name='pipeline_knowledge_manifests' AND c.row_id=m.id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND NOT r.payload_erased")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (run, manifest) in pipeline_runs {
        registry::register_pipeline_run_origin_copies(tx, tenant, workspace, run, manifest).await?;
    }
    register_legacy(tx, tenant, workspace, unit).await
}

pub(crate) async fn reconcile_change_owned_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
) -> Result<Vec<Uuid>> {
    let units = direct_change_rows(tx, tenant, workspace, change).await?;
    for unit in units {
        super::propagate::reconcile_unit(tx, tenant, workspace, unit).await?;
    }
    let ids:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM knowledge_owned_copies c WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id IN (SELECT unit_id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3) AND NOT redacted ORDER BY id")
        .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    if ids.len() as i64 > MAX_OWNED_COPIES {
        return Err(Error::CapacityExceeded);
    }
    Ok(ids)
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register_planning_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    entity: Uuid,
    operation: &str,
    request: Uuid,
) -> Result<()> {
    registry::register_exact(
        tx,
        tenant,
        workspace,
        unit,
        "planning_receipt",
        CopyRelation::PlanningReceipt,
        entity,
        0,
        Some(operation),
        Some(request),
    )
    .await
}

pub(super) async fn register_pipeline_receipt_exact(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    run: Uuid,
    operation: &str,
    request: Uuid,
) -> Result<()> {
    registry::register_exact(
        tx,
        tenant,
        workspace,
        unit,
        "pipeline_receipt",
        CopyRelation::PipelineReceipt,
        run,
        0,
        Some(operation),
        Some(request),
    )
    .await
}
