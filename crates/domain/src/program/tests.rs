use super::*;

fn patch(p: &Program) -> SaveProgram {
    SaveProgram {
        program_id: p.id,
        revision: p.revision,
        input_cursor: p.input_cursor,
        name: TextPatch::Unchanged,
        intent: TextPatch::Unchanged,
        basis: TextPatch::Unchanged,
        boundaries: TextPatch::Unchanged,
        constraints: TextPatch::Unchanged,
        success: TextPatch::Unchanged,
        mode: None,
        working_notes: TextPatch::Unchanged,
        pending_question: TextPatch::Unchanged,
        complete: false,
        consumed_knowledge: None,
    }
}

fn complete(p: &Program) -> SaveProgram {
    let mut change = patch(p);
    change.input_cursor = p.latest_input;
    change.name = TextPatch::Set(Some("Project".into()));
    change.intent = TextPatch::Set(Some("Make the stated outcome possible.".into()));
    change.basis = TextPatch::Set(Some("Owner narrative; no external evidence yet.".into()));
    change.boundaries = TextPatch::Set(Some(
        "Include the stated outcome; exclude implementation.".into(),
    ));
    change.constraints = TextPatch::Set(Some("No explicit constraints were supplied.".into()));
    change.success = TextPatch::Set(Some("The stated outcome can be observed.".into()));
    change.mode = Some(ProgramMode::Prod);
    change.complete = true;
    change
}

#[test]
fn completion_preserves_identity_and_open_edits_preserve_required_fields() {
    let draft = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 100);
    let opened = draft.saved(&complete(&draft)).unwrap();
    assert_eq!(opened.id, draft.id);
    assert_eq!(opened.status, ProgramStatus::Open);
    assert_eq!(opened.current_step, ProgramStep::Ready);
    let mut question = patch(&opened);
    question.pending_question =
        TextPatch::Set(Some("Which explicit alternative is intended?".into()));
    let waiting = opened.saved(&question).unwrap();
    assert_eq!(waiting.status, ProgramStatus::Open);
    assert_eq!(waiting.name, opened.name);
    assert_eq!(waiting.current_step, ProgramStep::WaitingInput);
    let mut clearing = patch(&waiting);
    clearing.name = TextPatch::Set(None);
    assert_eq!(waiting.saved(&clearing), Err(Error::ProgramIncomplete));
    assert_eq!(waiting.name, opened.name);
}

#[test]
fn a_new_original_invalidates_old_saves_and_completion_requires_its_coverage() {
    let p = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 100);
    let old = complete(&p);
    let next = p.with_new_input(200).unwrap();
    assert_eq!(next.saved(&old), Err(Error::StaleRevision));
    let mut fresh = old;
    fresh.revision = next.revision;
    assert_eq!(next.saved(&fresh), Err(Error::InputPending));
    fresh.input_cursor = next.latest_input;
    assert_eq!(next.saved(&fresh).unwrap().status, ProgramStatus::Open);
    assert_eq!(p.revision, 1);
    assert_eq!(p.latest_input, 1);
}

#[test]
fn question_and_blank_required_text_cannot_complete() {
    let p = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 100);
    let mut c = complete(&p);
    c.constraints = TextPatch::Set(Some(" \n\t".into()));
    assert_eq!(p.saved(&c), Err(Error::ProgramIncomplete));
    c = complete(&p);
    c.pending_question = TextPatch::Set(Some("Critical unresolved choice".into()));
    assert_eq!(p.saved(&c), Err(Error::ProgramIncomplete));
    c.pending_question = TextPatch::Set(Some("   ".into()));
    assert!(p.saved(&c).unwrap().pending_question.is_none());
}

