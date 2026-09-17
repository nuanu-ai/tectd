use super::*;

const FULL_SOURCE_TARGET: &str = "slice-component-decision-interrogator";
const CANONICAL_SOURCE_LEDGER: &str = "requirements-ledger.json";

fn amendment_violation(
    code: &str,
    path: &str,
    expected: Option<String>,
    actual: Option<String>,
) -> PipelineArtifactViolation {
    PipelineArtifactViolation {
        code: code.to_owned(),
        path: path.to_owned(),
        expected,
        actual,
    }
}

fn amendment_error(code: &str, violations: Vec<PipelineArtifactViolation>) -> Error {
    Error::InvalidPipelineArtifact(Box::new(PipelineArtifactDiagnostic::bounded(
        code.to_owned(),
        FULL_SOURCE_TARGET.to_owned(),
        "source_amendment".to_owned(),
        violations,
        true,
        "Reload the current run and phase-5 output, then submit a new request_id with exact current predecessor lineage, a changed hash-valid successor source, and the direct authority text and provenance.".to_owned(),
    )))
}

fn source_from_ledger(body: &str) -> Option<(String, String)> {
    let value: serde_json::Value = serde_json::from_str(body).ok()?;
    Some((
        value.get("source")?.get("path")?.as_str()?.to_owned(),
        value.get("source")?.get("digest")?.as_str()?.to_owned(),
    ))
}

pub(super) async fn validate_source_amendment_ledger(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let payload = sqlx::query_scalar::<_, serde_json::Value>(
        "SELECT request_payload FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4 AND request_payload ? 'source_amendment' AND NOT payload_erased ORDER BY sequence DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(FULL_SOURCE_TARGET)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(payload) = payload else {
        return Ok(());
    };
    let request: RecordPipelineInput = decode(payload)?;
    let amendment = request.source_amendment.ok_or(Error::InternalInvariant)?;
    let ledger = output
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "requirements-ledger.json")
        .and_then(|artifact| source_from_ledger(&artifact.body))
        .ok_or_else(|| {
            amendment_error(
                "source_amendment_ledger_invalid",
                vec![amendment_violation(
                    "source_lineage_missing",
                    "$.output.artifacts[requirements-ledger.json].body.source",
                    Some("successor path and digest".to_owned()),
                    None,
                )],
            )
        })?;
    let expected = (
        &amendment.successor.path,
        &amendment.successor.artifact.digest,
    );
    let mut violations = Vec::new();
    if &ledger.0 != expected.0 {
        violations.push(amendment_violation(
            "source_path_mismatch",
            "$.source.path",
            Some(expected.0.clone()),
            Some(ledger.0),
        ));
    }
    if &ledger.1 != expected.1 {
        violations.push(amendment_violation(
            "source_digest_mismatch",
            "$.source.digest",
            Some(expected.1.clone()),
            Some(ledger.1),
        ));
    }
    if violations.is_empty() {
        Ok(())
    } else {
        Err(amendment_error(
            "source_amendment_lineage_invalid",
            violations,
        ))
    }
}

