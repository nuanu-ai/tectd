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

pub(super) fn validate_output_integrity(
    output: &PipelinePhaseOutputDraft,
    definition_version: &str,
) -> Result<()> {
    if serde_json::to_vec(output).map_err(storage_error)?.len() > MAX_PIPELINE_OUTPUT_BYTES {
        if definition_version.starts_with("0.6") {
            return Err(Error::InvalidArguments);
        }
        return Err(Error::refused_at(
            RefusalCode::PayloadTooLarge,
            "WP6-OUTPUT-SIZE-01",
            "arguments.params.output",
            format!("at most {MAX_PIPELINE_OUTPUT_BYTES} encoded bytes"),
            "encoded output exceeds limit",
            "reduce_output",
            "pipeline_output",
        ));
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
            return Err(Error::refused_at(
                RefusalCode::InvalidOutput,
                "WP6-ARTIFACT-INTEGRITY-01",
                "arguments.params.output.artifacts",
                "digest matching exact body and valid declared media type",
                "digest mismatch or invalid media body",
                "correct_artifact",
                "artifact_digest_and_media_type",
            ));
        }
    }
    if output
        .evidence_artifacts
        .iter()
        .any(|reference| reference.artifact_id.is_nil() || reference.revision < 1)
    {
        return Err(Error::refused_at(
            RefusalCode::InvalidOutput,
            "WP6-EVIDENCE-REFERENCE-01",
            "arguments.params.output.evidence_artifacts",
            "non-nil artifact id and positive revision",
            "nil artifact id or non-positive revision",
            "correct_evidence_reference",
            "evidence_artifact_reference",
        ));
    }
    Ok(())
}

pub(super) async fn validate_ready_evidence_artifacts(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    refs: &[PipelineEvidenceArtifactRef],
) -> Result<()> {
    for reference in refs {
        let ready: Option<bool> = sqlx::query_scalar("SELECT readiness='ready' FROM pipeline_evidence_artifacts WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4")
            .bind(tenant).bind(workspace).bind(reference.artifact_id).bind(reference.revision)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?;
        if ready != Some(true) {
            return Err(Error::refused_at(
                RefusalCode::ArtifactNotReady,
                "WP6-EVIDENCE-READINESS-01",
                "arguments.params.output.evidence_artifacts",
                "an existing artifact revision with readiness ready",
                ready.map_or(
                    "artifact revision not found",
                    |_| "artifact revision not ready",
                ),
                "finalize_evidence_artifact",
                "ready_evidence_artifact",
            ));
        }
    }
    Ok(())
}
