use super::*;
use crate::workspace_state::{Origin, View, parse, read};
use tect_domain::{
    NativeKnowledgeChangeSummary, PipelineRunStatus, Session, StateStatus, Workspace,
};

fn state() -> WorkspaceState {
    let id = Uuid::new_v4();
    let mut state = WorkspaceState::opened(
        Workspace {
            id,
            key: "delivery".into(),
        },
        Session {
            id: Uuid::new_v4(),
            workspace_id: id,
            host_id: Uuid::new_v4(),
            native_session_id: Uuid::new_v4().to_string(),
            revoked: false,
        },
    );
    // Escaping and multibyte content force real byte slicing even with no Programs.
    state.selected_worktrees.push(tect_domain::WorktreeSummary {
        id: Uuid::new_v4(),
        repository_id: Uuid::new_v4(),
        path: "\"\\中文🙂".repeat(3000),
    });
    for _ in 0..6 {
        let mut native = tests::summary();
        native.pipeline_runs = (0..6)
            .map(|_| tect_domain::NativePipelineRunSummary {
                run_id: Uuid::new_v4(),
                slice_id: Uuid::new_v4(),
                status: "completed".into(),
            })
            .collect();
        native.knowledge_changes.push(NativeKnowledgeChangeSummary {
            change_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            slice_id: Uuid::new_v4(),
            status: PipelineRunStatus::Completed,
            current_phase_id: Some(tect_domain::KnowledgeChangePhaseId::KcQualifyEvidence),
        });
        state.native_planning.push(native);
    }
    state
}

fn reconstruct(full: Value, mut arguments: Value) -> Vec<u8> {
    let mut bytes = vec![];
    loop {
        let query = parse(arguments.clone()).unwrap();
        let page = read(full.clone(), &query, 8192, vec![]).unwrap();
        assert!(crate::responses::encoded_len(&page).unwrap() <= 8192);
        assert!(page["returned_bytes"].as_u64().unwrap() <= 4096);
        bytes.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
        if page["next_offset_bytes"].is_null() {
            break;
        }
        arguments = page["actions"][0]["arguments"]["params"].clone();
        crate::api::decode_public_call("query", page["actions"][0]["arguments"].clone()).unwrap();
    }
    bytes
}

#[test]
fn all_producers_defer_without_losing_identity_and_roundtrip_stable_business_actions() {
    let state = state();
    let file = FileObservation::missing();
    for origin in [Origin::Opened, Origin::State, Origin::Discovery] {
        let params = crate::workspace_state::params(
            origin,
            Uuid::new_v4(),
            (origin == Origin::Discovery).then(|| "/tmp".into()),
            (origin == Origin::Discovery).then_some(8192),
        );
        let observation = (origin == Origin::Discovery).then_some(file.clone());
        let original = logical(
            &state,
            observation.as_ref(),
            origin == Origin::Opened,
            &params,
        )
        .unwrap();
        assert_eq!(
            original,
            logical(
                &state,
                observation.as_ref(),
                origin == Origin::Opened,
                &params
            )
            .unwrap()
        );
        let root = encode(
            state.clone(),
            observation,
            8192,
            origin == Origin::Opened,
            params,
        )
        .unwrap();
        assert_eq!(root["status"], json!(StateStatus::Ready));
        assert_eq!(root["workspace"], json!(state.workspace));
        assert_eq!(root["session"], json!(state.session));
        assert_eq!(root["state_delivery"]["kind"], "deferred");
        assert!(root.get("programs").is_none());
        assert!(root.get("native_planning").is_none());
        assert_eq!(root["actions"].as_array().unwrap().len(), 1);
        if origin == Origin::Opened {
            assert_eq!(root["response_rules"], crate::responses::RESPONSE_RULES);
        }
        let args = root["actions"][0]["arguments"]["params"].clone();
        let rebuilt = reconstruct(original.clone(), args.clone());
        assert_eq!(
            serde_json::from_slice::<Value>(&rebuilt).unwrap(),
            json!({"state":original})
        );
        assert_eq!(
            format!("{:x}", Sha256::digest(&rebuilt)),
            root["state_delivery"]["representation_digest"]
        );
        assert_eq!(rebuilt.len() as u64, root["state_delivery"]["total_bytes"]);
        let mut eof = args;
        eof["offset_bytes"] = json!(rebuilt.len());
        let page = read(original.clone(), &parse(eof).unwrap(), 8192, vec![]).unwrap();
        assert_eq!(page["returned_bytes"], 0);
        assert!(page["actions"].as_array().unwrap().is_empty());
        let mut changed = original;
        changed["selected_worktrees"][0]["path"] = json!("changed");
        let err = read(
            changed,
            &parse(root["actions"][0]["arguments"]["params"].clone()).unwrap(),
            8192,
            vec![],
        )
        .unwrap_err();
        assert_eq!(
            err.refusal().unwrap().path.as_deref(),
            Some("arguments.params.representation_digest")
        );
    }
}

