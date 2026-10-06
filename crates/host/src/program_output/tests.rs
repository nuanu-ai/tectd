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
    let empty = encoded_len(&page_value(&page).unwrap()).unwrap();
    for text in ["plain", "\n\t\u{1f}", "\\\"quoted\\path", "日本語🙂"] {
        page.inputs[0].input = text.to_owned();
        let expected = encoded_len(&page_value(&page).unwrap()).unwrap() - empty;
        assert_eq!(guard.input_bytes(text).unwrap() as usize, expected);
    }
}

#[test]
fn compact_mutation_guard_keeps_large_fields_readable_and_checks_projection_capacity() {
    let mut program = draft();
    program.max_input_bytes = 6 * 1024 * 1024;
    program.intent = Some("y".repeat(3 * 1024 * 1024));
    program.name = Some("z".repeat(1024 * 1024));
    let guard = ProgramEncoding { capacity: 8192 };
    guard.check(&program).unwrap();
    for operation in ["begin", "save", "updated"] {
        let reply = mutation(&program, operation).unwrap();
        assert!(encoded_len(&reply).unwrap() <= 8192);
        assert!(reply["program"].get("intent").is_none());
        assert_eq!(
            reply["field_destinations"]["full_program_and_original_inputs"]["arguments"]["params"]
                ["program_id"],
            program.id.to_string()
        );
        assert_eq!(reply["saved"], operation == "save");
        println!(
            "program.{operation} envelope_bytes={}",
            encoded_len(&reply).unwrap()
        );
    }
    assert_eq!(
        super::program(program.clone()).unwrap(),
        mutation(&program, "updated").unwrap()
    );
    assert_eq!(
        ProgramEncoding { capacity: 1 }.check(&program),
        Err(Error::RequestTooLarge)
    );
    program.max_input_bytes = -1;
    assert_eq!(guard.check(&program), Err(Error::InvalidArguments));
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
    let capacity = encoded_len(&page_value(&first).unwrap()).unwrap();
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
            .any(|call| call["tool"] == "query"
                && call["arguments"]["route"] == "program.get"
                && call["arguments"]["params"]["after_input"] == 1)
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
        list_actions(&first.programs, &first.next_after).unwrap(),
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
        result["actions"].as_array().unwrap().last().unwrap()["arguments"]["route"],
        "program.begin"
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
        .find(|a| a["tool"] == "command" && a["arguments"]["route"] == "program.save")
        .unwrap();
    assert_eq!(save["arguments"]["params"]["input_cursor"], 8);
}

#[test]
fn large_program_field_and_single_input_use_pinned_lossless_fragments() {
    let mut program = draft();
    program.intent = Some("日本語🙂\\\"\n".repeat(4000));
    program.latest_input = 2;
    let original = ProgramPage {
        program,
        inputs: vec![input(1, &"input".repeat(6000)), input(2, "later")],
        next_after_input: None,
    };
    let mut expected = original.clone();
    expected.inputs.truncate(1);
    expected.next_after_input = Some(1);
    let expected = serde_json::to_vec(&serde_json::to_value(&expected).unwrap()).unwrap();
    let mut window = crate::planning_read::Window::default();
    let mut assembled = Vec::new();
    let mut maximum = 0;
    loop {
        let page = page_read(original.clone(), Some(0), 25, &window, None, 8192).unwrap();
        let bytes = encoded_len(&page).unwrap();
        maximum = maximum.max(bytes);
        assert!(bytes <= 8192);
        assert_eq!(page["kind"], "fragment");
        assembled.extend_from_slice(page["text"].as_str().unwrap().as_bytes());
        let Some(next) = page["next_offset_bytes"].as_u64() else {
            assert_eq!(page["actions"].as_array().unwrap().len(), 1);
            let next_page = &page["actions"][0];
            assert_eq!(next_page["arguments"]["params"]["after_input"], 1);
            assert!(
                next_page["arguments"]["params"]
                    .get("representation_digest")
                    .is_none()
            );
            crate::api::decode_public_call("query", next_page["arguments"].clone()).unwrap();
            break;
        };
        let action = &page["actions"][0];
        let call = crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
        let crate::tools::Invocation::Program(crate::program_tools::ProgramInvocation::Get {
            window: next_window,
            program_revision,
            after_input,
            ..
        }) = crate::tools::parse_invocation(call.name, call.arguments).unwrap()
        else {
            panic!("real program read")
        };
        assert_eq!(after_input, Some(0));
        assert_eq!(program_revision, Some(original.program.revision));
        assert_eq!(next_window.offset_bytes, Some(next));
        window = next_window;
    }
    assert!(
        assembled == expected,
        "lossless canonical JSON bytes differ"
    );
    println!("program.get max_envelope_bytes={maximum}");
    assert!(
        page_read(original, Some(0), 25, &window, Some(999), 8192)
            .unwrap_err()
            .refusal()
            .is_some()
    );
}

#[test]
fn guidance_revision_two_keeps_v1_archive_and_explains_actual_success_wire_field() {
    let old = include_str!("../../../../skills/tectd-program/revisions/1.md");
    assert!(old.contains("# TectD Program"));
    assert!(!old.contains("## Effective save contract"));
    let method = StaticProgramGuidance.planning_method();
    assert_eq!(method.version, "2");
    assert_eq!(
        method.digest,
        format!("{:x}", sha2::Sha256::digest(PROGRAM_SKILL.as_bytes()))
    );
    assert!(method.body.contains("arguments.params.success"));
    assert!(
        method
            .body
            .contains("There is no `program_success` argument")
    );
    let action = save_action(&draft(), 0).unwrap();
    assert_eq!(
        action["input"]["effective_contract"]["missing_fields"]
            .as_array()
            .unwrap()
            .len(),
        6
    );
    let success = action["input"]["fields"]
        .as_array()
        .unwrap()
        .iter()
        .find(|field| field["path"] == "arguments.params.success")
        .unwrap();
    assert!(
        success["format"]
            .as_str()
            .unwrap()
            .contains("Observable outcomes")
    );
}
