//! What Jev is asked: the state object, the keys and the three kinds of questions.
mod shaping;

use super::*;
use crate::{
    AdviceCompletion, AdviceExchange, AdviceReply, AdviceUnavailableReason, Error, ProgramMode,
    ScopeCandidateDraft, SizingScope, SizingWordSource, SliceCandidateDraft,
};
use uuid::Uuid;

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

fn scope_draft(existing: Uuid) -> ScopeCandidateDraft {
    serde_json::from_value(json!({
        "boundary":"finite",
        "goals":[
            {"identity":{"local":"g1"},"text":"Invite by email","source_ref_id":Uuid::new_v4(),
             "exact_quote":"Add invitations by email.",
             "resolution":{"kind":"candidate","reference":{"local":"send"}}},
            {"identity":{"local":"g2"},"text":"Stay small","source_ref_id":Uuid::new_v4(),
             "resolution":{"kind":"candidate","reference":{"id":existing}}}
        ],
        "candidates":[
            {"identity":{"local":"send"},"title":"Send invitations","outcome":"A user is invited",
             "trigger":"t","delivered_behavior":"b","proof":"p",
             "includes":["invitation form","audit dashboard"],"excludes":["billing"],
             "dependencies":[{"id":existing}],"coverage_goals":[{"local":"g1"}]},
            {"identity":{"id":existing,"revision":2},"title":"Accept invitations",
             "outcome":"An invited user joins","trigger":"t","delivered_behavior":"b","proof":"p",
             "coverage_goals":[{"local":"g2"}]}
        ]
    }))
    .unwrap()
}

fn slice_draft(existing: Uuid) -> SliceCandidateDraft {
    serde_json::from_value(json!({
        "coverage_summary":"Whole Scope",
        "nodes":[
            {"kind":"work","identity":{"local":"form"},"title":"Invitation form",
             "outcome":"The form sends an invitation","includes":["form"],"excludes":["styling"],
             "proof":["p"],"pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"},
            {"kind":"decision","identity":{"local":"mail"},"title":"Choose the mail provider",
             "question":"Which provider sends the mail?","resolution_criteria":["c"],
             "dependencies":[{"local":"form"}]},
            {"kind":"work","identity":{"candidate_id":existing,"revision":1},"title":"Delivery",
             "outcome":"The invitation arrives","includes":["sending","retries","metrics"],
             "dependencies":[{"local":"mail"},{"candidate_id":existing,"revision":1}],
             "proof":["p"],"pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r"}
        ]
    }))
    .unwrap()
}

fn scope_statement() -> SizingScope {
    SizingScope {
        title: "Invitations".into(),
        outcome: "Users invite each other".into(),
        includes: vec!["email".into()],
        excludes: vec!["billing".into()],
    }
}

fn scope_advice() -> SizingAdvice {
    SizingDraft::scope_candidates(&scope_draft(Uuid::new_v4()))
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod)
}

fn slice_advice() -> SizingAdvice {
    SizingDraft::slice_candidates(&slice_draft(Uuid::new_v4()), scope_statement())
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod)
}

fn criteria(advice: &SizingAdvice, name: &str) -> Vec<String> {
    advice.questions().questions[name]
        .criteria
        .keys()
        .cloned()
        .collect()
}

