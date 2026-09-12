use super::*;
use tect_domain::{Session, SetupContext, SetupDirectory, Workspace, WorkspaceState};

fn draft() -> Setup {
    Setup::draft(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        SetupDirectory {
            path: "/workspace/current-task".into(),
            device: 1,
            inode: 2,
        },
        0,
    )
}
fn input(sequence: i64, text: &str) -> SetupInput {
    SetupInput {
        id: Uuid::max(),
        sequence,
        request_id: Uuid::max(),
        session_id: Uuid::max(),
        input: text.into(),
    }
}
fn sample(setup: Setup, texts: &[&str]) -> SetupPage {
    SetupPage {
        setup,
        inputs: texts
            .iter()
            .enumerate()
            .map(|(i, text)| input(i as i64 + 1, text))
            .collect(),
        next_after_input: None,
        file: FileObservation::missing(),
    }
}

#[test]
fn escaped_original_cost_and_paging_preserve_whole_messages() {
    let guard = SetupEncoding {
        capacity: crate::frame::MAX_FRAME_BYTES,
    };
    let mut p = sample(draft(), &[""]);
    let empty = encoded_len(&page_value(&p)).unwrap();
    for text in ["plain", "\\\"\n\t", "Русский 日本語🙂"] {
        p.inputs[0].input = text.into();
        assert_eq!(
            guard.input_bytes(text).unwrap() as usize,
            encoded_len(&page_value(&p)).unwrap() - empty
        );
    }
    let original = "Read all this original input. \\ \" \n日本語".repeat(200);
    let mut p = sample(draft(), &[&original]);
    p.next_after_input = Some(1);
    let capacity = encoded_len(&page_value(&p)).unwrap();
    let value = page(
        sample(p.setup, &[&original, &original, &original]),
        capacity,
    )
    .unwrap();
    assert_eq!(value["inputs"].as_array().unwrap().len(), 1);
    assert_eq!(value["inputs"][0]["input"], original);
    assert_eq!(value["next_after_input"], 1);
    assert_eq!(
        value["actions"].as_array().unwrap().last().unwrap()["tool"],
        "begin_program"
    );
}

#[test]
fn every_step_has_context_and_historical_status_is_not_fresh_verification() {
    for step in [
        SetupStep::Compose,
        SetupStep::WaitingInput,
        SetupStep::ReadyToApply,
        SetupStep::Complete,
    ] {
        let mut setup = draft();
        setup.current_step = step;
        let saved = saved(setup.clone());
        assert_eq!(saved["task_directory"], setup.directory.path);
        assert!(saved["setup"].get("directory").is_none());
        assert_eq!(saved["file"]["observed_now"], false);
        assert_eq!(saved["actions"][0]["arguments"]["name"], "tectd-setup");
        assert!(
            !saved["current_step_instruction"]
                .as_str()
                .unwrap()
                .is_empty()
        );
        let envelope = crate::responses::success(saved);
        assert!(envelope["content"][0]["text"].as_str().unwrap().len() <= 2000);
        let current = page(sample(setup, &[]), crate::frame::MAX_FRAME_BYTES).unwrap();
        assert_eq!(current["file"]["observed_now"], true);
    }
}

#[test]
fn recovery_uses_original_history_and_save_cursor_never_moves_back() {
    let mut setup = draft();
    setup.input_cursor = 8;
    setup.latest_input = 10;
    let mut p = sample(setup.clone(), &["old"]);
    p.inputs[0].sequence = 2;
    p.next_after_input = Some(2);
    let value = page(p, crate::frame::MAX_FRAME_BYTES).unwrap();
    let save = value["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["tool"] == "save_setup")
        .unwrap();
    assert_eq!(save["arguments"]["input_cursor"], 8);
    let response = crate::responses::failure(
        Error::StaleRevision,
        Some(("save_setup", &json!({"setup_id":setup.id,"revision":1}))),
    );
    let data: Value =
        serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["actions"][0], reload(setup.id));
}

#[test]
fn largest_accepted_original_stays_readable_after_draft_growth() {
    let guard = SetupEncoding {
        capacity: crate::frame::MAX_FRAME_BYTES,
    };
    let mut setup = draft();
    setup.max_input_bytes = guard.input_bytes(&"x".repeat(5 * 1024 * 1024)).unwrap();
    guard.check(&setup).unwrap();
    setup.content = Some("y".repeat(4 * 1024 * 1024));
    assert_eq!(guard.check(&setup), Err(Error::RequestTooLarge));
}

#[test]
fn legacy_name_capacity_falls_back_honestly_without_losing_enumeration() {
    let mut program = tect_domain::Program::draft(Uuid::new_v4(), Uuid::new_v4(), 0);
    program.name = Some("Full existing Program name".repeat(500));
    let list = tect_domain::ProgramList {
        programs: vec![program.summary()],
        next_after: None,
    };
    let list_value =
        crate::program_output::list(list.clone(), crate::frame::MAX_FRAME_BYTES).unwrap();
    let capacity = encoded_len(&list_value).unwrap();
    let mut state = WorkspaceState::opened(
        Workspace {
            id: program.workspace_id,
            key: "key".into(),
        },
        Session {
            id: Uuid::new_v4(),
            workspace_id: program.workspace_id,
            host_id: Uuid::new_v4(),
            native_session_id: Uuid::new_v4().to_string(),
            revoked: false,
        },
    );
    state.programs = list.programs;
    state.setup_context = Some(SetupContext {
        task_directory: "/task".into(),
        setup: Some(draft().summary()),
    });
    let result = crate::workspace_output::workspace(state, capacity).unwrap();
    assert_eq!(result["programs_delivery"], "use_list_programs");
    assert!(result["programs"].as_array().unwrap().is_empty());
    assert_eq!(result["next_after"], Value::Null);
    assert!(
        result["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call == &action("list_programs", json!({"limit":25})))
    );
    let envelope = crate::responses::success(result);
    assert!(
        !envelope["content"][0]["text"]
            .as_str()
            .unwrap()
            .contains("No Programs")
    );
    assert_eq!(list_value["programs"][0]["name"], program.name.unwrap());
}

#[test]
fn uncertain_apply_replays_exact_ready_revision_and_auth_errors_disclose_nothing() {
    let args = json!({"setup_id":Uuid::new_v4(),"revision":7});
    let response =
        crate::responses::failure(Error::StorageUnavailable, Some(("apply_setup", &args)));
    let data: Value =
        serde_json::from_str(response["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(data["actions"][0], action("apply_setup", args.clone()));
    let denied = crate::responses::failure(Error::SetupUnavailable, Some(("get_setup", &args)));
    let data: Value = serde_json::from_str(denied["content"][1]["text"].as_str().unwrap()).unwrap();
    assert!(data["actions"].as_array().unwrap().is_empty());
    assert!(data.get("setup").is_none());
}
