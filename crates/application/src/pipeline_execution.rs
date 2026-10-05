mod receipt_digest;
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
        if let Some(replay) = tx.pipeline_migration_replay(workspace.id, request).await? {
            tx.commit().await?;
            return Ok(replay);
        }
        let definition = definitions
            .definition_for(
                predecessor.run.definition_kind,
                Some(request.successor_definition_version.as_str()),
            )
            .map_err(tect_domain::migration_successor_retirement_error)?;
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
        guard: &dyn PipelineExecutionOutputGuard,
    ) -> Result<PipelineContextResponse> {
        query.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(
                context,
                if query.view == PipelineRunContextView::Current {
                    TransactionMode::ReadWrite
                } else {
                    TransactionMode::ReadOnly
                },
            )
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let value = crate::request_diagnostics::measure("application.context_assembly", async {
            Ok::<_, Error>(match query.view {
                PipelineRunContextView::Output => PipelineContextResponse::Output(Box::new(
                    tx.pipeline_phase_output(
                        workspace.id,
                        principal_id,
                        query.run_id,
                        query.output_id.ok_or(Error::InvalidArguments)?,
                        query.digest.as_deref().ok_or(Error::InvalidArguments)?,
                    )
                    .await?
                    .ok_or(Error::NotFound)?,
                )),
                PipelineRunContextView::Current => {
                    let stored = tx
                        .pipeline_run_ordinary_context(
                            workspace.id,
                            principal_id,
                            session.id,
                            query.run_id,
                        )
                        .await?
                        .ok_or(Error::NotFound)?;
                    // The same compact encoder used for the reply must succeed before
                    // committing any newly allocated snapshot-reference receipt.
                    guard.check_context(&stored)?;
                    PipelineContextResponse::Current(Box::new(stored))
                }
                PipelineRunContextView::DeliveryReceipt => {
                    let receipt = tx
                        .pipeline_existing_delivery_receipt(
                            workspace.id,
                            principal_id,
                            query.run_id,
                        )
                        .await?
                        .ok_or_else(|| {
                            Error::refused_at(
                                tect_domain::RefusalCode::DeliveryRefreshRequired,
                                "PIPELINE-SNAPSHOT-REFERENCE-MISSING",
                                "arguments.params.view",
                                "existing current-epoch snapshot-reference receipt",
                                "receipt unavailable",
                                "refresh_pipeline_context",
                                "current_pipeline_context",
                            )
                        })?;
                    PipelineContextResponse::DeliveryReceipt(Box::new(receipt))
                }
                PipelineRunContextView::ReceiptDiff => {
                    let stored = tx
                        .pipeline_run_context_without_delivery_receipt(
                            workspace.id,
                            principal_id,
                            query.run_id,
                        )
                        .await?
                        .ok_or(Error::NotFound)?;
                    PipelineContextResponse::ReceiptDiff(Box::new(
                        query.resolve_receipt_diff(&stored, &receipt_digest::ReceiptDigest)?,
                    ))
                }
                PipelineRunContextView::Snapshot
                | PipelineRunContextView::PhaseContract
                | PipelineRunContextView::Details => {
                    let stored = tx
                        .pipeline_run_context_without_delivery_receipt(
                            workspace.id,
                            principal_id,
                            query.run_id,
                        )
                        .await?
                        .ok_or(Error::NotFound)?;
                    query.resolve_pinned_read(&stored)?
                }
            })
        })
        .await?;
        crate::request_diagnostics::measure("application.commit", tx.commit()).await?;
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
        use crate::request_diagnostics::measure;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let stored = measure(
            "application.completion_preflight",
            tx.pipeline_run_completion_context(workspace.id, principal_id, request.run_id),
        )
        .await?
        .ok_or(Error::NotFound)?;
        measure("application.completion_validation", async {
            request.validate(&stored.definition)
        })
        .await?;
        let value = measure(
            "application.phase_mutation",
            tx.complete_pipeline_phase(workspace.id, session.id, request),
        )
        .await?;
        measure("application.output_guard", async {
            guard.check_mutation(&value)
        })
        .await?;
        measure("application.commit", tx.commit()).await?;
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
mod digest_attestation_tests;
