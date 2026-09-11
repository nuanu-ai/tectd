use super::*;

fn draft() -> Program {
    Program::draft(Uuid::new_v4(), Uuid::new_v4(), 0)
}

fn input(sequence: i64, text: &str) -> ProgramInput {
    ProgramInput {
        id: Uuid::max(),
        sequence,
        request_id: Uuid::max(),
        session_id: Uuid::max(),
        input: text.to_owned(),
    }
}

#[test]
fn encoding_cost_includes_both_json_escape_layers_exactly() {
    let guard = ProgramEncoding {
        capacity: MAX_FRAME_BYTES,
    };
    let mut page = ProgramPage {
        program: draft(),
        inputs: vec![input(1, "")],
        next_after_input: None,
    };
    let empty = encoded_len(&page_value(&page)).unwrap();
    for text in ["plain", "\n\t\u{1f}", "\\\"quoted\\path", "日本語🙂"] {
        page.inputs[0].input = text.to_owned();
        let expected = encoded_len(&page_value(&page)).unwrap() - empty;
        assert_eq!(guard.input_bytes(text).unwrap() as usize, expected);
    }
}

#[test]
fn later_prd_growth_cannot_make_existing_original_unreadable() {
    let guard = ProgramEncoding {
        capacity: MAX_FRAME_BYTES,
    };
    let mut program = draft();
    program.max_input_bytes = guard.input_bytes(&"x".repeat(6 * 1024 * 1024)).unwrap();
    guard.check(&program).unwrap();
    program.intent = Some("y".repeat(3 * 1024 * 1024));
    assert_eq!(guard.check(&program), Err(Error::RequestTooLarge));
    program.intent = None;
    program.max_input_bytes = 0;
    program.name = Some("z".repeat(6 * 1024 * 1024));
    assert_eq!(
        guard.check(&program),
        Err(Error::RequestTooLarge),
        "a stored full name must remain enumerable with the maximum worktree set"
    );
}

#[test]
fn capacity_pages_keep_whole_input_entries_and_exact_cursor() {
    let original = "line\\quoted\n日本語".repeat(200);
    let mut program = draft();
    program.latest_input = 3;
    let first = ProgramPage {
        program: program.clone(),
        inputs: vec![input(1, &original)],
        next_after_input: Some(1),
    };
    let capacity = encoded_len(&page_value(&first)).unwrap();
    let result = page(
        ProgramPage {
            program,
            inputs: (1..=3).map(|seq| input(seq, &original)).collect(),
            next_after_input: None,
        },
        capacity,
    )
    .unwrap();
    assert_eq!(result["inputs"].as_array().unwrap().len(), 1);
    assert_eq!(result["inputs"][0]["input"], original);
    assert_eq!(result["next_after_input"], 1);
    assert!(
        result["actions"]
            .as_array()
            .unwrap()
            .iter()
            .any(|call| call["tool"] == "get_program" && call["arguments"]["after_input"] == 1)
    );
}

#[test]
fn capacity_pages_keep_full_names_and_creation_last() {
    let mut programs: Vec<_> = (0..3)
        .map(|_| {
            let mut p = draft();
            p.name = Some("full program name".repeat(200));
            p.summary()
        })
        .collect();
    programs.sort_by_key(|p| p.id);
    let first = ProgramList {
        programs: vec![programs[0].clone()],
        next_after: Some(programs[0].cursor().encode()),
    };
    let first_value = with_actions(
        json!(&first),
        list_actions(&first.programs, &first.next_after),
        Some(0),
    );
    let result = list(
        ProgramList {
            programs: programs.clone(),
            next_after: None,
        },
        encoded_len(&first_value).unwrap(),
    )
    .unwrap();
    assert_eq!(result["programs"].as_array().unwrap().len(), 1);
    assert_eq!(
        result["programs"][0]["name"],
        programs[0].name.as_ref().unwrap().as_str()
    );
    assert_eq!(result["next_after"], programs[0].cursor().encode());
    assert_eq!(
        result["actions"].as_array().unwrap().last().unwrap()["tool"],
        "begin_program"
    );
}

#[test]
fn original_history_reads_never_move_the_save_cursor_back() {
    let mut p = draft();
    p.input_cursor = 8;
    p.latest_input = 10;
    let result = page(
        ProgramPage {
            program: p,
            inputs: vec![input(2, "old")],
            next_after_input: Some(2),
        },
        MAX_FRAME_BYTES,
    )
    .unwrap();
    let save = result["actions"]
        .as_array()
        .unwrap()
        .iter()
        .find(|a| a["tool"] == "save_program")
        .unwrap();
    assert_eq!(save["arguments"]["input_cursor"], 8);
}