#[test]
fn mode_is_required_to_complete_and_on_every_save_of_an_open_program() {
    let draft = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 100);
    assert_eq!(draft.mode, None);
    // A Draft saves without a mode and keeps none.
    let mut partial = patch(&draft);
    partial.name = TextPatch::Set(Some("Project".into()));
    assert_eq!(draft.saved(&partial).unwrap().mode, None);
    // Completion without a mode is refused like a missing text field.
    let mut unstated = complete(&draft);
    unstated.mode = None;
    assert_eq!(draft.saved(&unstated), Err(Error::ProgramIncomplete));
    // A mode saved into the Draft earlier counts at completion.
    let mut early = patch(&draft);
    early.mode = Some(ProgramMode::Demo);
    let staged = draft.saved(&early).unwrap();
    assert_eq!(staged.status, ProgramStatus::Draft);
    let mut later = complete(&staged);
    later.mode = None;
    assert_eq!(staged.saved(&later).unwrap().mode, Some(ProgramMode::Demo));

    let opened = draft.saved(&complete(&draft)).unwrap();
    assert_eq!(opened.mode, Some(ProgramMode::Prod));
    // Omission keeps the mode; a value replaces it.
    let kept = opened.saved(&patch(&opened)).unwrap();
    assert_eq!(kept.mode, Some(ProgramMode::Prod));
    let mut raised = patch(&kept);
    raised.mode = Some(ProgramMode::SeriousProd);
    let raised = kept.saved(&raised).unwrap();
    assert_eq!(raised.mode, Some(ProgramMode::SeriousProd));
    assert_eq!(raised.status, ProgramStatus::Open);

    // A Program opened before the field existed has none: every save is refused
    // until one states it.
    let mut legacy = opened.clone();
    legacy.mode = None;
    assert_eq!(legacy.saved(&patch(&legacy)), Err(Error::ProgramIncomplete));
    let mut notes = patch(&legacy);
    notes.working_notes = TextPatch::Set(Some("Continue here.".into()));
    assert_eq!(legacy.saved(&notes), Err(Error::ProgramIncomplete));
    notes.mode = Some(ProgramMode::Mvp);
    let stated = legacy.saved(&notes).unwrap();
    assert_eq!(stated.mode, Some(ProgramMode::Mvp));
    assert_eq!(stated.working_notes.as_deref(), Some("Continue here."));
}

#[test]
fn mode_patch_keeps_on_omission_takes_one_of_four_values_and_refuses_null() {
    let base = serde_json::json!({"program_id":Uuid::new_v4(),"revision":1,"input_cursor":0});
    let omitted: SaveProgram = serde_json::from_value(base.clone()).unwrap();
    assert_eq!(omitted.mode, None);
    assert!(serde_json::json!(omitted).get("mode").is_none());
    let names = ["demo", "mvp", "prod", "serious_prod"];
    assert_eq!(ProgramMode::ALL.map(ProgramMode::as_str), names);
    for (mode, name) in ProgramMode::ALL.into_iter().zip(names) {
        let mut stated = base.clone();
        stated["mode"] = serde_json::json!(name);
        let change: SaveProgram = serde_json::from_value(stated.clone()).unwrap();
        assert_eq!(change.mode, Some(mode));
        assert_eq!(serde_json::json!(change)["mode"], name);
        assert!(!mode.description().trim().is_empty());
    }
    for invalid in [
        serde_json::Value::Null,
        serde_json::json!(""),
        serde_json::json!("production"),
        serde_json::json!("Prod"),
        serde_json::json!(2),
    ] {
        let mut stated = base.clone();
        stated["mode"] = invalid;
        assert!(serde_json::from_value::<SaveProgram>(stated).is_err());
    }
}

#[test]
fn program_without_a_mode_serializes_without_the_key_and_reads_back() {
    let draft = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 100);
    let bare = serde_json::json!(draft);
    assert!(bare.get("mode").is_none());
    assert_eq!(serde_json::from_value::<Program>(bare).unwrap().mode, None);
    let opened = draft.saved(&complete(&draft)).unwrap();
    let stated = serde_json::json!(opened);
    assert_eq!(stated["mode"], "prod");
    assert_eq!(
        serde_json::from_value::<Program>(stated).unwrap().mode,
        Some(ProgramMode::Prod)
    );
}