#[test]
fn small_ordinary_state_and_low_capacity_guard() {
    let mut state = state();
    state.selected_worktrees.clear();
    state.native_planning.clear();
    for producer in [workspace, opened] {
        let ordinary = producer(state.clone(), 8192).unwrap();
        assert!(ordinary.get("state_delivery").is_none());
        assert_eq!(ordinary["programs_delivery"], "listed");
        assert_eq!(producer(state.clone(), 1), Err(Error::RequestTooLarge));
    }
    let seed = Uuid::new_v4();
    let p =
        crate::workspace_state::params(Origin::Discovery, seed, Some("/tmp".into()), Some(8192));
    let a = logical(&state, Some(&FileObservation::missing()), false, &p).unwrap();
    let calls = a["actions"].as_array().unwrap();
    let setup = calls
        .iter()
        .find(|call| call["arguments"]["route"] == "setup.begin")
        .unwrap();
    let program = calls
        .iter()
        .find(|call| call["arguments"]["route"] == "program.begin")
        .unwrap();
    assert_ne!(
        setup["arguments"]["params"]["request_id"],
        program["arguments"]["params"]["request_id"]
    );
    assert_eq!(p.view, View::Root);
}

#[test]
fn six_programs_heads_and_native_summaries_reconstruct_without_projection_loss() {
    let mut state = state();
    state.selected_worktrees.clear();
    for summary in &mut state.native_planning {
        summary.pipeline_runs.truncate(1);
    }
    for _ in 0..6 {
        let program_id = Uuid::new_v4();
        state.programs.push(tect_domain::ProgramSummary {
            id: program_id,
            status: tect_domain::ProgramStatus::Draft,
            revision: 1,
            name: Some("program".into()),
            current_step: tect_domain::ProgramStep::Compose,
        });
        state.candidate_sets.push(tect_domain::CandidateSetSummary {
            id: Uuid::new_v4(),
            program_id,
            revision: 2,
            status: tect_domain::CandidateSetStatus::Draft,
            boundary: tect_domain::CandidateBoundary::Finite,
            snapshot_id: Uuid::new_v4(),
            input_cursor: 1,
            latest_input: 3,
        });
    }
    let params = crate::workspace_state::params(Origin::Opened, Uuid::new_v4(), None, None);
    let original = logical(&state, None, true, &params).unwrap();
    assert_eq!(
        state
            .native_planning
            .iter()
            .map(|summary| summary.pipeline_runs.len())
            .sum::<usize>(),
        6
    );
    assert_eq!(original["actions"].as_array().unwrap().len(), 32);
    let root = encode(state, None, 8192, true, params).unwrap();
    assert_eq!(root["state_delivery"]["kind"], "deferred");
    let bytes = reconstruct(
        original.clone(),
        root["actions"][0]["arguments"]["params"].clone(),
    );
    let restored = serde_json::from_slice::<Value>(&bytes).unwrap();
    assert_eq!(restored, json!({"state":original}));
    assert_eq!(restored["state"]["programs"].as_array().unwrap().len(), 6);
    assert_eq!(
        restored["state"]["candidate_sets"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    assert_eq!(
        restored["state"]["native_planning"][0]["knowledge_changes"][0]["current_phase_id"],
        "kc-qualify-evidence"
    );
}
