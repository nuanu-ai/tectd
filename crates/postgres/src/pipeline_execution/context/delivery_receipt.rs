use super::*;

/// Allocate exactly one backend-owned receipt for each immutable run revision.
/// The receipt binds the delivery to the definition snapshot persisted on the
/// run; no agent-provided digest or consumed list participates in this proof.
pub(super) async fn load_or_create_delivery_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    context_epoch: i64,
    manifest_digest: &str,
) -> Result<(PipelineDeliveryReceipt, bool)> {
    let inserted = sqlx::query("INSERT INTO pipeline_delivery_receipts(tenant_id,workspace_id,delivery_id,run_id,context_epoch,manifest_digest) VALUES($1,$2,$3,$4,$5,$6) ON CONFLICT (tenant_id,workspace_id,run_id,context_epoch) DO NOTHING")
        .bind(tenant)
        .bind(workspace)
        .bind(Uuid::new_v4())
        .bind(run)
        .bind(context_epoch)
        .bind(manifest_digest)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    let row: Option<(Uuid, i64, String, String)> = sqlx::query_as(
        "SELECT delivery_id,context_epoch,manifest_digest,delivered_at::text FROM pipeline_delivery_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND context_epoch=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(context_epoch)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some((delivery_id, epoch, digest, delivered_at)) = row else {
        return Err(Error::InternalInvariant);
    };
    if digest != manifest_digest {
        return Err(Error::InternalInvariant);
    }
    Ok((
        PipelineDeliveryReceipt {
            delivery_id,
            run_id: run,
            context_epoch: epoch,
            manifest_digest: digest,
            delivered_at,
        },
        inserted.rows_affected() == 1,
    ))
}
