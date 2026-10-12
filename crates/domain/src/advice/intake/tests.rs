//! What Jev is asked about a recorded input: the state object and the four questions,
//! by level and by what the saved draft offers.
mod shaping;

use super::*;
use crate::{
    AdviceCompletion, AdviceExchange, AdviceReply, AdviceUnavailableReason, SizingWordSource,
};
use uuid::Uuid;

const NEW_INPUT: &str = "Also let an admin revoke an invitation.";

fn program() -> Program {
    let mut program = Program::draft(Uuid::new_v4(), Uuid::new_v4(), 0);
    program.name = Some("Invitations".into());
    program.intent = Some("Let users invite each other".into());
    program.basis = Some("Teams ask for it".into());
    program.boundaries = Some("Email invitations only".into());
    program.constraints = Some("No billing changes".into());
    program.success = Some("An invited user joins".into());
    program.working_notes = Some("Notes that are not part of the statement".into());
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

/// A stored Scope candidate draft of `count` candidates, each depending on the next.
fn scope_draft(count: usize) -> ResolvedCandidateDraft {
    let ids = (0..count).map(|_| Uuid::new_v4()).collect::<Vec<_>>();
    let candidates = (0..count)
        .map(|index| {
            json!({"id":ids[index],"revision":1,"title":format!("Scope {}", index + 1),
                "outcome":"A user is invited","trigger":"t","delivered_behavior":"b","proof":"p",
                "includes":["invitation form","audit dashboard"],"excludes":["billing"],
                "dependencies":ids.get(index + 1).into_iter().collect::<Vec<_>>(),
                "coverage_goal_ids":[],"evidence_ids":[]})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"boundary":"finite","goals":[],"evidence":[],
        "candidates":candidates,"blockers":[],"protected_changes":[]}))
    .unwrap()
}

/// A stored Slice candidate draft: a work node, a decision node and a work node.
fn slice_draft() -> ResolvedSliceCandidateDraft {
    let (form, mail, delivery) = (Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4());
    let work = |id: Uuid, title: &str, includes: Value, dependencies: Value| {
        json!({"kind":"work","id":id,"revision":1,"title":title,"outcome":"The invitation arrives",
            "includes":includes,"excludes":["styling"],"dependencies":dependencies,"proof":["p"],
            "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r",
            "why_lightweight_insufficient":null,"why_further_vertical_split_not_viable":null,
            "source_result_ids":[]})
    };
    serde_json::from_value(
        json!({"coverage_summary":"Whole Scope","supersessions":[],"nodes":[
            work(form, "Invitation form", json!(["form"]), json!([])),
            {"kind":"decision","id":mail,"revision":1,"title":"Choose the mail provider",
             "question":"Which provider sends the mail?","resolution_criteria":["c"],
             "dependencies":[form],"source_result_ids":[]},
            work(delivery, "Delivery", json!(["sending","retries","metrics"]), json!([mail]))
        ]}),
    )
    .unwrap()
}

/// A stored Slice candidate draft that holds nothing but one decision node.
fn decisions_only() -> ResolvedSliceCandidateDraft {
    let mut draft = slice_draft();
    draft.nodes.remove(2);
    draft.nodes.remove(0);
    let crate::SliceCandidateNode::Decision { dependencies, .. } = &mut draft.nodes[0] else {
        panic!("the node left is the decision")
    };
    dependencies.clear();
    draft
}

fn scope_statement() -> SizingScope {
    SizingScope {
        title: "Invitations".into(),
        outcome: "Users invite each other".into(),
        includes: vec!["email".into()],
        excludes: vec!["billing".into()],
    }
}

fn scope_advice(saved: Option<&ResolvedCandidateDraft>) -> IntakeAdvice {
    IntakePlanning::scope_candidates(saved).unwrap().advice(
        &program(),
        ProgramMode::Prod,
        &words(),
        NEW_INPUT,
    )
}

fn slice_advice(saved: Option<&ResolvedSliceCandidateDraft>) -> IntakeAdvice {
    IntakePlanning::slice_candidates(saved, scope_statement())
        .unwrap()
        .advice(&program(), ProgramMode::Prod, &words(), NEW_INPUT)
}

fn asked(advice: &IntakeAdvice) -> Vec<&str> {
    let names = advice.questions().questions.keys();
    names.map(String::as_str).collect()
}

/// The criteria of one question, `ABSTAIN` included, in name order.
fn criteria(advice: &IntakeAdvice, name: &str) -> Vec<String> {
    let question = &advice.questions().questions[name];
    question.criteria.keys().cloned().collect()
}

/// An acceptable answer: `choice` at `probability`, the rest shared by the others.
fn answer(advice: &IntakeAdvice, name: &str, choice: &str, probability: f64) -> AdviceAnswer {
    let offered = &advice.questions().questions[name].criteria;
    assert!(
        offered.contains_key(choice),
        "{name} does not offer {choice}"
    );
    let rest = (1.0 - probability) / (offered.len() - 1) as f64;
    AdviceAnswer {
        choice: choice.into(),
        probabilities: offered
            .keys()
            .map(|criterion| {
                let share = if criterion == choice {
                    probability
                } else {
                    rest
                };
                (criterion.clone(), share)
            })
            .collect(),
        confidence: Some(0.4),
    }
}

