//! What every point of a recorded input lets Jev read, whatever the input was recorded
//! on: the level, the product level the input is judged at, the Program's statement,
//! the user's earlier words and the input itself. A level that has more to show, the
//! opened Scope or a saved draft, adds it to this object. There is one layout of the
//! Program and of the words, and it is this one.
use super::point;
use super::sizing_state::SizingWord;
use crate::{Program, ProgramMode};
use serde_json::{Value, json};

/// The level of an input recorded on the Program itself, on the way from the user's
/// input to the Program's statement: no candidate set stands under it.
pub(super) const PROGRAM_LEVEL: &str = "input_to_program";

/// The one JSON object Jev reads about an input recorded on `level`: the Program's
/// six PRD text fields, the product level with its meaning, the user's earlier words
/// in recorded order and the new input.
pub(super) fn of(
    level: &str,
    program: &Program,
    mode: ProgramMode,
    user_words: &[SizingWord],
    new_input: &str,
) -> Value {
    json!({"level":level,"mode":point::mode(mode),
        "program":{"name":program.name,"intent":program.intent,"basis":program.basis,
            "boundaries":program.boundaries,"constraints":program.constraints,
            "success":program.success},
        "user_words":user_words,"new_input":new_input})
}
