//! Native declaration/provenance checks, before any phase attempt is persisted.
use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn validate(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    scope: Uuid,
    slice: Uuid,
    slice_revision: i64,
) -> Result<()> {
    if !native_work_contract_definition(definition) {
        return Ok(());
    }
    let authoring = request.phase_id == NATIVE_WORK_CONTRACT_PHASE;
    let consumer = matches!(
        request.phase_id.as_str(),
        "slice-plan-builder" | "slice-engineering-plan-review" | "slice-execution-runner"
    );
    if !authoring && !consumer {
        return Ok(());
    }
    let phase = definition
        .phases
        .iter()
        .find(|p| p.id == NATIVE_WORK_CONTRACT_PHASE)
        .ok_or(Error::InternalInvariant)?;
    if !native_work_contract_phase(definition, phase) {
        return Err(native_work_contract_refusal(
            "native schema requirement missing from pinned definition",
        ));
    }
    let (contract, author_session, author_revision) = if authoring {
        let artifact = request
            .output
            .artifacts
            .iter()
            .find(|a| a.name == NATIVE_WORK_CONTRACT_ARTIFACT);
        if request.output.verdict.as_deref() != Some("contract_ready") {
            return if artifact.is_some() {
                Err(native_work_contract_refusal(
                    "canonical contract artifact is only allowed for contract_ready",
                ))
            } else {
                Ok(())
            };
        }
        (
            NativeSliceWorkContract::parse(
                &artifact
                    .ok_or_else(|| native_work_contract_refusal("contract artifact missing"))?
                    .body,
            )?,
            session,
            request.run_revision,
        )
    } else {
        let row: Option<(i64,String,serde_json::Value,Uuid,serde_json::Value)>=sqlx::query_as(
            "SELECT o.revision,o.body_digest,o.artifacts,a.actor_session_id,a.request_payload FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id JOIN slice_pipeline_phase_attempts a ON a.tenant_id=o.tenant_id AND a.workspace_id=o.workspace_id AND a.id=o.attempt_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND NOT b.stale AND NOT o.payload_erased AND NOT a.payload_erased AND o.verdict='contract_ready'")
            .bind(tenant).bind(workspace).bind(request.run_id).bind(NATIVE_WORK_CONTRACT_PHASE).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let (revision, digest, artifacts, actor, payload) = row.ok_or_else(|| {
            native_work_contract_refusal("current nonstale Phase4 output required")
        })?;
        if !request.consumed_outputs.iter().any(|r| {
            r.phase_id == NATIVE_WORK_CONTRACT_PHASE
                && r.output_revision == revision
                && r.digest == digest
        }) {
            return Err(native_work_contract_refusal(
                "current Phase4 output receipt not consumed",
            ));
        }
        let artifacts: Vec<PipelinePhaseArtifactDraft> = decode(artifacts)?;
        let artifact = artifacts
            .iter()
            .find(|a| a.name == NATIVE_WORK_CONTRACT_ARTIFACT)
            .ok_or_else(|| native_work_contract_refusal("Phase4 contract artifact missing"))?;
        let revision = payload
            .get("run_revision")
            .and_then(serde_json::Value::as_i64)
            .ok_or(Error::InternalInvariant)?;
        (
            NativeSliceWorkContract::parse(&artifact.body)?,
            actor,
            revision,
        )
    };
    if authoring {
        for reference in contract
            .required_reads
            .iter()
            .map(|r| &r.reference)
            .chain(contract.authority.source.iter())
        {
            if let NativeWorkReference::NativeOutput { phase_id, .. } = reference
                && definition
                    .phases
                    .iter()
                    .find(|p| &p.id == phase_id)
                    .is_none_or(|p| p.ordinal >= 4)
            {
                return Err(native_work_contract_refusal(
                    "authoring read cannot depend on current or later phase outputs",
                ));
            }
        }
    }
    let t = &contract.target;
    if t.scope_id != scope
        || t.slice_id != slice
        || t.slice_revision != slice_revision
        || t.run_id != request.run_id
        || t.run_revision != author_revision
        || t.definition_version != definition.version
        || t.definition_digest != definition.digest
    {
        return Err(native_work_contract_refusal(
            "locked target or authoring pins mismatch",
        ));
    }
    let declared = contract
        .session_declaration
        .as_ref()
        .ok_or_else(|| native_work_contract_refusal("session declaration missing"))?;
    let actual: Option<(Uuid,Uuid,String,bool)>=sqlx::query_as("SELECT workspace_id,host_id,native_session_id,revoked FROM agent_sessions WHERE tenant_id=$1 AND id=$2")
        .bind(tenant).bind(author_session).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let actual = actual.ok_or_else(|| native_work_contract_refusal("author session missing"))?;
    if declared.workspace_id != workspace
        || declared.session_id != author_session
        || declared.workspace_id != actual.0
        || declared.host_id != actual.1
        || declared.native_session_id != actual.2
        || actual.3
    {
        return Err(native_work_contract_refusal(
            "authenticated author session mismatch or revoked",
        ));
    }
    let current_host:Uuid=sqlx::query_scalar("SELECT host_id FROM agent_sessions WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND NOT revoked")
        .bind(tenant).bind(workspace).bind(session).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::Forbidden)?;
    let checkpoint:(Option<Uuid>,Option<String>)=sqlx::query_as("SELECT source_checkpoint_id,source_checkpoint_digest FROM slice_pipeline_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let declared_checkpoint = contract
        .source_checkpoint
        .as_ref()
        .map(|r| (r.checkpoint_id, r.digest.as_str()));
    if declared_checkpoint != checkpoint.0.zip(checkpoint.1.as_deref()) {
        return Err(native_work_contract_refusal(
            "run source checkpoint mismatch",
        ));
    }
    for read in &contract.required_reads {
        validate_reference(
            tx,
            tenant,
            workspace,
            current_host,
            request.run_id,
            &read.reference,
        )
        .await?;
    }
    if let Some(reference) = &contract.authority.source {
        validate_reference(
            tx,
            tenant,
            workspace,
            current_host,
            request.run_id,
            reference,
        )
        .await?;
    }
    for p in contract
        .write_scope
        .allowed_roots
        .iter()
        .chain(&contract.write_scope.allowed_paths)
        .chain(&contract.write_scope.denied_roots)
        .chain(contract.authority.scope.iter().flat_map(|s| &s.targets))
    {
        validate_source(tx, tenant, workspace, current_host, p.source_id).await?;
    }
    Ok(())
}
async fn validate_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    host: Uuid,
    source: Uuid,
) -> Result<()> {
    let exists:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM source_worktrees WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND id=$4)")
        .bind(tenant).bind(workspace).bind(host).bind(source).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(native_work_contract_refusal(
            "source not registered for current workspace/host",
        ))
    }
}
async fn validate_reference(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    host: Uuid,
    run: Uuid,
    r: &NativeWorkReference,
) -> Result<()> {
    let valid:bool=match r {
        NativeWorkReference::NativeOutput{phase_id,output_revision,digest}=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND b.output_revision=$5 AND o.body_digest=$6 AND NOT b.stale AND NOT o.payload_erased)")
            .bind(tenant).bind(workspace).bind(run).bind(phase_id).bind(output_revision).bind(digest).fetch_one(&mut **tx).await.map_err(storage_error)?,
        NativeWorkReference::NativeInput{input_id,sequence,digest}=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND id=$4 AND sequence=$5 AND input_digest=$6 AND NOT payload_erased)")
            .bind(tenant).bind(workspace).bind(run).bind(input_id).bind(sequence).bind(digest).fetch_one(&mut **tx).await.map_err(storage_error)?,
        NativeWorkReference::NativeEvidenceArtifact{artifact_id,revision,digest}=>sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pipeline_evidence_artifacts WHERE tenant_id=$1 AND workspace_id=$2 AND artifact_id=$3 AND revision=$4 AND digest=$5 AND readiness='ready')")
            .bind(tenant).bind(workspace).bind(artifact_id).bind(revision).bind(digest).fetch_one(&mut **tx).await.map_err(storage_error)?,
        NativeWorkReference::SourceFile{source_id,..}=>{validate_source(tx,tenant,workspace,host,*source_id).await?;true},
    };
    if valid {
        Ok(())
    } else {
        Err(native_work_contract_refusal(
            "required native reference stale, missing or mismatched",
        ))
    }
}
