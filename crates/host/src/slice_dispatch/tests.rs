use super::*;
use tect_domain::{NativeSlice, PipelineCheckpointRef, PipelineKind, SliceState};

fn slice(pipeline: PipelineKind, source_checkpoint: Option<PipelineCheckpointRef>) -> NativeSlice {
    NativeSlice {
        id: Uuid::new_v4(),
        scope_id: Uuid::new_v4(),
        revision: 1,
        candidate_id: Uuid::new_v4(),
        candidate_revision: 1,
        opening_snapshot_id: Uuid::new_v4(),
        title: "Bounded outcome".into(),
        outcome: "Observed result".into(),
        pipeline,
        selected_option_id: None,
        verification_plan_id: None,
        verification_plan_schema: None,
        verification_plan_digest: None,
        verification_plan_source_definition_version: None,
        verification_plan_source_definition_digest: None,
        state: SliceState::Open,
        pipeline_status: "not_started".into(),
        pipeline_run_id: None,
        knowledge_change_id: None,
        knowledge_run_id: None,
        knowledge_status: None,
        source_checkpoint,
        execution_claimed: false,
    }
}

#[test]
fn opened_slice_output_is_not_started_without_design_guidance_or_execution_claim() {
    let slice = slice(PipelineKind::LightweightTddDevelopment, None);
    let value = output(slice, Vec::new()).unwrap();
    assert_eq!(value["pipeline_status"], "not_started");
    assert_eq!(value["execution_claimed"], false);
    assert!(value.get("rules").is_none());
    assert!(value.get("method").is_none());
    assert!(value.get("execute").is_none());
}

#[test]
fn new_inquiry_pipelines_require_the_authored_immutable_inquiry() {
    for pipeline in [PipelineKind::Research, PipelineKind::DeepBrainstorming] {
        let value = pipeline_begin_action(&slice(pipeline, None)).unwrap();
        let fields = value["context_input"]["fields"].as_array().unwrap();
        assert_eq!(fields.len(), 2);
        assert_eq!(fields[1]["path"], "arguments.params.inquiry");
        assert!(
            fields[1]["format"]
                .as_str()
                .unwrap()
                .starts_with("Immutable inquiry:")
        );
        assert!(value["arguments"]["params"].get("inquiry").is_none());
        assert!(
            value["arguments"]["params"]
                .get("source_checkpoint")
                .is_none()
        );
    }
}

#[test]
fn checkpoint_backed_research_begin_server_fills_exact_checkpoint() {
    let checkpoint = PipelineCheckpointRef {
        checkpoint_id: Uuid::new_v4(),
        digest: "checkpoint-digest".into(),
    };
    let value =
        pipeline_begin_action(&slice(PipelineKind::Research, Some(checkpoint.clone()))).unwrap();
    assert_eq!(
        value["arguments"]["params"]["source_checkpoint"],
        json!(checkpoint)
    );
    assert_eq!(
        value["context_input"]["fields"][1]["format"],
        "Exact inquiry from that checkpoint in current Scope planning, not a new authored contract."
    );
    assert!(value["arguments"]["params"].get("inquiry").is_none());
}

#[test]
fn legacy_pipeline_begin_action_remains_qualification_only() {
    let value =
        pipeline_begin_action(&slice(PipelineKind::LightweightTddDevelopment, None)).unwrap();
    assert_eq!(
        value["context_input"]["fields"].as_array().unwrap().len(),
        1
    );
    assert!(value["arguments"]["params"].get("inquiry").is_none());
    assert!(
        value["arguments"]["params"]
            .get("source_checkpoint")
            .is_none()
    );
}
