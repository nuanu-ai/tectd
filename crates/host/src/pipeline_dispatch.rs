use crate::pipeline_definitions::StaticPipelineDefinitions;
use crate::pipeline_tools::PipelineInvocation;
use serde_json::Value;
use tect_application::WorkspaceService;
use tect_domain::{RequestContext, Result};

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: PipelineInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let boundary = invocation.refusal_boundary();
    let result = match invocation {
        PipelineInvocation::Context(query) => service
            .pipeline_context(
                context,
                &query,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::context_pinned(value, capacity, &query)),
        PipelineInvocation::Instruction(query) => service
            .pipeline_instruction(context, &query)
            .await
            .and_then(|value| crate::pipeline_output::instruction_pinned(value, capacity, &query)),
        PipelineInvocation::Begin(request) => service
            .pipeline_run_begin(
                context,
                &request,
                &StaticPipelineDefinitions,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::begin(value, capacity)),
        PipelineInvocation::Migrate(request) => service
            .pipeline_run_migrate(context, &request, &StaticPipelineDefinitions)
            .await
            .and_then(|value| {
                let action = crate::responses::action(
                    "slice_pipeline_context",
                    serde_json::json!({"run_id":value.successor_run_id}),
                )?;
                let value = serde_json::to_value(value)
                    .map_err(tect_domain::Error::invalid_arguments_from)?;
                Ok(crate::responses::with_actions(value, vec![action], None))
            }),
        PipelineInvocation::Complete(request) => service
            .pipeline_phase_complete(
                context,
                &request,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::Input(request) => service
            .pipeline_input_record(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::EscalateDelivery(request) => service
            .pipeline_delivery_escalate(context, &request)
            .await
            .and_then(|value| crate::pipeline_output::mutation(value, capacity)),
        PipelineInvocation::ResolveCheckpoint(request) => service
            .pipeline_checkpoint_resolve(
                context,
                &request,
                &crate::pipeline_output::PipelineEncoding::new(capacity),
            )
            .await
            .and_then(|value| crate::pipeline_output::checkpoint_resolution(value, capacity)),
        PipelineInvocation::EvidenceRegister(request) => service
            .pipeline_evidence_artifact_register(context, &request)
            .await
            .and_then(evidence_response),
        PipelineInvocation::EvidenceFinalize(request) => service
            .pipeline_evidence_artifact_finalize(context, &request)
            .await
            .and_then(evidence_response),
        PipelineInvocation::EvidenceRead(request) => service
            .pipeline_evidence_artifact_read(context, &request)
            .await
            .and_then(evidence_response),
    };
    result.map_err(|error| {
        error.normalize_pipeline_refusal(
            boundary.rule,
            boundary.path,
            boundary.expected,
            boundary.next_action,
            boundary.required,
        )
    })
}

fn evidence_response<T: serde::Serialize>(value: T) -> Result<Value> {
    serde_json::to_value(value)
        .map(|value| crate::responses::with_actions(value, Vec::new(), None))
        .map_err(tect_domain::Error::invalid_arguments_from)
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        PipelineEvidenceArtifact, PipelineEvidenceArtifactOutcome, PipelineEvidenceArtifactPage,
        PipelineEvidenceArtifactReadiness,
    };

    #[test]
    fn evidence_route_successes_keep_data_and_canonical_action_envelope() {
        let mut artifact = PipelineEvidenceArtifact {
            artifact_id: uuid::Uuid::new_v4(),
            digest: "a".repeat(64),
            size: 2,
            format: "text/plain".into(),
            provenance: "fixture".into(),
            target: "fixture".into(),
            revision: 1,
            readiness: PipelineEvidenceArtifactReadiness::Uploading,
        };
        let registered = evidence_response(PipelineEvidenceArtifactOutcome {
            artifact: artifact.clone(),
            replay: false,
        })
        .unwrap();
        artifact.readiness = PipelineEvidenceArtifactReadiness::Ready;
        let finalized = evidence_response(PipelineEvidenceArtifactOutcome {
            artifact: artifact.clone(),
            replay: false,
        })
        .unwrap();
        let read = evidence_response(PipelineEvidenceArtifactPage {
            artifact,
            offset: 0,
            limit: 4,
            fragment: "é".into(),
            complete: true,
            next_offset: None,
        })
        .unwrap();
        assert_eq!(registered["artifact"]["readiness"], "uploading");
        assert_eq!(finalized["artifact"]["readiness"], "ready");
        assert_eq!(read["fragment"], "é");
        assert_eq!(read["complete"], true);
        assert!(read.get("next_offset").is_none());
        for payload in [registered, finalized, read] {
            let encoded = crate::responses::success(payload.clone());
            assert_eq!(encoded["isError"], false);
            assert_eq!(encoded["content"].as_array().unwrap().len(), 3);
            let wire: Value =
                serde_json::from_str(encoded["content"][1]["text"].as_str().unwrap()).unwrap();
            assert_eq!(wire, payload);
            assert_eq!(wire["actions"], serde_json::json!([]));
            assert_eq!(wire["recommended_action"], Value::Null);
        }
    }
}
