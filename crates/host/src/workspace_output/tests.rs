use super::*;
use tect_domain::{
    NativePipelineRunSummary, NativeSliceSummary, NativeWorkCandidateSummary, SliceState,
};

pub(super) fn summary() -> NativePlanningSummary {
    NativePlanningSummary {
        scope_id: Uuid::new_v4(),
        scope_revision: 1,
        candidate_set_id: Uuid::new_v4(),
        candidate_set_revision: 3,
        candidate_set_status: SliceCandidateSetStatus::Ready,
        snapshot_id: Uuid::new_v4(),
        stale: false,
        eligible_work: vec![NativeWorkCandidateSummary {
            candidate_id: Uuid::new_v4(),
            candidate_revision: 1,
        }],
        slices_needing_result: Vec::new(),
        pipeline_runs: Vec::new(),
        knowledge_changes: Vec::new(),
    }
}

#[test]
fn completed_cycle_offers_new_original_input_before_historical_pipeline() {
    let mut summary = summary();
    summary.eligible_work.clear();
    summary.pipeline_runs.push(NativePipelineRunSummary {
        run_id: Uuid::new_v4(),
        slice_id: Uuid::new_v4(),
        status: "completed".into(),
    });
    let calls = native_actions(&summary).unwrap();
    assert_eq!(calls[0]["kind"], "needs_input");
    assert_eq!(calls[0]["arguments"]["route"], "slice.candidates.input");
    assert_eq!(
        calls[0]["arguments"]["params"]["revision"],
        summary.candidate_set_revision
    );
    assert_eq!(
        calls.last().unwrap()["arguments"]["route"],
        "slice.pipeline.context"
    );
}

#[test]
fn native_state_routes_stale_ready_and_open_slice_without_claiming_execution() {
    let ready = native_actions(&summary()).unwrap();
    assert_eq!(ready[0]["arguments"]["route"], "slice.open");
    assert!(ready.iter().all(|action| {
        !matches!(
            action["arguments"]["route"].as_str(),
            Some("slice.start" | "slice.execute")
        )
    }));

    let mut stale = summary();
    stale.stale = true;
    let stale_actions = native_actions(&stale).unwrap();
    assert_eq!(
        stale_actions[0]["arguments"]["route"],
        "slice.candidates.refresh"
    );

    let mut awaiting = summary();
    awaiting.eligible_work.clear();
    awaiting.slices_needing_result.push(NativeSliceSummary {
        slice_id: Uuid::new_v4(),
        slice_revision: 1,
        state: SliceState::Open,
    });
    let result_actions = native_actions(&awaiting).unwrap();
    assert_eq!(
        result_actions[0]["arguments"]["route"],
        "slice.result.record"
    );

    awaiting.slices_needing_result[0].state = SliceState::Blocked;
    let blocked_actions = native_actions(&awaiting).unwrap();
    assert!(
        blocked_actions
            .iter()
            .all(|action| action["arguments"]["route"] != "slice.result.record")
    );
    assert_eq!(
        blocked_actions[0]["arguments"]["route"],
        "slice.candidates.input"
    );

    let mut managed = summary();
    managed.eligible_work.clear();
    managed.pipeline_runs.push(NativePipelineRunSummary {
        run_id: Uuid::new_v4(),
        slice_id: Uuid::new_v4(),
        status: "blocked".into(),
    });
    assert_eq!(
        native_actions(&managed).unwrap()[0]["arguments"]["route"],
        "slice.pipeline.context"
    );
    managed.stale = true;
    let stale_managed = native_actions(&managed).unwrap();
    assert_eq!(
        stale_managed[0]["arguments"]["route"],
        "slice.pipeline.context"
    );
    assert_eq!(
        stale_managed[1]["arguments"]["route"],
        "slice.candidates.refresh"
    );
}
