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
        "slice.candidates.context"
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

fn opened() -> WorkspaceState {
    let workspace = tect_domain::Workspace {
        id: Uuid::new_v4(),
        key: "projection-test".into(),
    };
    let session = tect_domain::Session {
        id: Uuid::new_v4(),
        workspace_id: workspace.id,
        host_id: Uuid::new_v4(),
        native_session_id: "projection-session".into(),
        revoked: false,
    };
    WorkspaceState::opened(workspace, session)
}

fn program(name: &str) -> ProgramSummary {
    let mut program = tect_domain::Program::draft(Uuid::new_v4(), Uuid::new_v4(), 0);
    program.name = Some(name.into());
    program.summary()
}

fn populated() -> WorkspaceState {
    let mut state = opened();
    state.native_planning.push(summary());
    state.programs.push(program("existing program"));
    state.next_after = Some(state.programs[0].cursor().encode());
    state.candidate_sets.push(tect_domain::CandidateSetSummary {
        id: Uuid::new_v4(),
        program_id: state.programs[0].id,
        revision: 1,
        status: tect_domain::CandidateSetStatus::Draft,
        boundary: tect_domain::CandidateBoundary::Finite,
        snapshot_id: Uuid::new_v4(),
        input_cursor: 0,
        latest_input: 0,
    });
    state.setup_context = Some(SetupContext {
        task_directory: "/known/task".into(),
        setup: Some(tect_domain::SetupSummary {
            id: Uuid::new_v4(),
            status: tect_domain::SetupStatus::Draft,
            revision: 1,
            current_step: tect_domain::SetupStep::Compose,
        }),
    });
    state
}

fn suppressed(result: &Value) {
    assert_eq!(result["next_action"], Value::Null);
    assert_eq!(result["actions"], json!([]));
}

#[test]
fn trusted_suppression_preserves_null_for_existing_workspace() {
    let mut state = opened();
    state.next_action = None;
    let result = workspace(state.clone(), crate::frame::MAX_FRAME_BYTES).unwrap();
    suppressed(&result);
    assert_eq!(result["workspace"], json!(state.workspace));
    assert_eq!(result["session"], json!(state.session));
}

#[test]
fn trusted_suppression_precedes_native_candidate_program_and_setup_suggestions() {
    let mut state = populated();
    let ordinary = workspace(state.clone(), crate::frame::MAX_FRAME_BYTES).unwrap();
    for route in [
        "slice.open",
        "slice.candidates.context",
        "scope.candidates.context",
        "setup.get",
        "program.get",
        "program.list",
        "setup.inspect",
        "program.begin",
    ] {
        assert!(
            ordinary["actions"]
                .as_array()
                .unwrap()
                .iter()
                .any(|call| call["arguments"]["route"] == route),
            "{route}"
        );
    }
    state.next_action = None;
    let result = workspace(state.clone(), crate::frame::MAX_FRAME_BYTES).unwrap();
    suppressed(&result);
    assert_eq!(result["native_planning"], json!(state.native_planning));
    assert_eq!(result["candidate_sets"], json!(state.candidate_sets));
    assert_eq!(result["setup_context"], json!(state.setup_context));
    let discovery = discovery(
        SetupDiscovery {
            state,
            file: FileObservation::missing(),
        },
        crate::frame::MAX_FRAME_BYTES,
        None,
    )
    .unwrap();
    suppressed(&discovery);
}

#[test]
fn trusted_suppression_precedes_unopened_workspace_suggestion() {
    let mut state = WorkspaceState::unopened();
    state.next_action = None;
    suppressed(&workspace(state, crate::frame::MAX_FRAME_BYTES).unwrap());
}

#[test]
fn enabled_owner_shaped_states_keep_open_inspect_begin_and_setup_input() {
    let unopened = workspace(WorkspaceState::unopened(), crate::frame::MAX_FRAME_BYTES).unwrap();
    assert_eq!(unopened["next_action"], "workspace.open");
    assert_eq!(unopened["actions"].as_array().unwrap().len(), 1);
    let result = workspace(opened(), crate::frame::MAX_FRAME_BYTES).unwrap();
    assert_eq!(result["next_action"], "setup.inspect");
    assert_eq!(result["actions"][0]["arguments"]["route"], "setup.inspect");
    assert_eq!(result["actions"][1]["arguments"]["route"], "program.begin");
    let missing = discovery(
        SetupDiscovery {
            state: opened(),
            file: FileObservation::missing(),
        },
        crate::frame::MAX_FRAME_BYTES,
        None,
    )
    .unwrap();
    assert_eq!(missing["next_action"], "setup.begin");
    assert_eq!(missing["actions"][1]["arguments"]["route"], "program.begin");
}

#[test]
fn trusted_suppression_survives_actual_program_paging() {
    let mut state = opened();
    state.next_action = None;
    state.programs = (0..3).map(|_| program(&"whole name".repeat(100))).collect();
    let mut first = state.clone();
    first.programs.truncate(1);
    first.next_after = Some(first.programs[0].cursor().encode());
    let capacity = crate::responses::encoded_len(
        &value(
            &first,
            None,
            false,
            false,
            &crate::workspace_state::params(
                crate::workspace_state::Origin::State,
                Uuid::nil(),
                None,
                None,
            ),
        )
        .unwrap(),
    )
    .unwrap();
    let result = workspace(state.clone(), capacity).unwrap();
    suppressed(&result);
    assert_eq!(result["programs_delivery"], "listed");
    assert_eq!(result["programs"].as_array().unwrap().len(), 1);
    assert_eq!(result["programs"][0]["name"], json!(state.programs[0].name));
    assert_eq!(result["next_after"], first.next_after.unwrap());
    assert!(crate::responses::encoded_len(&result).unwrap() <= capacity);
}

#[test]
fn trusted_suppression_survives_forced_capacity_fallback() {
    let mut state = populated();
    state.next_action = None;
    state.programs = vec![program(&"oversized whole name".repeat(1000))];
    let mut fallback = state.clone();
    fallback.programs.clear();
    fallback.next_after = None;
    let capacity = crate::responses::encoded_len(
        &value(
            &fallback,
            None,
            true,
            false,
            &crate::workspace_state::params(
                crate::workspace_state::Origin::State,
                Uuid::nil(),
                None,
                None,
            ),
        )
        .unwrap(),
    )
    .unwrap();
    assert!(
        crate::responses::encoded_len(
            &value(
                &state,
                None,
                false,
                false,
                &crate::workspace_state::params(
                    crate::workspace_state::Origin::State,
                    Uuid::nil(),
                    None,
                    None
                )
            )
            .unwrap()
        )
        .unwrap()
            > capacity
    );
    let result = workspace(state, capacity).unwrap();
    suppressed(&result);
    assert_eq!(result["programs_delivery"], "use_list_programs");
    assert_eq!(result["programs"], json!([]));
    assert_eq!(result["next_after"], Value::Null);
    assert!(crate::responses::encoded_len(&result).unwrap() <= capacity);
}
