use super::*;

pub(super) async fn authorize_phase_replay(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    run: Uuid,
    prior: &PipelineMutationOutcome,
) -> Result<()> {
    if prior.context.run.id != run {
        return Err(Error::InternalInvariant);
    }
    let principal = session_principal(tx, session).await?;
    context::authorize_frozen_replay(tx, tenant, workspace, principal, run, &prior.context).await
}

pub(super) async fn backend_evidence_refs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase_id: &str,
    phase_ordinal: u32,
    consumed_knowledge: &Option<ConsumedKnowledgeManifestRef>,
) -> Result<(serde_json::Value, Option<PipelineKnowledgeBindingReceipt>)> {
    let outputs: Vec<(String, i64, Uuid, String)> = sqlx::query_as(
        "SELECT b.phase_id,b.output_revision,o.id,o.body_digest FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal < $4 AND b.stale=false AND NOT o.payload_erased ORDER BY b.phase_ordinal",
    )
    .bind(tenant).bind(workspace).bind(run).bind(phase_ordinal as i32)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let inputs: Vec<(Uuid, i64, String)> = sqlx::query_as(
        "SELECT id,sequence,input_digest FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4 AND NOT payload_erased ORDER BY sequence",
    )
    .bind(tenant).bind(workspace).bind(run).bind(phase_id)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut refs = Vec::with_capacity(outputs.len() + inputs.len() + 1);
    refs.extend(outputs.into_iter().map(|(phase, revision, id, digest)| {
        serde_json::json!({
            "kind":"output", "reference":id, "phase_id":phase, "revision":revision, "digest":digest
        })
    }));
    refs.extend(inputs.into_iter().map(|(id, sequence, digest)| {
        serde_json::json!({
            "kind":"input", "reference":id, "sequence":sequence, "digest":digest
        })
    }));
    let binding = if let Some(consumed) = consumed_knowledge {
        let row: Option<(Uuid, String, i64)> = sqlx::query_as(
            "SELECT id,digest,workspace_generation FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND digest=$4 AND run_id=$5 AND NOT payload_erased",
        )
        .bind(tenant).bind(workspace).bind(consumed.manifest_id).bind(&consumed.digest).bind(run)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let (manifest_id, digest, generation) = row.ok_or(Error::StaleContext)?;
        refs.push(serde_json::json!({"kind":"knowledge_manifest","reference":manifest_id,"revision":generation,"digest":digest}));
        Some(PipelineKnowledgeBindingReceipt {
            manifest_id,
            digest,
            workspace_generation: generation,
        })
    } else {
        None
    };
    Ok((serde_json::Value::Array(refs), binding))
}
