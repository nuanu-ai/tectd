//! The point on an input recorded on the Program itself: the part of the state of
//! `intake` that a Program has, and the questions of the candidate levels.
use super::*;
use crate::SizingLevel;

/// The words recorded for a Program before a new input are its own inputs.
fn program_words() -> Vec<SizingWord> {
    ["Add invitations by email.", "Keep it small."]
        .into_iter()
        .map(|text| SizingWord {
            source: SizingWordSource::ProgramInput,
            text: text.into(),
        })
        .collect()
}

fn asked_on_the_program(mode: ProgramMode) -> ModelAdvice {
    ModelAdvice::of_program_input(&program(), mode, &program_words(), NEW_INPUT)
}

#[test]
fn the_state_of_a_program_input_holds_its_level_the_statement_the_mode_the_words_and_the_input() {
    for mode in ProgramMode::ALL {
        assert_eq!(
            asked_on_the_program(mode).questions().state,
            json!({"level":"input_to_program",
                "mode":{"level":mode.as_str(),"description":mode.description()},
                "program":{"name":"Invitations","intent":"Let users invite each other",
                    "basis":"Teams ask for it","boundaries":"Email invitations only",
                    "constraints":"No billing changes","success":"An invited user joins"},
                "user_words":[{"source":"program_input","text":"Add invitations by email."},
                    {"source":"program_input","text":"Keep it small."}],
                "new_input":NEW_INPUT})
        );
    }
    // No Scope and no candidates: a Program has no candidate set under it.
    let advice = asked_on_the_program(ProgramMode::Prod);
    let state = &advice.questions().state;
    assert_eq!(state.as_object().unwrap().len(), 5);
    assert!(state.get("scope").is_none() && state.get("candidates").is_none());
    // The level is the Program's own: no candidate set is planned on it.
    for planned in [SizingLevel::ProgramToScopes, SizingLevel::ScopeToSlices] {
        assert_ne!(state["level"], planned.as_str());
    }
}

#[test]
fn the_state_of_a_program_input_is_laid_out_as_intake_lays_out_the_same_parts() {
    // What `intake` sends for a candidate set without a saved draft, about the same
    // Program, product level, words and input, differs in the level alone.
    let program = program();
    for mode in ProgramMode::ALL {
        let model = ModelAdvice::of_program_input(&program, mode, &words(), NEW_INPUT);
        let intake = IntakePlanning::scope_candidates(None).unwrap().advice(
            &program,
            mode,
            &words(),
            NEW_INPUT,
        );
        let mut expected = intake.questions().state.clone();
        assert_eq!(expected["level"], "program_to_scopes");
        expected["level"] = json!("input_to_program");
        assert_eq!(model.questions().state, expected);
        assert_eq!(model.questions().state.to_string(), expected.to_string());
    }
    // A Program that is not stated yet and has no earlier words is laid out the same
    // way: every field of the statement is there, empty.
    let unstated = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 0);
    let first = ModelAdvice::of_program_input(&unstated, ProgramMode::Demo, &[], NEW_INPUT);
    let state = &first.questions().state;
    assert_eq!(
        state["program"],
        json!({"name":null,"intent":null,"basis":null,"boundaries":null,
            "constraints":null,"success":null})
    );
    assert_eq!(state["user_words"], json!([]));
    assert_eq!(state["new_input"], NEW_INPUT);
}

#[test]
fn an_input_recorded_on_the_program_is_asked_the_questions_of_the_candidate_levels() {
    let on_a_candidate_set = advice();
    for mode in ProgramMode::ALL {
        let on_the_program = asked_on_the_program(mode);
        let set = on_the_program.questions();
        assert!(set.validate().is_ok());
        assert_eq!(
            set.questions.keys().collect::<Vec<_>>(),
            ["practical", "specialist"]
        );
        // The same instructions, criteria and descriptions, text for text.
        assert_eq!(set.questions, on_a_candidate_set.questions().questions);
        assert_eq!(texts(&on_the_program), texts(&on_a_candidate_set));
        // And the same field: its largest sample and what it says it is about.
        assert_eq!(on_the_program.largest(), on_a_candidate_set.largest());
        assert_eq!(on_the_program.subject(), on_a_candidate_set.subject());
    }

    // The same answers become the same field under the same note.
    let given: BTreeMap<String, AdviceAnswer> = [
        ("practical", "routine", 0.6),
        ("specialist", "demanding", 0.8),
    ]
    .into_iter()
    .map(|(name, choice, probability)| {
        let answer = AdviceAnswer {
            choice: choice.into(),
            probabilities: BTreeMap::from([(choice.to_owned(), probability)]),
            confidence: None,
        };
        (name.to_owned(), answer)
    })
    .collect();
    let on_the_program = asked_on_the_program(ProgramMode::Prod);
    assert_eq!(
        on_the_program.shape(&given),
        on_a_candidate_set.shape(&given)
    );
    assert_eq!(
        json!(on_the_program.shape(&given)),
        json!({"status":"advised","note":MODEL_NOTE,"practical":"routine","practical_p":0.6,
            "specialist":"demanding","specialist_p":0.8})
    );
    assert!(on_the_program.kept(&given).is_empty());
}

/// The texts were written for an input of a candidate set. They hold for an input of
/// a Program as written only while every part of the state they name is a part the
/// Program-level state has.
#[test]
fn every_part_of_the_state_the_wording_names_is_in_the_state_of_a_program_input() {
    let advice = asked_on_the_program(ProgramMode::Prod);
    let state = advice.questions().state.as_object().unwrap();
    let mut named = Vec::new();
    for text in texts(&advice) {
        for (at, mark) in text.match_indices("state.") {
            let part = text[at + mark.len()..]
                .chars()
                .take_while(|symbol| symbol.is_ascii_lowercase() || *symbol == '_')
                .collect::<String>();
            assert!(state.contains_key(&part), "state.{part}: {text}");
            named.push(part);
        }
    }
    named.sort();
    named.dedup();
    assert_eq!(named, ["mode", "new_input", "user_words"]);
}
