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