/// Every question answered by `choose`, which returns the choice and its probability.
fn answers(
    advice: &IntakeAdvice,
    choose: impl Fn(&str) -> (&'static str, f64),
) -> BTreeMap<String, AdviceAnswer> {
    asked(advice)
        .into_iter()
        .map(|name| {
            let (choice, probability) = choose(name);
            (name.to_owned(), answer(advice, name, choice, probability))
        })
        .collect()
}

const WORK_OPTIONS: [&str; 6] = ["ABSTAIN", "bounded", "large", "none", "oversized", "small"];
const FIT_OPTIONS: [&str; 5] = [
    "ABSTAIN",
    "beyond_mode",
    "contradicts",
    "in_line",
    "unclear",
];

#[test]
fn scope_input_becomes_one_state_object_and_four_questions() {
    let draft = scope_draft(2);
    let advice = scope_advice(Some(&draft));
    let set = advice.questions();
    assert!(set.validate().is_ok());
    assert_eq!(
        set.state,
        json!({
            "level":"program_to_scopes",
            "mode":{"level":"prod","description":
                "A product in operation that fits on one machine or on one machine per service."},
            "program":{"name":"Invitations","intent":"Let users invite each other",
                "basis":"Teams ask for it","boundaries":"Email invitations only",
                "constraints":"No billing changes","success":"An invited user joins"},
            "user_words":[
                {"source":"program_input","text":"Add invitations by email."},
                {"source":"scope_candidate_input","text":"Keep it small."}
            ],
            "new_input":NEW_INPUT,
            "candidates":[
                {"key":"c1","title":"Scope 1","outcome":"A user is invited",
                 "includes":[{"key":"c1.i1","text":"invitation form"},
                     {"key":"c1.i2","text":"audit dashboard"}],
                 "excludes":["billing"],"dependencies":["c2"]},
                {"key":"c2","title":"Scope 2","outcome":"A user is invited",
                 "includes":[{"key":"c2.i1","text":"invitation form"},
                     {"key":"c2.i2","text":"audit dashboard"}],
                 "excludes":["billing"],"dependencies":[]}
            ]
        })
    );
    assert_eq!(asked(&advice), ["fit", "nearest", "placement", "work"]);
    assert_eq!(criteria(&advice, "work"), WORK_OPTIONS);
    assert_eq!(
        criteria(&advice, "placement"),
        [
            "ABSTAIN",
            "covered",
            "extends",
            "new_candidate",
            "outside_program"
        ]
    );
    assert_eq!(
        criteria(&advice, "nearest"),
        ["ABSTAIN", "c1", "c2", "none"]
    );
    assert_eq!(criteria(&advice, "fit"), FIT_OPTIONS);
    assert_eq!(
        set.questions["nearest"].criteria["c2"],
        "Candidate c2 in state.candidates is the closest."
    );
    for (name, question) in &set.questions {
        assert!(!question.instructions.contains('{'), "{name}");
        assert!(question.instructions.contains("state.new_input"), "{name}");
        for description in question.criteria.values() {
            assert!(!description.contains('{'), "{name}: {description}");
        }
    }
}

#[test]
fn slice_input_shows_decision_nodes_and_never_offers_them() {
    let draft = slice_draft();
    let advice = slice_advice(Some(&draft));
    let set = advice.questions();
    assert!(set.validate().is_ok());
    assert_eq!(set.state["level"], "scope_to_slices");
    assert_eq!(
        set.state["scope"],
        json!({"title":"Invitations","outcome":"Users invite each other",
            "includes":["email"],"excludes":["billing"]})
    );
    assert_eq!(set.state["new_input"], NEW_INPUT);
    assert_eq!(set.state["user_words"].as_array().unwrap().len(), 2);
    assert_eq!(
        set.state["candidates"],
        json!([
            {"key":"c1","kind":"work","title":"Invitation form",
             "outcome":"The invitation arrives",
             "includes":[{"key":"c1.i1","text":"form"}],"excludes":["styling"],
             "dependencies":[]},
            {"key":"c2","kind":"decision","title":"Choose the mail provider",
             "question":"Which provider sends the mail?","dependencies":["c1"]},
            {"key":"c3","kind":"work","title":"Delivery","outcome":"The invitation arrives",
             "includes":[{"key":"c3.i1","text":"sending"},{"key":"c3.i2","text":"retries"},
                 {"key":"c3.i3","text":"metrics"}],
             "excludes":["styling"],"dependencies":["c2"]}
        ])
    );
    assert_eq!(asked(&advice), ["fit", "nearest", "placement", "work"]);
    // The decision node keeps its position and is not a criterion.
    assert_eq!(
        criteria(&advice, "nearest"),
        ["ABSTAIN", "c1", "c3", "none"]
    );
    assert_eq!(
        criteria(&advice, "placement"),
        [
            "ABSTAIN",
            "covered",
            "extends",
            "new_candidate",
            "outside_program",
            "separate_scope"
        ]
    );
    assert_eq!(criteria(&advice, "work"), WORK_OPTIONS);
    assert_eq!(criteria(&advice, "fit"), FIT_OPTIONS);
    assert!(
        set.questions["placement"].criteria["separate_scope"].contains("state.scope"),
        "the criterion names the Scope it compares with"
    );
    let draft = scope_draft(1);
    assert!(
        scope_advice(Some(&draft))
            .questions()
            .state
            .get("scope")
            .is_none()
    );
}

#[test]
fn without_a_candidate_to_name_nearest_is_not_asked_and_placement_is_narrower() {
    let (empty, decisions) = (scope_draft(0), decisions_only());
    let cases = [
        (scope_advice(None), None, false),
        (scope_advice(Some(&empty)), Some(json!([])), false),
        (slice_advice(None), None, true),
        (
            slice_advice(Some(&decisions)),
            Some(
                json!([{"key":"c1","kind":"decision","title":"Choose the mail provider",
                "question":"Which provider sends the mail?","dependencies":[]}]),
            ),
            true,
        ),
    ];
    for (advice, candidates, slices) in cases {
        let set = advice.questions();
        assert!(set.validate().is_ok());
        // `candidates` is there exactly when the set has a saved draft.
        assert_eq!(set.state.get("candidates"), candidates.as_ref());
        assert_eq!(asked(&advice), ["fit", "placement", "work"]);
        let mut placement = vec!["ABSTAIN", "new_candidate", "outside_program"];
        if slices {
            placement.push("separate_scope");
        }
        assert_eq!(criteria(&advice, "placement"), placement);
        assert_eq!(criteria(&advice, "work"), WORK_OPTIONS);
        assert_eq!(criteria(&advice, "fit"), FIT_OPTIONS);
        assert_eq!(set.state["new_input"], NEW_INPUT);
        assert_eq!(set.state["program"]["name"], "Invitations");
    }
}

#[test]
fn every_request_carries_the_product_level_and_the_wording_judges_against_it() {
    let draft = scope_draft(2);
    let planning = || IntakePlanning::scope_candidates(Some(&draft)).unwrap();
    let prod = scope_advice(Some(&draft));
    for mode in ProgramMode::ALL {
        let advice = planning().advice(&program(), mode, &words(), NEW_INPUT);
        assert_eq!(
            advice.questions().state["mode"],
            json!({"level":mode.as_str(),"description":mode.description()})
        );
        // The level changes what Jev reads and nothing of what it is asked.
        assert_eq!(advice.questions().questions, prod.questions().questions);
    }
    let questions = &prod.questions().questions;
    for text in [
        &questions["work"].instructions,
        &questions["fit"].instructions,
        &questions["fit"].criteria["in_line"],
        &questions["fit"].criteria["beyond_mode"],
    ] {
        assert!(text.contains("the product level in state.mode"), "{text}");
    }
    for name in ["work", "fit"] {
        let instructions = &questions[name].instructions;
        assert!(instructions.contains("state.user_words"), "{name}");
        assert!(instructions.contains("state.program"), "{name}");
    }
    assert!(questions["fit"].criteria["contradicts"].contains("state.user_words"));
    // The four sizes are the texts `sizing` asks with, not copies of them.
    for (size, description) in super::super::sizing_catalog::SIZE_CRITERIA {
        assert_eq!(questions["work"].criteria[size], description);
    }
}

#[test]
fn the_statement_is_the_six_fields_of_the_program_as_they_stand() {
    let mut partial = program();
    partial.constraints = None;
    let advice = IntakePlanning::scope_candidates(None).unwrap().advice(
        &partial,
        ProgramMode::Demo,
        &[],
        NEW_INPUT,
    );
    let state = &advice.questions().state;
    assert_eq!(
        state["program"],
        json!({"name":"Invitations","intent":"Let users invite each other",
            "basis":"Teams ask for it","boundaries":"Email invitations only",
            "constraints":null,"success":"An invited user joins"})
    );
    assert_eq!(state["user_words"], json!([]));
    assert!(!state.to_string().contains("Notes that are not part"));
}

#[test]
fn a_stored_draft_with_a_dependency_outside_it_is_an_internal_error() {
    let mut scope = scope_draft(2);
    scope.candidates[0].dependencies[0] = Uuid::new_v4();
    assert_eq!(
        IntakePlanning::scope_candidates(Some(&scope)),
        Err(Error::InternalInvariant)
    );
    let mut slices = slice_draft();
    slices.nodes.remove(0);
    assert_eq!(
        IntakePlanning::slice_candidates(Some(&slices), scope_statement()),
        Err(Error::InternalInvariant)
    );
}
