use super::registry::{CopyRelation, register};
use super::*;

pub(super) async fn register_maintenance_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let signals: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT id,unit_revision FROM knowledge_maintenance_signals \
         WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for (signal, revision) in signals {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "maintenance_signal",
            CopyRelation::MaintenanceSignal,
            signal,
            revision,
        )
        .await?;
        let tasks: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM knowledge_maintenance_tasks WHERE tenant_id=$1 \
             AND workspace_id=$2 AND signal_id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(signal)
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?;
        for task in tasks {
            register(
                tx,
                tenant,
                workspace,
                unit,
                "maintenance_task",
                CopyRelation::MaintenanceTask,
                task,
                revision,
            )
            .await?;
        }
    }
    let receipts: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM knowledge_maintenance_command_receipts \
         WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for receipt in receipts {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "maintenance_receipt",
            CopyRelation::MaintenanceReceipt,
            receipt,
            0,
        )
        .await?;
    }
    let consumers: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT id,unit_revision FROM knowledge_maintenance_consumers \
         WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    for (consumer, revision) in consumers {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "maintenance_consumer",
            CopyRelation::MaintenanceConsumer,
            consumer,
            revision,
        )
        .await?;
    }
    Ok(())
}
