use crate::{
    PipelineDefinitionProvider, PipelineExecutionOutputGuard, TransactionMode, WorkspaceService,
};
use sha2::{Digest, Sha256};
use tect_domain::{
    BeginPipelineRun, BeginPipelineRunOutcome, CompletePipelinePhase, Error,
    EscalatePipelineDelivery, PipelineContextResponse, PipelineInstructionQuery,
    PipelineInstructionResponse, PipelineMutationOutcome, PipelineRunContextQuery,
    PipelineRunContextView, PipelineRunMigrationCommand, PipelineRunMigrationOutcome,
    RecordPipelineInput, ResolvePipelineCheckpoint, ResolvePipelineCheckpointOutcome, Result,
    SliceState,
};
use uuid::Uuid;

/// Proof that the application hashed the successor body for one exact request.
///
/// The fields are private and there is no public constructor, so host and adapter
/// callers can inspect this value but cannot mint one from request data.
///
/// ```compile_fail
/// use tect_application::VerifiedPipelineSourceDigest;
///
/// let _ = VerifiedPipelineSourceDigest {
///     request_id: Default::default(),
///     digest: String::new(),
/// };
/// ```
#[derive(Debug)]
pub struct VerifiedPipelineSourceDigest {
    request_id: Uuid,
    digest: String,
}

impl VerifiedPipelineSourceDigest {
    fn new(request_id: Uuid, declared_digest: &str, actual_digest: String) -> Result<Self> {
        if declared_digest != actual_digest {
            return Err(Error::InvalidArguments);
        }
        Ok(Self {
            request_id,
            digest: actual_digest,
        })
    }

    pub fn request_id(&self) -> Uuid {
        self.request_id
    }

    pub fn digest(&self) -> &str {
        &self.digest
    }
}

