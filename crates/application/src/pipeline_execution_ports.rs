use async_trait::async_trait;
use tect_domain::{
    BeginPipelineRun, BeginPipelineRunOutcome, CompletePipelinePhase, EscalatePipelineDelivery,
    FinalizePipelineEvidenceArtifact, PipelineDefinitionSnapshot, PipelineEvidenceArtifactOutcome,
    PipelineEvidenceArtifactPage, PipelineKind, PipelineMutationOutcome, PipelineRunContext,
    PipelineRunMigrationCommand, PipelineRunMigrationOutcome, ReadPipelineEvidenceArtifact,
    RecordPipelineInput, RegisterPipelineEvidenceArtifact, ResolvePipelineCheckpoint,
    ResolvePipelineCheckpointOutcome, Result,
};
use uuid::Uuid;

use crate::VerifiedPipelineSourceDigest;

pub trait PipelineDefinitionProvider: Send + Sync {
    fn definition(&self, kind: PipelineKind) -> Result<PipelineDefinitionSnapshot>;

    /// Resolve the immutable definition selected for a new run. Existing
    /// providers remain v0.6-compatible by accepting an omitted selector and
    /// by accepting an explicit selector only when it matches their default
    /// snapshot. Providers with additional immutable snapshots can override
    /// this method.
    fn definition_for(
        &self,
        kind: PipelineKind,
        requested_version: Option<&str>,
    ) -> Result<PipelineDefinitionSnapshot> {
        let definition = self.definition(kind)?;
        if requested_version.is_some_and(|version| version != definition.version) {
            return Err(tect_domain::Error::InvalidArguments);
        }
        Ok(definition)
    }
}

pub trait PipelineExecutionOutputGuard: Send + Sync {
    fn check_begin(&self, value: &BeginPipelineRunOutcome) -> Result<()>;
    fn check_mutation(&self, value: &PipelineMutationOutcome) -> Result<()>;
    fn check_checkpoint_resolution(&self, value: &ResolvePipelineCheckpointOutcome) -> Result<()>;
}

#[async_trait]
pub trait PipelineExecutionStore: Send {
    async fn register_pipeline_evidence_artifact(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RegisterPipelineEvidenceArtifact,
    ) -> Result<PipelineEvidenceArtifactOutcome>;
    async fn finalize_pipeline_evidence_artifact(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &FinalizePipelineEvidenceArtifact,
    ) -> Result<PipelineEvidenceArtifactOutcome>;
    async fn read_pipeline_evidence_artifact(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        request: &ReadPipelineEvidenceArtifact,
    ) -> Result<PipelineEvidenceArtifactPage>;
    async fn pipeline_begin_replay(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        request: &BeginPipelineRun,
    ) -> Result<Option<BeginPipelineRunOutcome>>;
    async fn pipeline_run_context(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<PipelineRunContext>>;
    async fn pipeline_run_context_without_delivery_receipt(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        run_id: Uuid,
    ) -> Result<Option<PipelineRunContext>>;
    async fn pipeline_phase_output(
        &mut self,
        workspace_id: Uuid,
        run_id: Uuid,
        output_id: Uuid,
        digest: &str,
    ) -> Result<Option<tect_domain::PipelinePhaseOutput>>;
    async fn begin_pipeline_run(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &BeginPipelineRun,
        definition: &PipelineDefinitionSnapshot,
    ) -> Result<BeginPipelineRunOutcome>;
    async fn migrate_pipeline_run(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &PipelineRunMigrationCommand,
        definition: &PipelineDefinitionSnapshot,
    ) -> Result<PipelineRunMigrationOutcome>;
    async fn complete_pipeline_phase(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &CompletePipelinePhase,
    ) -> Result<PipelineMutationOutcome>;
    async fn record_pipeline_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordPipelineInput,
        verified_source_digest: Option<&VerifiedPipelineSourceDigest>,
    ) -> Result<PipelineMutationOutcome>;
    async fn escalate_pipeline_delivery(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &EscalatePipelineDelivery,
    ) -> Result<PipelineMutationOutcome>;
    async fn resolve_pipeline_checkpoint(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &ResolvePipelineCheckpoint,
    ) -> Result<ResolvePipelineCheckpointOutcome>;
}
