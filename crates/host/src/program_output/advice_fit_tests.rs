//! How much advice the reply of `program.record_input` can carry: the field of `model`
//! alone, since `intake` is not asked about an input of a Program. Measured with the
//! real reply and the real encoder.
use super::*;
use crate::advice_reply::model_tests::model_bytes;
use crate::advice_reply::{ModelReplyRoom, finish_model};
use crate::json_fragment::READ_BUDGET;
use tect_application::AdviceOutputGuard;
use tect_domain::{AdviceField, AdviceShaping, ModelAdvice, ProgramMode, ProgramStatus};

/// The planning knowledge a recorded input captures, as far as its reply reads it:
/// the id, the digest and the workspace generation of the manifest go into the save
/// call the reply offers.
fn captured(program: &Program, generation: i64) -> tect_domain::PlanningKnowledgeStatus {
    let method = StaticProgramGuidance.planning_method();
    serde_json::from_value(json!({"manifest":{"id":program.id,"digest":"0".repeat(64),
        "stage":"program","owner_id":program.id,"owner_revision":program.revision,
        "input_revision":program.latest_input,"request_id":program.id,
        "policy_id":tect_domain::PLANNING_KNOWLEDGE_POLICY_ID,
        "policy_version":tect_domain::PLANNING_KNOWLEDGE_POLICY_VERSION,
        "task_context_digest":"0".repeat(64),"task_context":{},
        "workspace_generation":generation,
        "needs":{"stage":"program","roles":[],"expected_abstraction":"a",
            "policy_id":tect_domain::PLANNING_KNOWLEDGE_POLICY_ID,
            "policy_version":tect_domain::PLANNING_KNOWLEDGE_POLICY_VERSION,"method":method},
        "selected":[],"unresolved_needs":[]}}))
    .unwrap()
}

/// An Open Program stated in full, each PRD text field `statement` bytes long, right
/// after the command recorded a new input on it.
fn recorded_on_a_stated_program(statement: usize) -> Program {
    let id = Uuid::new_v4();
    let mut program = Program::draft(id, id, 0);
    let text = Some("s".repeat(statement));
    program.name = text.clone();
    program.intent = text.clone();
    program.basis = text.clone();
    program.boundaries = text.clone();
    program.constraints = text.clone();
    program.success = text;
    program.mode = Some(ProgramMode::Prod);
    program.status = ProgramStatus::Open;
    program.current_step = ProgramStep::Ready;
    program.input_cursor = 1;
    let mut program = program.with_new_input(0).unwrap();
    program.planning_knowledge = Some(captured(&program, 1));
    program
}

/// The Program whose reply takes the most bytes: nothing of it is stated, so the save
/// call the reply offers lists all seven fields as missing, and every number is at
/// its widest.
fn recorded_on_the_widest_program() -> Program {
    let id = Uuid::max();
    let mut program = Program::draft(id, id, 0);
    program.revision = i64::MAX;
    program.input_cursor = i64::MAX;
    program.latest_input = i64::MAX;
    program.planning_knowledge = Some(captured(&program, i64::MAX));
    program
}

/// The `model` point for an input of a Program. What Jev reads changes nothing of
/// the field.
fn point() -> ModelAdvice {
    let id = Uuid::new_v4();
    ModelAdvice::of_program_input(&Program::draft(id, id, 0), ProgramMode::Prod, &[], "i")
}

/// The reply as the command builds it, the largest `model_advice` field, the field
/// the point gives without room, and what the read budget has left beside the
/// largest field, in bytes.
fn room(reply: &Value) -> (usize, usize, usize, usize) {
    let base = encoded_len(reply).unwrap();
    let largest = model_bytes(reply, &point().largest());
    let short = model_bytes(reply, &ModelAdvice::without_room());
    let advised = finish_model(reply.clone(), Some(point().largest()), READ_BUDGET).unwrap();
    let left = READ_BUDGET - encoded_len(&advised).unwrap();
    assert_eq!(base + largest + left, READ_BUDGET);
    (base, largest, short, left)
}

