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
mod digest_attestation_tests {
    use super::*;
    use async_trait::async_trait;
    use std::sync::{
        Arc,
        atomic::{AtomicUsize, Ordering},
    };
    use tect_domain::{
        FileObservation, FilePublication, HostAuth, PipelineSourceAmendment,
        PipelineSourceArtifactDraft, PipelineSourcePredecessor, PipelineSourceSuccessor,
        RequestContext, SetupDirectory, SourceLocation,
    };

    struct CountingStore(AtomicUsize);

    #[async_trait]
    impl crate::Store for CountingStore {
        async fn begin(&self, _mode: TransactionMode) -> Result<Box<dyn crate::UnitOfWork>> {
            self.0.fetch_add(1, Ordering::SeqCst);
            Err(Error::InternalInvariant)
        }
    }

    struct UnusedAdapters;

    #[async_trait]
    impl crate::SourceInspector for UnusedAdapters {
        async fn inspect(&self, _path: &str, _allowed_roots: &[String]) -> Result<SourceLocation> {
            Err(Error::InternalInvariant)
        }
    }

    impl crate::SetupFiles for UnusedAdapters {
        fn resolve_directory(
            &self,
            _path: &str,
            _current_roots: &[String],
        ) -> Result<SetupDirectory> {
            Err(Error::InternalInvariant)
        }

        fn inspect(
            &self,
            _directory: &SetupDirectory,
            _max_bytes: usize,
        ) -> Result<FileObservation> {
            Err(Error::InternalInvariant)
        }

        fn publish(&self, _directory: &SetupDirectory, _content: &str) -> Result<FilePublication> {
            Err(Error::InternalInvariant)
        }
    }

    fn amendment_request(body: &str, declared_digest: &str) -> RecordPipelineInput {
        RecordPipelineInput {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 2,
            phase_id: "slice-implementation-spec-synthesizer".to_owned(),
            input: "Direct source amendment authority.".to_owned(),
            source_amendment: Some(PipelineSourceAmendment {
                target_phase_id: "slice-component-decision-interrogator".to_owned(),
                predecessor: PipelineSourcePredecessor {
                    output_id: Uuid::new_v4(),
                    output_revision: 1,
                    output_digest: "a".repeat(64),
                    artifact_name: "requirements-ledger.json".to_owned(),
                    artifact_digest: "b".repeat(64),
                    source_path: "source.md".to_owned(),
                    source_digest: "c".repeat(64),
                },
                successor: PipelineSourceSuccessor {
                    path: "source.md".to_owned(),
                    artifact: PipelineSourceArtifactDraft {
                        name: "source.md".to_owned(),
                        media_type: "text/markdown".to_owned(),
                        body: body.to_owned(),
                        digest: declared_digest.to_owned(),
                        reference: None,
                    },
                },
                authorization_scope: "Amend the current Full Design source.".to_owned(),
                authorization_provenance: "Exact direct operator input.".to_owned(),
            }),
        }
    }

    #[test]
    fn forged_declared_digest_cannot_mint_an_attestation() {
        let request = amendment_request("arbitrary body", &"0".repeat(64));
        request.validate().expect("shape is valid");
        let amendment = request.source_amendment.as_ref().unwrap();
        let actual_digest = Sha256::digest(amendment.successor.artifact.body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();

        assert!(matches!(
            VerifiedPipelineSourceDigest::new(
                request.request_id,
                &amendment.successor.artifact.digest,
                actual_digest,
            ),
            Err(Error::InvalidArguments)
        ));
    }

    #[tokio::test]
    async fn public_service_rejects_forged_digest_before_entering_store() {
        let store = Arc::new(CountingStore(AtomicUsize::new(0)));
        let service = WorkspaceService::new(
            store.clone(),
            Arc::new(UnusedAdapters),
            Arc::new(UnusedAdapters),
        );
        let context = RequestContext {
            auth: HostAuth {
                host_id: Uuid::new_v4(),
                credential: "a".repeat(64),
            },
            native_session_id: "native-session".to_owned(),
            workspace_key: "workspace".to_owned(),
        };
        let request = amendment_request("arbitrary body", &"0".repeat(64));

        assert!(matches!(
            service.pipeline_input_record(&context, &request).await,
            Err(Error::InvalidArguments)
        ));
        assert_eq!(store.0.load(Ordering::SeqCst), 0);
    }

    #[test]
    fn attestation_is_bound_to_request_and_actual_digest() {
        let body = "amended body";
        let digest = Sha256::digest(body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        let request = amendment_request(body, &digest);
        let verified =
            VerifiedPipelineSourceDigest::new(request.request_id, &digest, digest.clone())
                .expect("application-computed digest matches declaration");

        assert_eq!(verified.request_id(), request.request_id);
        assert_eq!(verified.digest(), digest);
    }
}
