use serde_json::Value;
use tect_domain::{
    BeginPipelineRun, CompletePipelinePhase, Error, EscalatePipelineDelivery,
    PipelineRunContextQuery, RecordPipelineInput, ResolvePipelineCheckpoint, Result,
};

pub(crate) enum PipelineInvocation {
    Context(PipelineRunContextQuery),
    Begin(BeginPipelineRun),
    Complete(Box<CompletePipelinePhase>),
    Input(Box<RecordPipelineInput>),
    EscalateDelivery(EscalatePipelineDelivery),
    ResolveCheckpoint(ResolvePipelineCheckpoint),
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<PipelineInvocation> {
    reject_optional_nulls(&arguments)?;
    match name {
        "slice_pipeline_context" => decode(arguments).map(PipelineInvocation::Context),
        "slice_pipeline_begin" => decode(arguments).map(PipelineInvocation::Begin),
        "slice_pipeline_phase_complete" => decode(arguments)
            .map(Box::new)
            .map(PipelineInvocation::Complete),
        "slice_pipeline_input" => decode(arguments)
            .map(Box::new)
            .map(PipelineInvocation::Input),
        "slice_pipeline_delivery_escalate" => {
            decode(arguments).map(PipelineInvocation::EscalateDelivery)
        }
        "slice_pipeline_checkpoint_resolve" => {
            decode(arguments).map(PipelineInvocation::ResolveCheckpoint)
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn decode<T: for<'de> serde::Deserialize<'de>>(value: Value) -> Result<T> {
    serde_json::from_value(value).map_err(Error::invalid_arguments_from)
}

fn reject_optional_nulls(value: &Value) -> Result<()> {
    const OPTIONAL: &[&str] = &[
        "delivery_mode",
        "view",
        "output_id",
        "digest",
        "verdict",
        "reviewer_context",
        "followup_proposal",
        "reference",
        "revisit_phase_id",
        "escalation_target",
        "terminal_result",
        "consumed_knowledge",
        "inquiry",
        "source_checkpoint",
        "research_checkpoint",
        "source_amendment",
        "terminal",
    ];
    match value {
        Value::Object(object) => {
            if object
                .iter()
                .any(|(key, value)| OPTIONAL.contains(&key.as_str()) && value.is_null())
            {
                return Err(Error::InvalidArguments);
            }
            for value in object.values() {
                reject_optional_nulls(value)?;
            }
        }
        Value::Array(values) => {
            for value in values {
                reject_optional_nulls(value)?;
            }
        }
        _ => {}
    }
    Ok(())
}
