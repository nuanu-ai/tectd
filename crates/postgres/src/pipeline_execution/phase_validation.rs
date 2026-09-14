use super::*;

pub(super) async fn load_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    manifest: Option<Uuid>,
) -> Result<Option<PipelineKnowledgeManifest>> {
    let principal = session_principal(tx, session).await?;
    crate::durable_knowledge::manifest::load(tx, tenant, workspace, manifest, principal).await
}

pub(super) fn validate_output_integrity(output: &PipelinePhaseOutputDraft) -> Result<()> {
    if serde_json::to_vec(output).map_err(storage_error)?.len() > MAX_PIPELINE_OUTPUT_BYTES {
        return Err(Error::InvalidArguments);
    }
    for artifact in &output.artifacts {
        let actual = Sha256::digest(artifact.body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        if actual != artifact.digest
            || artifact.media_type == "application/json"
                && serde_json::from_str::<serde_json::Value>(&artifact.body).is_err()
        {
            return Err(Error::InvalidArguments);
        }
    }
    Ok(())
}