impl WorkspaceService {
    pub async fn pipeline_evidence_artifact_register(
        &self,
        context: &tect_domain::RequestContext,
        request: &tect_domain::RegisterPipelineEvidenceArtifact,
    ) -> Result<tect_domain::PipelineEvidenceArtifactOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .register_pipeline_evidence_artifact(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_evidence_artifact_finalize(
        &self,
        context: &tect_domain::RequestContext,
        request: &tect_domain::FinalizePipelineEvidenceArtifact,
    ) -> Result<tect_domain::PipelineEvidenceArtifactOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .finalize_pipeline_evidence_artifact(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_evidence_artifact_read(
        &self,
        context: &tect_domain::RequestContext,
        request: &tect_domain::ReadPipelineEvidenceArtifact,
    ) -> Result<tect_domain::PipelineEvidenceArtifactPage> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal = tx.session_principal(session.id).await?;
        let value = tx
            .read_pipeline_evidence_artifact(workspace.id, principal, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_run_begin(
        &self,
        context: &tect_domain::RequestContext,
        request: &BeginPipelineRun,
        definitions: &dyn PipelineDefinitionProvider,
        guard: &dyn PipelineExecutionOutputGuard,
    ) -> Result<BeginPipelineRunOutcome> {
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if let Some(replay) = tx
            .pipeline_begin_replay(workspace.id, principal_id, request)
            .await?
        {
            guard.check_begin(&replay)?;
            tx.commit().await?;
            return Ok(replay);
        }
        let slice = tx
            .native_slice(workspace.id, request.slice_id)
            .await?
            .ok_or(Error::NotFound)?;
        if slice.scope_id != request.scope_id || slice.state != SliceState::Open {
            return Err(Error::Forbidden);
        }
        let definition =
            definitions.definition_for(slice.pipeline, request.definition_version.as_deref())?;
        // The compact v0.7 snapshot has its own bounded contract checks and
        // intentionally does not satisfy every legacy v0.6 definition
        // invariant. Its digest, kind, instruction identities, body digests,
        // and phase bounds are verified by the static provider before this
        // point; keep the legacy validator for v0.6 and older snapshots.
        if !definition.version.starts_with("0.7") {
            definition.validate()?;
        }
        request.validate(&definition)?;
        slice.validate_verification_plan_binding()?;
        if slice.verification_plan_id.is_some()
            && (slice.verification_plan_source_definition_version.as_deref()
                != Some(definition.version.as_str())
                || slice.verification_plan_source_definition_digest.as_deref()
                    != Some(definition.digest.as_str()))
        {
            return Err(Error::StaleContext);
        }
        let value = tx
            .begin_pipeline_run(workspace.id, session.id, request, &definition)
            .await?;
        guard.check_begin(&value)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_run_migrate(
        &self,
        context: &tect_domain::RequestContext,
        request: &PipelineRunMigrationCommand,
        definitions: &dyn PipelineDefinitionProvider,
    ) -> Result<PipelineRunMigrationOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let predecessor = tx
            .pipeline_run_context_without_delivery_receipt(
                workspace.id,
                principal_id,
                request.predecessor_run_id,
            )
            .await?
            .ok_or(Error::NotFound)?;
        let definition = definitions.definition_for(
            predecessor.run.definition_kind,
            Some(request.successor_definition_version.as_str()),
        )?;
        if definition.version == predecessor.run.definition_version {
            return Err(Error::refused_at(
                tect_domain::RefusalCode::LegacyMigrationRequired,
                "WP6-MIGRATION-VERSION-01",
                "arguments.params.successor_definition_version",
                "a definition version different from the predecessor",
                definition.version.clone(),
                "select_successor_definition",
                "new_definition_version",
            ));
        }
        let value = tx
            .migrate_pipeline_run(workspace.id, session.id, request, &definition)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_context(
        &self,
        context: &tect_domain::RequestContext,
        query: &PipelineRunContextQuery,
    ) -> Result<PipelineContextResponse> {
        query.validate()?;
        let (mut tx, workspace, session) = self
            // Context reads allocate/reuse the backend-owned delivery receipt.
            // Keep this write scoped to the receipt; the run and manifest remain
            // immutable and are still loaded through the normal read path.
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let value = match query.view {
            PipelineRunContextView::Current => PipelineContextResponse::Current(Box::new(
                tx.pipeline_run_context(workspace.id, principal_id, query.run_id)
                    .await?
                    .ok_or(Error::NotFound)?,
            )),
            PipelineRunContextView::Output => PipelineContextResponse::Output(Box::new(
                tx.pipeline_phase_output(
                    workspace.id,
                    query.run_id,
                    query.output_id.ok_or(Error::InvalidArguments)?,
                    query.digest.as_deref().ok_or(Error::InvalidArguments)?,
                )
                .await?
                .ok_or(Error::NotFound)?,
            )),
            PipelineRunContextView::DeliveryReceipt => {
                let context = tx
                    .pipeline_run_context(workspace.id, principal_id, query.run_id)
                    .await?
                    .ok_or(Error::NotFound)?;
                PipelineContextResponse::DeliveryReceipt(Box::new(
                    context.delivery_receipt.ok_or(Error::InternalInvariant)?,
                ))
            }
        };
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_instruction(
        &self,
        context: &tect_domain::RequestContext,
        query: &PipelineInstructionQuery,
    ) -> Result<PipelineInstructionResponse> {
        query.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let stored = tx
            .pipeline_run_context_without_delivery_receipt(workspace.id, principal_id, query.run_id)
            .await?
            .ok_or(Error::NotFound)?;
        let value = query.resolve(&stored)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_phase_complete(
        &self,
        context: &tect_domain::RequestContext,
        request: &CompletePipelinePhase,
        guard: &dyn PipelineExecutionOutputGuard,
    ) -> Result<PipelineMutationOutcome> {
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let stored = tx
            .pipeline_run_context(workspace.id, principal_id, request.run_id)
            .await?
            .ok_or(Error::NotFound)?;
        request.validate(&stored.definition)?;
        let value = tx
            .complete_pipeline_phase(workspace.id, session.id, request)
            .await?;
        guard.check_mutation(&value)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_input_record(
        &self,
        context: &tect_domain::RequestContext,
        request: &RecordPipelineInput,
    ) -> Result<PipelineMutationOutcome> {
        request.validate()?;
        let verified_source_digest = request
            .source_amendment
            .as_ref()
            .map(|amendment| {
                let actual_digest = Sha256::digest(amendment.successor.artifact.body.as_bytes())
                    .iter()
                    .map(|byte| format!("{byte:02x}"))
                    .collect::<String>();
                VerifiedPipelineSourceDigest::new(
                    request.request_id,
                    &amendment.successor.artifact.digest,
                    actual_digest,
                )
            })
            .transpose()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .record_pipeline_input(
                workspace.id,
                session.id,
                request,
                verified_source_digest.as_ref(),
            )
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_delivery_escalate(
        &self,
        context: &tect_domain::RequestContext,
        request: &EscalatePipelineDelivery,
    ) -> Result<PipelineMutationOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .escalate_pipeline_delivery(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_checkpoint_resolve(
        &self,
        context: &tect_domain::RequestContext,
        request: &ResolvePipelineCheckpoint,
        guard: &dyn PipelineExecutionOutputGuard,
    ) -> Result<ResolvePipelineCheckpointOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .resolve_pipeline_checkpoint(workspace.id, session.id, request)
            .await?;
        guard.check_checkpoint_resolution(&value)?;
        tx.commit().await?;
        Ok(value)
    }
}

#[cfg(test)]
#[path = "pipeline_execution_tests.rs"]
mod digest_attestation_tests;