pub(crate) async fn record_input(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RecordPipelineInput,
) -> Result<PipelineMutationOutcome> {
    let principal = session_principal(tx, session).await?;
    load_context(tx, tenant, workspace, principal, request.run_id)
        .await?
        .ok_or(Error::NotFound)?;
    let payload = json(request)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    let row:(i64,String,Option<String>,Option<i32>,String,serde_json::Value)=sqlx::query_as("SELECT revision,status,current_phase_id,current_phase_ordinal,definition_kind,definition FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    load_context(tx, tenant, workspace, principal, request.run_id)
        .await?
        .ok_or(Error::NotFound)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    if row.0 != request.run_revision {
        return Err(Error::StaleRevision);
    }
    checkpoint::ensure_run_source_open(tx, tenant, workspace, request.run_id).await?;
    if sqlx::query_scalar::<_, bool>(
        "SELECT EXISTS(SELECT 1 FROM pipeline_research_checkpoints WHERE tenant_id=$1 AND workspace_id=$2 AND producer_run_id=$3 AND status='open')",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(request.run_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?
    {
        return Err(Error::InputPending);
    }
    if row.2.as_deref() != Some(&request.phase_id)
        || matches!(row.1.as_str(), "completed" | "escalated")
    {
        return Err(Error::StaleContext);
    }
    let input_phase_id = if let Some(amendment) = &request.source_amendment {
        apply_source_amendment(
            tx, tenant, workspace, session, request, amendment, row.3, &row.4, &row.5,
        )
        .await?;
        FULL_SOURCE_TARGET
    } else {
        request.phase_id.as_str()
    };
    let sequence:i64=sqlx::query_scalar("SELECT COALESCE(MAX(sequence),0)+1 FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let id = Uuid::new_v4();
    let input_digest = digest(&request.input)?;
    sqlx::query("INSERT INTO slice_pipeline_inputs(id,tenant_id,workspace_id,run_id,sequence,phase_id,input,input_digest,actor_session_id,request_id,request_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11)")
        .bind(id).bind(tenant).bind(workspace).bind(request.run_id).bind(sequence).bind(input_phase_id).bind(&request.input).bind(input_digest).bind(session).bind(request.request_id).bind(&payload)
        .execute(&mut **tx).await.map_err(storage_error)?;
    if request.source_amendment.is_some() {
        sqlx::query("UPDATE slice_pipeline_runs SET revision=revision+1,status='active',current_phase_id=$4,current_phase_ordinal=5,knowledge_manifest_id=NULL,knowledge_manifest_digest=NULL WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(request.run_id).bind(FULL_SOURCE_TARGET).execute(&mut **tx).await.map_err(storage_error)?;
    } else {
        sqlx::query("UPDATE slice_pipeline_runs SET revision=revision+1,status='active' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(request.run_id).execute(&mut **tx).await.map_err(storage_error)?;
    }
    let outcome = PipelineMutationOutcome {
        context: load_context(tx, tenant, workspace, principal, request.run_id)
            .await?
            .ok_or(Error::InternalInvariant)?,
        result: None,
    };
    sqlx::query("UPDATE slice_pipeline_inputs SET result_payload=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(id).bind(json(&outcome)?).execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_pipeline_input_copies(
        tx,
        tenant,
        workspace,
        request.run_id,
        id,
        request.request_id,
    )
    .await?;
    Ok(outcome)
}

#[allow(clippy::too_many_arguments)]
async fn apply_source_amendment(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &RecordPipelineInput,
    amendment: &PipelineSourceAmendment,
    current_ordinal: Option<i32>,
    definition_kind: &str,
    definition_value: &serde_json::Value,
) -> Result<()> {
    let mut violations = Vec::new();
    if definition_kind != PipelineKind::FullDesignToExecution.as_str() {
        violations.push(amendment_violation(
            "pipeline_out_of_scope",
            "$.source_amendment",
            Some(PipelineKind::FullDesignToExecution.as_str().to_owned()),
            Some(definition_kind.to_owned()),
        ));
    }
    if amendment.target_phase_id != FULL_SOURCE_TARGET {
        violations.push(amendment_violation(
            "target_phase_invalid",
            "$.source_amendment.target_phase_id",
            Some(FULL_SOURCE_TARGET.to_owned()),
            Some(amendment.target_phase_id.clone()),
        ));
    }
    if amendment.predecessor.artifact_name != CANONICAL_SOURCE_LEDGER {
        violations.push(amendment_violation(
            "predecessor_artifact_invalid",
            "$.source_amendment.predecessor.artifact_name",
            Some(CANONICAL_SOURCE_LEDGER.to_owned()),
            Some(amendment.predecessor.artifact_name.clone()),
        ));
    }
    let definition: PipelineDefinitionSnapshot = decode(definition_value.clone())?;
    if current_ordinal.is_none_or(|ordinal| ordinal < 5)
        || !definition
            .phases
            .iter()
            .any(|phase| phase.id == FULL_SOURCE_TARGET && phase.ordinal == 5)
    {
        violations.push(amendment_violation(
            "current_phase_out_of_scope",
            "$.phase_id",
            Some("current Full Design phase at ordinal 5 or later".to_owned()),
            Some(request.phase_id.clone()),
        ));
    }
    if amendment.authorization_scope.trim().is_empty()
        || amendment.authorization_provenance.trim().is_empty()
    {
        violations.push(amendment_violation(
            "authority_required",
            "$.source_amendment.authorization_scope",
            Some("direct source-amendment authority scope and provenance".to_owned()),
            None,
        ));
    }
    if request.input.trim().is_empty() {
        violations.push(amendment_violation(
            "authority_text_required",
            "$.input",
            Some("exact direct operator instruction".to_owned()),
            None,
        ));
    }
    if amendment.successor.path != amendment.successor.artifact.name {
        violations.push(amendment_violation(
            "successor_identity_mismatch",
            "$.source_amendment.successor.artifact.name",
            Some(amendment.successor.path.clone()),
            Some(amendment.successor.artifact.name.clone()),
        ));
    }
    let successor_digest = Sha256::digest(amendment.successor.artifact.body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect::<String>();
    if successor_digest != amendment.successor.artifact.digest {
        violations.push(amendment_violation(
            "successor_digest_mismatch",
            "$.source_amendment.successor.artifact.digest",
            Some(successor_digest),
            Some(amendment.successor.artifact.digest.clone()),
        ));
    }
    if amendment.successor.artifact.digest == amendment.predecessor.source_digest {
        violations.push(amendment_violation(
            "source_unchanged",
            "$.source_amendment.successor.artifact.digest",
            Some("digest different from predecessor source".to_owned()),
            Some(amendment.successor.artifact.digest.clone()),
        ));
    }
    if !violations.is_empty() {
        return Err(amendment_error("source_amendment_invalid", violations));
    }

    let predecessor: Option<(Uuid,i64,String,serde_json::Value,bool)> = sqlx::query_as(
        "SELECT o.id,b.output_revision,o.body_digest,o.artifacts,b.stale FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4",
    )
    .bind(tenant).bind(workspace).bind(request.run_id).bind(FULL_SOURCE_TARGET)
    .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((output_id, output_revision, output_digest, artifacts, stale)) = predecessor else {
        return Err(amendment_error(
            "source_amendment_predecessor_missing",
            vec![amendment_violation(
                "predecessor_binding_missing",
                "$.source_amendment.predecessor",
                Some("current non-stale phase-5 binding".to_owned()),
                None,
            )],
        ));
    };
    let artifact = artifacts.as_array().and_then(|values| {
        values
            .iter()
            .find(|value| value["name"] == CANONICAL_SOURCE_LEDGER)
    });
    let artifact_digest = artifact.and_then(|value| value["digest"].as_str());
    let source = artifact
        .and_then(|value| value["body"].as_str())
        .and_then(source_from_ledger);
    let predecessor_matches = !stale
        && output_id == amendment.predecessor.output_id
        && output_revision == amendment.predecessor.output_revision
        && output_digest == amendment.predecessor.output_digest
        && artifact_digest == Some(amendment.predecessor.artifact_digest.as_str())
        && source.as_ref().is_some_and(|value| {
            value.0 == amendment.predecessor.source_path
                && value.1 == amendment.predecessor.source_digest
        });
    if !predecessor_matches {
        return Err(amendment_error(
            "source_amendment_predecessor_stale",
            vec![amendment_violation(
                "predecessor_not_current",
                "$.source_amendment.predecessor",
                Some(
                    "exact current non-stale phase-5 output, binding, artifact and source lineage"
                        .to_owned(),
                ),
                Some(format!(
                    "output_id={output_id},revision={output_revision},stale={stale}"
                )),
            )],
        ));
    }
    super::phase::helpers::stale_from_ordinal(
        tx,
        tenant,
        workspace,
        session,
        request.run_id,
        5,
        "source_amendment",
    )
    .await
}

pub(crate) async fn escalate_delivery(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &EscalatePipelineDelivery,
) -> Result<PipelineMutationOutcome> {
    let payload = json(request)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND operation='delivery_escalate' AND request_id=$4")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    let row:(i64,String,String,Option<String>)=sqlx::query_as("SELECT revision,status,delivery_mode,current_phase_id FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NotFound)?;
    if let Some((stored,result,erased))=sqlx::query_as::<_,(Option<serde_json::Value>,Option<serde_json::Value>,bool)>(
        "SELECT request_payload,result_payload,payload_erased FROM slice_pipeline_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND operation='delivery_escalate' AND request_id=$4")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(request.request_id).fetch_optional(&mut **tx).await.map_err(storage_error)? {
        if erased { return Err(Error::KnowledgePayloadErased) }
        if stored != Some(payload.clone()) { return Err(Error::InputConflict) }
        return decode(result.ok_or(Error::InternalInvariant)?);
    }
    if row.0 != request.run_revision {
        return Err(Error::StaleRevision);
    }
    if row.1 == "completed"
        || row.1 == "escalated"
        || row.2 != "whole"
        || row.3.as_deref() != Some(&request.phase_id)
    {
        return Err(Error::Forbidden);
    }
    sqlx::query("UPDATE slice_pipeline_runs SET revision=revision+1,delivery_mode='phasewise' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).execute(&mut **tx).await.map_err(storage_error)?;
    let principal = session_principal(tx, session).await?;
    let outcome = PipelineMutationOutcome {
        context: load_context(tx, tenant, workspace, principal, request.run_id)
            .await?
            .ok_or(Error::InternalInvariant)?,
        result: None,
    };
    sqlx::query("INSERT INTO slice_pipeline_receipts(tenant_id,workspace_id,run_id,operation,request_id,actor_session_id,request_payload,result_payload) VALUES($1,$2,$3,'delivery_escalate',$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(request.request_id).bind(session).bind(payload).bind(json(&outcome)?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    crate::knowledge_lifecycle::erase::register_pipeline_receipt_copies(
        tx,
        tenant,
        workspace,
        request.run_id,
        request.request_id,
    )
    .await?;
    Ok(outcome)
}
