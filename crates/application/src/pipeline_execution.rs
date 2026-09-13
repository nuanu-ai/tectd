use crate::{PipelineDefinitionProvider, TransactionMode, WorkspaceService};
use tect_domain::{
    BeginPipelineRun, BeginPipelineRunOutcome, CompletePipelinePhase, Error,
    EscalatePipelineDelivery, PipelineContextResponse, PipelineMutationOutcome,
    PipelineRunContextQuery, PipelineRunContextView, RecordPipelineInput, Result, SliceState,
};

impl WorkspaceService {
    pub async fn pipeline_run_begin(
        &self,
        context: &tect_domain::RequestContext,
        request: &BeginPipelineRun,
        definitions: &dyn PipelineDefinitionProvider,
    ) -> Result<BeginPipelineRunOutcome> {
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        if let Some(replay) = tx.pipeline_begin_replay(workspace.id, request).await? {
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
        let definition = definitions.definition(slice.pipeline)?;
        definition.validate()?;
        request.validate(&definition)?;
        let value = tx
            .begin_pipeline_run(workspace.id, session.id, request, &definition)
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
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let value = match query.view {
            PipelineRunContextView::Current => PipelineContextResponse::Current(Box::new(
                tx.pipeline_run_context(workspace.id, query.run_id)
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
        };
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_phase_complete(
        &self,
        context: &tect_domain::RequestContext,
        request: &CompletePipelinePhase,
    ) -> Result<PipelineMutationOutcome> {
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let stored = tx
            .pipeline_run_context(workspace.id, request.run_id)
            .await?
            .ok_or(Error::NotFound)?;
        request.validate(&stored.definition)?;
        let value = tx
            .complete_pipeline_phase(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_input_record(
        &self,
        context: &tect_domain::RequestContext,
        request: &RecordPipelineInput,
    ) -> Result<PipelineMutationOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .record_pipeline_input(workspace.id, session.id, request)
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
}