/// An acceptable answer: `choice` at `probability`, the rest shared by the others.
fn answer(advice: &SizingAdvice, name: &str, choice: &str, probability: f64) -> AdviceAnswer {
    let asked = &advice.questions().questions[name].criteria;
    assert!(asked.contains_key(choice), "{name} does not offer {choice}");
    let rest = (1.0 - probability) / (asked.len() - 1) as f64;
    AdviceAnswer {
        choice: choice.into(),
        probabilities: asked
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
    advice: &SizingAdvice,
    choose: impl Fn(&str) -> (&'static str, f64),
) -> BTreeMap<String, AdviceAnswer> {
    advice
        .questions()
        .questions
        .keys()
        .map(|name| {
            let (choice, probability) = choose(name);
            (name.clone(), answer(advice, name, choice, probability))
        })
        .collect()
}

fn content(field: &AdviceField) -> Value {
    assert!(matches!(field, AdviceField::Advised { .. }), "{field:?}");
    json!(field)
}

/// Bytes the field costs inside a reply, where the data travels as a JSON string.
fn reply_bytes(field: &AdviceField) -> usize {
    serde_json::to_string(&json!(field).to_string())
        .unwrap()
        .len()
}

#[test]
fn scope_draft_becomes_one_state_object_and_three_kinds_of_questions() {
    let advice = scope_advice();
    let set = advice.questions();
    assert!(set.validate().is_ok());
    assert_eq!(
        set.state,
        json!({
            "level":"program_to_scopes",
            "mode":{"level":"prod","description":
                "A product in operation that fits on one machine or on one machine per service."},
            "user_words":[
                {"source":"program_input","text":"Add invitations by email."},
                {"source":"scope_candidate_input","text":"Keep it small."}
            ],
            "goals":[
                {"text":"Invite by email","quote":"Add invitations by email."},
                {"text":"Stay small"}
            ],
            "candidates":[
                {"key":"c1","title":"Send invitations","outcome":"A user is invited",
                 "includes":[{"key":"c1.i1","text":"invitation form"},
                     {"key":"c1.i2","text":"audit dashboard"}],
                 "excludes":["billing"],"dependencies":["c2"]},
                {"key":"c2","title":"Accept invitations","outcome":"An invited user joins",
                 "includes":[],"excludes":[],"dependencies":[]}
            ]
        })
    );
    assert_eq!(
        set.questions.keys().collect::<Vec<_>>(),
        [
            "c1.advice",
            "c1.i1",
            "c1.i2",
            "c1.size",
            "c2.advice",
            "c2.size"
        ]
    );
    assert_eq!(
        criteria(&advice, "c1.size"),
        ["ABSTAIN", "bounded", "large", "oversized", "small"]
    );
    assert_eq!(
        criteria(&advice, "c2.advice"),
        ["ABSTAIN", "clarify", "keep", "simplify", "split"]
    );
    assert_eq!(
        criteria(&advice, "c1.i2"),
        ["ABSTAIN", "enabler", "extra", "requested"]
    );
    let item = &set.questions["c1.i2"].instructions;
    assert!(item.contains("item c1.i2 of candidate c1 in state.candidates"));
    for (name, question) in &set.questions {
        let key = name.split('.').next().unwrap();
        assert!(question.instructions.contains(&format!("candidate {key} ")));
        assert!(!question.instructions.contains('{'), "{name}");
    }
}

#[test]
fn slice_draft_asks_about_work_nodes_and_alone_offers_a_separate_scope() {
    let advice = slice_advice();
    let set = advice.questions();
    assert!(set.validate().is_ok());
    assert_eq!(set.state["level"], "scope_to_slices");
    assert_eq!(
        set.state["scope"],
        json!({"title":"Invitations","outcome":"Users invite each other",
            "includes":["email"],"excludes":["billing"]})
    );
    assert_eq!(set.state["goals"], json!([]));
    assert_eq!(set.state["mode"]["level"], "prod");
    assert_eq!(set.state["user_words"].as_array().unwrap().len(), 2);
    assert_eq!(
        set.state["candidates"],
        json!([
            {"key":"c1","kind":"work","title":"Invitation form",
             "outcome":"The form sends an invitation",
             "includes":[{"key":"c1.i1","text":"form"}],"excludes":["styling"],
             "dependencies":[]},
            {"key":"c2","kind":"decision","title":"Choose the mail provider",
             "question":"Which provider sends the mail?","dependencies":["c1"]},
            {"key":"c3","kind":"work","title":"Delivery","outcome":"The invitation arrives",
             "includes":[{"key":"c3.i1","text":"sending"},{"key":"c3.i2","text":"retries"},
                 {"key":"c3.i3","text":"metrics"}],
             "excludes":[],"dependencies":["c2","c3"]}
        ])
    );
    // The decision node is shown and not asked about.
    assert_eq!(
        set.questions.keys().collect::<Vec<_>>(),
        [
            "c1.advice",
            "c1.i1",
            "c1.size",
            "c3.advice",
            "c3.i1",
            "c3.i2",
            "c3.i3",
            "c3.size"
        ]
    );
    assert_eq!(
        criteria(&advice, "c3.advice"),
        [
            "ABSTAIN",
            "clarify",
            "keep",
            "separate_scope",
            "simplify",
            "split"
        ]
    );
    assert!(!criteria(&scope_advice(), "c1.advice").contains(&"separate_scope".to_owned()));
    assert!(
        set.questions["c3.advice"].criteria["separate_scope"].contains("state.scope"),
        "the criterion names the Scope it compares with"
    );
}

#[test]
fn every_request_carries_the_product_level_and_the_wording_judges_against_it() {
    let draft = || {
        SizingDraft::scope_candidates(&scope_draft(Uuid::new_v4()))
            .unwrap()
            .unwrap()
    };
    let prod = draft().advice(&words(), ProgramMode::Prod);
    for mode in ProgramMode::ALL {
        let advice = draft().advice(&words(), mode);
        assert_eq!(
            advice.questions().state["mode"],
            json!({"level":mode.as_str(),"description":mode.description()})
        );
        // The level changes what Jev reads and nothing of what it is asked.
        assert_eq!(advice.questions().questions, prod.questions().questions);
    }
    let asked = &prod.questions().questions;
    for text in [
        &asked["c1.size"].instructions,
        &asked["c1.advice"].criteria["simplify"],
        &asked["c1.i1"].instructions,
        &asked["c1.i1"].criteria["extra"],
    ] {
        assert!(text.contains("the product level in state.mode"), "{text}");
    }
    assert!(asked["c1.size"].instructions.contains("state.user_words"));
    assert!(asked["c1.i1"].instructions.contains("state.user_words"));
    assert_eq!(
        asked["c1.i1"].criteria["extra"],
        "Neither requested nor needed for the requested work at the product level in state.mode."
    );
}

#[test]
fn keys_follow_draft_positions_and_nothing_else() {
    let existing = Uuid::new_v4();
    let first = SizingDraft::scope_candidates(&scope_draft(existing))
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod);
    let mut reworded = scope_draft(existing);
    reworded.candidates[0].title = "Another title".into();
    reworded.candidates[0].identity.local = Some("renamed".into());
    reworded.candidates[0].includes[1] = "another item".into();
    let second = SizingDraft::scope_candidates(&reworded)
        .unwrap()
        .unwrap()
        .advice(&[], ProgramMode::Prod);
    assert_eq!(
        first.questions().questions.keys().collect::<Vec<_>>(),
        second.questions().questions.keys().collect::<Vec<_>>()
    );
    assert_eq!(second.questions().state["user_words"], json!([]));
    assert_eq!(
        second.questions().state["candidates"][0]["includes"][1],
        json!({"key":"c1.i2","text":"another item"})
    );
    assert_eq!(first, scope_advice_of(existing));
}

fn scope_advice_of(existing: Uuid) -> SizingAdvice {
    SizingDraft::scope_candidates(&scope_draft(existing))
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod)
}

#[test]
fn drafts_without_a_candidate_to_ask_about_give_no_advice_point() {
    let mut empty = scope_draft(Uuid::new_v4());
    empty.candidates.clear();
    assert_eq!(SizingDraft::scope_candidates(&empty), Ok(None));
    let mut decisions = slice_draft(Uuid::new_v4());
    decisions.nodes = serde_json::from_value(json!([{"kind":"decision",
        "identity":{"local":"mail"},"title":"t","question":"q","resolution_criteria":["c"]}]))
    .unwrap();
    assert_eq!(
        SizingDraft::slice_candidates(&decisions, scope_statement()),
        Ok(None)
    );
}

#[test]
fn a_dependency_outside_the_draft_is_refused() {
    let mut scope = scope_draft(Uuid::new_v4());
    scope.candidates[0].dependencies[0].id = Some(Uuid::new_v4());
    assert_eq!(
        SizingDraft::scope_candidates(&scope),
        Err(Error::InvalidArguments)
    );
    let mut slices = slice_draft(Uuid::new_v4());
    slices.nodes.remove(0);
    assert_eq!(
        SizingDraft::slice_candidates(&slices, scope_statement()),
        Err(Error::InvalidArguments)
    );
}
