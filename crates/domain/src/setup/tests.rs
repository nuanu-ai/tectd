use super::*;

fn draft() -> Setup {
    Setup::draft(
        Uuid::new_v4(),
        Uuid::new_v4(),
        Uuid::new_v4(),
        SetupDirectory {
            path: "/fixture/task".into(),
            device: 1,
            inode: 2,
        },
        20,
    )
}
fn save(setup: &Setup, ready: bool) -> SaveSetup {
    SaveSetup {
        setup_id: setup.id,
        revision: setup.revision,
        input_cursor: setup.latest_input,
        ready,
        content: TextPatch::Set(Some("# Company\nPreserve inherited instructions.\n".into())),
        working_notes: TextPatch::Unchanged,
        pending_question: TextPatch::Set(None),
    }
}

#[test]
fn ready_requires_current_inputs_and_applied_is_terminal() {
    let initial = draft();
    assert_eq!(initial.validate_apply(1), Err(Error::InputPending));
    let ready = initial.saved(&save(&initial, true)).unwrap();
    let newer = ready.with_new_input(ready.revision, 300).unwrap();
    assert_eq!(newer.current_step, SetupStep::Compose);
    assert_eq!(
        newer.validate_apply(newer.revision),
        Err(Error::InputPending)
    );
    assert_eq!(newer.saved(&save(&ready, true)), Err(Error::StaleRevision));
    let ready = newer.saved(&save(&newer, true)).unwrap();
    let applied = ready.applied(ready.revision, &"a".repeat(64)).unwrap();
    assert_eq!(applied.validate_apply(ready.revision), Ok(()));
    assert_eq!(
        applied.validate_apply(applied.revision),
        Err(Error::StaleRevision)
    );
    assert_eq!(
        applied.saved(&save(&applied, true)),
        Err(Error::SetupAlreadyApplied)
    );
    assert_eq!(
        applied.with_new_input(applied.revision, 2),
        Err(Error::SetupAlreadyApplied)
    );
    assert_eq!(applied.applied_from_revision, Some(ready.revision));
}

#[test]
fn question_and_partial_patch_preserve_intent_without_regressing_consumption() {
    let initial = draft();
    let mut change = save(&initial, false);
    change.pending_question = TextPatch::Set(Some("Which business boundary?".into()));
    let waiting = initial.saved(&change).unwrap();
    assert_eq!(waiting.current_step, SetupStep::WaitingInput);
    let mut correction = save(&waiting, true);
    correction.content = TextPatch::Unchanged;
    correction.pending_question = TextPatch::Unchanged;
    assert_eq!(waiting.saved(&correction), Err(Error::SetupIncomplete));
    correction.pending_question = TextPatch::Set(None);
    let ready = waiting.saved(&correction).unwrap();
    assert_eq!(ready.content, waiting.content);
    correction.revision = ready.revision;
    correction.input_cursor = 0;
    assert_eq!(ready.saved(&correction), Err(Error::InputPending));
    let original = "  original message\nwith spaces  ";
    assert_eq!(validate_setup_input(Uuid::new_v4(), original), Ok(()));
    for invalid in ["", " ", "hidden\0suffix"] {
        assert_eq!(
            validate_setup_input(Uuid::new_v4(), invalid),
            Err(Error::InvalidArguments)
        );
    }
}

#[test]
fn setup_grants_are_component_safe_and_fail_closed() {
    let roots = vec!["/fixture/task".into()];
    assert!(crate::setup_path_is_granted("/fixture/task/sub", &roots));
    for path in [
        "/fixture/task-other",
        "/fixture/task/../other",
        "/fixture/task/./sub",
        "/fixture/task//sub",
        "relative",
        "/fixture/task/",
    ] {
        assert!(!crate::setup_path_is_granted(path, &roots));
    }
    assert!(!crate::setup_path_is_granted("/fixture/task", &[]));
}
