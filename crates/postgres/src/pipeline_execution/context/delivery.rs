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

pub(crate) async fn load_output(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run_id: Uuid,
    output_id: Uuid,
    digest: &str,
) -> Result<Option<PipelinePhaseOutput>> {
    let erased:Option<bool>=sqlx::query_scalar("SELECT payload_erased FROM slice_pipeline_phase_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND id=$4")
        .bind(tenant).bind(workspace).bind(run_id).bind(output_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if erased == Some(true) {
        return Err(Error::KnowledgePayloadErased);
    }
    let row:Option<serde_json::Value>=sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object('id',o.id,'run_id',o.run_id,'phase_id',o.phase_id,'phase_ordinal',o.phase_ordinal,'revision',o.revision,'body',o.body,'producer_context_id',o.producer_context_id,'digest',o.body_digest,'reference',o.reference,'knowledge_publication',o.knowledge_publication,'fields',o.fields,'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads,'resource_reads',o.resource_reads,'artifacts',o.artifacts,'evidence_artifacts',o.evidence_artifacts,'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal,'stale',COALESCE(b.stale,true),'stale_reason',CASE WHEN b.output_id IS NULL THEN 'not_current_binding' ELSE b.stale_reason END) FROM slice_pipeline_phase_outputs o LEFT JOIN slice_pipeline_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.body_digest=$5")
        .bind(tenant).bind(workspace).bind(run_id).bind(output_id).bind(digest)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(decode).transpose()
}
