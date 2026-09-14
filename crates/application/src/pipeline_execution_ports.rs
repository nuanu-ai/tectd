use async_trait::async_trait;
use tect_domain::{
    BeginPipelineRun, BeginPipelineRunOutcome, CompletePipelinePhase, EscalatePipelineDelivery,
    PipelineDefinitionSnapshot, PipelineKind, PipelineMutationOutcome, PipelineRunContext,
    RecordPipelineInput, Result,
};
use uuid::Uuid;

pub trait PipelineDefinitionProvider: Send + Sync {
    fn definition(&self, kind: PipelineKind) -> Result<PipelineDefinitionSnapshot>;
}

#[async_trait]
pub trait PipelineExecutionStore: Send {
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
    ) -> Result<PipelineMutationOutcome>;
    async fn escalate_pipeline_delivery(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &EscalatePipelineDelivery,
    ) -> Result<PipelineMutationOutcome>;
}
