//! What Jev is asked about the executors of a recorded input: the state `intake`
//! sends for the same input and the two questions; and what the answers become.
mod program_input;
mod shaping;

use super::super::test_drafts::{stored_scopes, stored_slices};
use super::*;
use crate::{
    AdviceCompletion, AdviceExchange, AdviceReply, AdviceUnavailableReason, SizingScope,
    SizingWordSource,
};
use uuid::Uuid;

const NEW_INPUT: &str = "Compare the two mail providers, then switch the sender to the cheaper.";

fn program() -> Program {
    let mut program = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 0);
    program.name = Some("Invitations".into());
    program.intent = Some("Let users invite each other".into());
    program.basis = Some("Teams ask for it".into());
    program.boundaries = Some("Email invitations only".into());
    program.constraints = Some("No billing changes".into());
    program.success = Some("An invited user joins".into());
    program
}

fn words() -> Vec<SizingWord> {
    vec![
        SizingWord {
            source: SizingWordSource::ProgramInput,
            text: "Add invitations by email.".into(),
        },
        SizingWord {
            source: SizingWordSource::ScopeCandidateInput,
            text: "Keep it small.".into(),
        },
    ]
}

fn scope_statement() -> SizingScope {
    SizingScope {
        title: "Invitations".into(),
        outcome: "Users invite each other".into(),
        includes: vec!["email".into()],
        excludes: vec!["billing".into()],
    }
}

/// What an input can land on: a Scope candidate set and the Slice candidate set of an
/// opened Scope, each without a saved draft and with one.
fn plannings() -> Vec<IntakePlanning> {
    let scopes = stored_scopes(&[(Uuid::new_v4(), 1), (Uuid::new_v4(), 2)]);
    let slices = stored_slices(&[(Uuid::new_v4(), 1)]);
    vec![
        IntakePlanning::scope_candidates(None).unwrap(),
        IntakePlanning::scope_candidates(Some(&scopes)).unwrap(),
        IntakePlanning::slice_candidates(None, scope_statement()).unwrap(),
        IntakePlanning::slice_candidates(Some(&slices), scope_statement()).unwrap(),
    ]
}

fn asked_about(planning: &IntakePlanning, mode: ProgramMode) -> ModelAdvice {
    ModelAdvice::of(planning, &program(), mode, &words(), NEW_INPUT)
}

/// The point for an input of a Scope candidate set without a saved draft, at `prod`.
fn advice() -> ModelAdvice {
    asked_about(&plannings()[0], ProgramMode::Prod)
}

/// The criteria of one question, `ABSTAIN` included, in name order.
fn criteria(advice: &ModelAdvice, name: &str) -> Vec<String> {
    let question = &advice.questions().questions[name];
    question.criteria.keys().cloned().collect()
}

/// Every text the point sends to Jev or to the agent.
fn texts(advice: &ModelAdvice) -> Vec<String> {
    let mut texts = vec![MODEL_NOTE.to_owned()];
    for question in advice.questions().questions.values() {
        texts.push(question.instructions.clone());
        texts.extend(question.criteria.values().cloned());
    }
    texts
}

#[test]
fn the_state_is_the_one_intake_sends_for_the_same_input() {
    let program = program();
    for planning in plannings() {
        for mode in ProgramMode::ALL {
            let model = ModelAdvice::of(&planning, &program, mode, &words(), NEW_INPUT);
            let intake = planning.clone().advice(&program, mode, &words(), NEW_INPUT);
            let (sent, intake_sent) = (&model.questions().state, &intake.questions().state);
            assert_eq!(sent, intake_sent);
            assert_eq!(sent.to_string(), intake_sent.to_string());
            assert_eq!(
                sent["mode"],
                json!({"level":mode.as_str(),"description":mode.description()})
            );
        }
    }
    // It is that state whole: the level, the Program's statement, the Scope on the
    // Slice level, the earlier words, the new input and the saved draft.
    let model = asked_about(&plannings()[3], ProgramMode::Mvp);
    let state = &model.questions().state;
    assert_eq!(state["level"], "scope_to_slices");
    assert_eq!(state["program"]["name"], "Invitations");
    assert_eq!(state["scope"]["title"], "Invitations");
    assert_eq!(state["user_words"].as_array().unwrap().len(), 2);
    assert_eq!(state["new_input"], NEW_INPUT);
    assert_eq!(state["candidates"].as_array().unwrap().len(), 1);
    assert_eq!(state.as_object().unwrap().len(), 7);
}