#[test]
fn program_input_reply_has_measured_room_for_model_advice() {
    let reply = program(recorded_on_a_stated_program(16)).unwrap();
    assert_eq!(reply["operation"], "updated");
    assert_eq!(reply["program"]["current_step"], "compose");
    assert!(reply.get("advice").is_none() && reply.get("model_advice").is_none());
    let (base, largest, short, left) = room(&reply);
    println!(
        "program.record_input base_envelope_bytes={base}; largest model advice bytes={largest}; \
         its advice_too_large field bytes={short}; bytes_left beside the largest model \
         field={left}"
    );
    assert_eq!((base, largest, short, left), (3842, 819, 194, 3531));

    // The reply says where the Program is read and does not carry its statement: a
    // statement of several kilobytes leaves the same room.
    let long = program(recorded_on_a_stated_program(4096)).unwrap();
    assert_eq!(encoded_len(&long).unwrap(), base);
    assert_eq!(room(&long), (base, largest, short, left));

    // The widest reply the command can build still leaves the largest field its room.
    let widest = program(recorded_on_the_widest_program()).unwrap();
    let save = &widest["actions"][1];
    assert_eq!(
        save["arguments"]["params"]["consumed_knowledge"]["workspace_generation"],
        i64::MAX
    );
    let missing = &save["input"]["effective_contract"]["missing_fields"];
    assert_eq!(missing.as_array().unwrap().len(), 7);
    let (widest_base, widest_largest, widest_short, widest_left) = room(&widest);
    println!(
        "program.record_input widest base_envelope_bytes={widest_base}; bytes_left beside \
         the largest model field={widest_left}"
    );
    assert_eq!((widest_largest, widest_short), (largest, short));
    assert_eq!((widest_base, widest_left), (4105, 3268));
}

/// The one point of the command is measured on the reply as the command built it.
/// With room for its largest field Jev is asked; with less it is not and the point
/// gives its short field; and a reply without room even for that answers with an
/// error, as the replies of the other input commands do at their own limit.
#[test]
fn the_guard_and_the_reply_that_leaves_agree_on_the_room_of_a_program_input_reply() {
    let reply = program(recorded_on_a_stated_program(16)).unwrap();
    let bytes = serde_json::to_vec(&reply).unwrap();
    let (base, largest, short, _) = room(&reply);
    let (advice, without_room) = (point().largest(), ModelAdvice::without_room());
    assert_eq!(without_room, AdviceField::too_large(&point().subject()));
    let asked = |capacity| {
        let room = ModelReplyRoom {
            reply: &reply,
            capacity,
        };
        room.check_reply(&advice)
    };

    // No field from the point: the reply leaves as the command built it.
    for capacity in [READ_BUDGET, base, 1] {
        let finished = finish_model(reply.clone(), None, capacity).unwrap();
        assert_eq!(serde_json::to_vec(&finished).unwrap(), bytes);
    }
    // Room for the largest field: Jev is asked and whatever it answers fits.
    assert_eq!(asked(base + largest), Ok(()));
    let finished = finish_model(reply.clone(), Some(advice.clone()), base + largest).unwrap();
    assert_eq!(finished["model_advice"], json!(advice));
    assert!(finished.get("advice").is_none());
    let mut without = finished;
    without.as_object_mut().unwrap().remove("model_advice");
    assert_eq!(serde_json::to_vec(&without).unwrap(), bytes);
    // One byte less: Jev is not asked, and the short field leaves with the reply
    // down to the capacity that just holds it.
    for capacity in [base + largest - 1, base + short] {
        assert_eq!(asked(capacity), Err(Error::RequestTooLarge), "{capacity}");
        let finished = finish_model(reply.clone(), Some(without_room.clone()), capacity).unwrap();
        assert_eq!(finished["model_advice"]["reason"], "advice_too_large");
    }
    // Below that the recorded input answers with an error.
    assert_eq!(
        finish_model(reply.clone(), Some(without_room), base + short - 1),
        Err(Error::RequestTooLarge)
    );
}
