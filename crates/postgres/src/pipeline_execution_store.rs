use crate::{pipeline_execution, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::{PipelineExecutionStore, VerifiedPipelineSourceDigest};
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
impl PipelineExecutionStore for PgUnitOfWork {
    async fn register_pipeline_evidence_artifact(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RegisterPipelineEvidenceArtifact,
    ) -> Result<PipelineEvidenceArtifactOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::evidence_artifact::register(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }
    async fn finalize_pipeline_evidence_artifact(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &FinalizePipelineEvidenceArtifact,
    ) -> Result<PipelineEvidenceArtifactOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::evidence_artifact::finalize(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }
    async fn read_pipeline_evidence_artifact(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        request: &ReadPipelineEvidenceArtifact,
    ) -> Result<PipelineEvidenceArtifactPage> {
        let tenant = self.tenant_id()?;
        pipeline_execution::evidence_artifact::read(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            request,
        )
        .await
    }
    async fn pipeline_begin_replay(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        request: &BeginPipelineRun,
    ) -> Result<Option<BeginPipelineRunOutcome>> {
        let tenant = self.tenant_id()?;
        pipeline_execution::begin_replay(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            request,
        )
        .await
    }

    async fn pipeline_run_context(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<PipelineRunContext>> {
        let tenant = self.tenant_id()?;
        pipeline_execution::load_context(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            run_id,
        )
        .await
    }

    async fn pipeline_run_context_without_delivery_receipt(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<PipelineRunContext>> {
        let tenant = self.tenant_id()?;
        pipeline_execution::load_context_without_delivery_receipt(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            run_id,
        )
        .await
    }

    async fn pipeline_phase_output(
        &mut self,
        workspace_id: Uuid,
        run_id: Uuid,
        output_id: Uuid,
        digest: &str,
    ) -> Result<Option<PipelinePhaseOutput>> {
        let tenant = self.tenant_id()?;
        pipeline_execution::load_output(
            self.transaction()?,
            tenant,
            workspace_id,
            run_id,
            output_id,
            digest,
        )
        .await
    }

    async fn begin_pipeline_run(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &BeginPipelineRun,
        definition: &PipelineDefinitionSnapshot,
    ) -> Result<BeginPipelineRunOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::begin(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
            definition,
        )
        .await
    }

    async fn migrate_pipeline_run(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &PipelineRunMigrationCommand,
        definition: &PipelineDefinitionSnapshot,
    ) -> Result<PipelineRunMigrationOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::migrate_run(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
            definition,
        )
        .await
    }

    async fn complete_pipeline_phase(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &CompletePipelinePhase,
    ) -> Result<PipelineMutationOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::complete_phase(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }

    async fn record_pipeline_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordPipelineInput,
        verified_source_digest: Option<&VerifiedPipelineSourceDigest>,
    ) -> Result<PipelineMutationOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::record_input(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
            verified_source_digest,
        )
        .await
    }

    async fn escalate_pipeline_delivery(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &EscalatePipelineDelivery,
    ) -> Result<PipelineMutationOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::escalate_delivery(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }

    async fn resolve_pipeline_checkpoint(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &ResolvePipelineCheckpoint,
    ) -> Result<ResolvePipelineCheckpointOutcome> {
        let tenant = self.tenant_id()?;
        pipeline_execution::resolve_checkpoint(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }
}