#[test]
fn an_input_is_asked_two_questions_whatever_it_lands_on() {
    let prod = advice();
    for planning in plannings() {
        for mode in ProgramMode::ALL {
            let advice = asked_about(&planning, mode);
            let set = advice.questions();
            assert!(set.validate().is_ok());
            assert_eq!(
                set.questions.keys().collect::<Vec<_>>(),
                ["practical", "specialist"]
            );
            // The level, the draft and the product level change what Jev reads and
            // nothing of what it is asked.
            assert_eq!(set.questions, prod.questions().questions);
        }
    }
    assert_eq!(
        criteria(&prod, "practical"),
        ["ABSTAIN", "implementation", "none", "routine"]
    );
    assert_eq!(
        criteria(&prod, "specialist"),
        ["ABSTAIN", "demanding", "none", "standard"]
    );
    let questions = &prod.questions().questions;
    let described = |name: &str, criterion: &str| questions[name].criteria[criterion].as_str();
    assert_eq!(
        described("practical", "none"),
        "The input asks for no practical work."
    );
    assert_eq!(
        described("practical", "routine"),
        "Routine or tightly bounded practical work."
    );
    assert_eq!(
        described("practical", "implementation"),
        "Ordinary or complex implementation."
    );
    assert!(described("specialist", "none").contains("no bounded intellectual work of its own"));
    assert!(described("specialist", "standard").contains("standard effort"));
    assert!(described("specialist", "demanding").contains("raised effort"));
    for (name, question) in questions {
        assert!(!question.instructions.contains('{'), "{name}");
        for description in question.criteria.values() {
            assert!(!description.contains('{'), "{name}: {description}");
        }
    }
}

#[test]
fn the_wording_judges_by_the_input_the_earlier_words_and_the_level_and_rounds_up() {
    let prod = advice();
    let questions = &prod.questions().questions;
    for (name, question) in questions {
        for part in [
            "state.new_input",
            "the user's earlier words in state.user_words",
            "the product level in state.mode",
            "When two criteria fit about equally, choose the more capable one",
        ] {
            assert!(question.instructions.contains(part), "{name}: {part}");
        }
    }
    // Rounding up is wording, not a threshold: the more capable criterion of each pair.
    assert!(
        questions["practical"]
            .instructions
            .ends_with("implementation over routine, and either over none.")
    );
    assert!(
        questions["specialist"]
            .instructions
            .ends_with("demanding over standard, and either over none.")
    );
    // The intellectual work the second question means, by its kinds.
    for kind in [
        "research",
        "analysis",
        "comparison",
        "design",
        "plan",
        "substantive review",
    ] {
        assert!(
            questions["specialist"].instructions.contains(kind),
            "{kind}"
        );
    }
    assert!(
        questions["specialist"]
            .instructions
            .contains("bounded intellectual work of its own")
    );
    assert!(
        questions["practical"]
            .instructions
            .contains("the practical work")
    );
}

/// Names of models and of their effort settings: a text of the point holds none of
/// them as a word.
const MODEL_WORDS: [&str; 12] = [
    "gpt", "luna", "sol", "astra", "claude", "opus", "sonnet", "haiku", "gemini", "xhigh", "high",
    "medium",
];

#[test]
fn no_text_of_the_point_names_a_model() {
    let texts = texts(&advice());
    assert_eq!(texts.len(), 1 + 2 * (1 + 4));
    for text in &texts {
        let lowered = text.to_lowercase();
        let words = lowered
            .split(|symbol: char| !symbol.is_alphanumeric())
            .collect::<Vec<_>>();
        for name in MODEL_WORDS {
            assert!(!words.contains(&name), "{name}: {text}");
        }
        // No model, and no version of one either.
        assert!(
            !text.chars().any(|symbol| symbol.is_ascii_digit()),
            "{text}"
        );
    }
}
