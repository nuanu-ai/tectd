//! What Jev is asked about the pipelines of a saved draft: which nodes, the state
//! object and the criteria of the one question per node.
mod kept;
mod shaping;

use super::super::test_drafts::stored_slices as stored;
use super::*;
use crate::{
    AdviceCompletion, AdviceExchange, AdviceReply, AdviceUnavailableReason, SizingWordSource,
};
use uuid::Uuid;

const LIGHTWEIGHT: &str = "slice.lightweight-tdd-development";
const FULL: &str = "slice.full-design-to-execution";
const DEBUG: &str = "slice.debug-root-cause";
const RESEARCH: &str = "slice.research";

fn words() -> Vec<SizingWord> {
    vec![SizingWord {
        source: SizingWordSource::ProgramInput,
        text: "Add invitations by email.".into(),
    }]
}

fn scope_statement() -> SizingScope {
    SizingScope {
        title: "Invitations".into(),
        outcome: "Users invite each other".into(),
        includes: vec!["email".into()],
        excludes: vec!["billing".into()],
    }
}

/// The catalogue a current snapshot holds: nine pipelines, each with texts of its own.
fn catalogue() -> PipelineCatalogueSnapshot {
    let entries = PipelineKind::CURRENT_SLICE_RUN_KINDS
        .into_iter()
        .chain([PipelineKind::PromoteToDurableKnowledge])
        .map(|kind| {
            json!({"kind":kind,"description":"d","implementation_status":"executable",
                "description_status":"refined","refinement_required":false,
                "choose_when":format!("Fits {kind:?}."),
                "do_not_choose_when":format!("Misfits {kind:?}."),
                "expected_result":format!("Gives {kind:?}.")})
        })
        .collect::<Vec<_>>();
    serde_json::from_value(json!({"revision":"4","digest":"d","entries":entries})).unwrap()
}

/// The ids a second save names: a work node it keeps, a work node it changes and a
/// decision node it changes.
struct Existing {
    kept: Uuid,
    changed: Uuid,
    decision: Uuid,
}

impl Existing {
    fn new() -> Self {
        Self {
            kept: Uuid::new_v4(),
            changed: Uuid::new_v4(),
            decision: Uuid::new_v4(),
        }
    }
}

fn work(identity: Value, title: &str, pipeline: &str, reason: &str) -> Value {
    json!({"kind":"work","identity":identity,"title":title,"outcome":"The invitation arrives",
        "includes":["sending"],"proof":["p"],"pipeline":pipeline,"pipeline_reason":reason})
}

/// A second draft of six nodes: a new work node, a new decision node, a work node
/// kept as it was, a work node changed, a decision node changed and a new work node.
fn draft(existing: &Existing) -> SliceCandidateDraft {
    let mut full = work(
        json!({"local":"retry"}),
        "Retries",
        FULL,
        "agent reason three",
    );
    full["why_lightweight_insufficient"] = json!("agent reason four");
    full["why_further_vertical_split_not_viable"] = json!("agent reason five");
    let mut changed = work(
        json!({"candidate_id":existing.changed,"revision":2}),
        "Delivery",
        DEBUG,
        "agent reason two",
    );
    changed["change_rationale"] = json!("The cause is not known after all");
    serde_json::from_value(json!({"coverage_summary":"Whole Scope","nodes":[
        work(json!({"local":"form"}), "Invitation form", LIGHTWEIGHT, "agent reason one"),
        {"kind":"decision","identity":{"local":"mail"},"title":"Choose the mail provider",
         "question":"Which provider sends the mail?","resolution_criteria":["c"],
         "dependencies":[{"local":"form"}]},
        work(json!({"candidate_id":existing.kept,"revision":1}), "Accept", RESEARCH,
            "agent reason kept"),
        changed,
        {"kind":"decision","identity":{"candidate_id":existing.decision,"revision":1},
         "change_rationale":"Reworded","title":"Choose the queue","question":"Which queue?",
         "resolution_criteria":["c"]},
        full
    ]}))
    .unwrap()
}

/// What the save of `draft(existing)` stored: new nodes under revision 1, the kept
/// node under the revision named, the two changed nodes under the next one.
fn saved(existing: &Existing) -> ResolvedSliceCandidateDraft {
    stored(&[
        (Uuid::new_v4(), 1),
        (Uuid::new_v4(), 1),
        (existing.kept, 1),
        (existing.changed, 3),
        (existing.decision, 2),
        (Uuid::new_v4(), 1),
    ])
}

fn prepared() -> PipelineDraft {
    let existing = Existing::new();
    PipelineDraft::slice_candidates(
        &draft(&existing),
        &saved(&existing),
        &catalogue(),
        scope_statement(),
    )
    .unwrap()
    .unwrap()
}

fn advice() -> PipelineAdvice {
    prepared().advice(&words(), ProgramMode::Prod)
}

fn criteria(advice: &PipelineAdvice, name: &str) -> Vec<String> {
    advice.questions().questions[name]
        .criteria
        .keys()
        .cloned()
        .collect()
}

/// An acceptable answer: `choice` at `probability`, `other` at `share` when it is
/// another criterion, and the rest shared by the remaining criteria.
fn answer(
    advice: &PipelineAdvice,
    name: &str,
    (choice, probability): (&str, f64),
    (other, share): (&str, f64),
) -> AdviceAnswer {
    let asked = &advice.questions().questions[name].criteria;
    assert!(asked.contains_key(choice), "{name} does not offer {choice}");
    assert!(asked.contains_key(other), "{name} does not offer {other}");
    let named = if other == choice { 1 } else { 2 };
    let given = if other == choice {
        probability
    } else {
        probability + share
    };
    let rest = (1.0 - given) / (asked.len() - named) as f64;
    AdviceAnswer {
        choice: choice.into(),
        probabilities: asked
            .keys()
            .map(|criterion| {
                let part = match criterion.as_str() {
                    criterion if criterion == choice => probability,
                    criterion if criterion == other => share,
                    _ => rest,
                };
                (criterion.clone(), part)
            })
            .collect(),
        confidence: Some(0.4),
    }
}

/// Every question answered by `choose`: the choice with its probability, and another
/// criterion with the probability it is given.
fn answers(
    advice: &PipelineAdvice,
    choose: impl Fn(&str) -> ((&'static str, f64), (&'static str, f64)),
) -> BTreeMap<String, AdviceAnswer> {
    advice
        .questions()
        .questions
        .keys()
        .map(|name| {
            let (chosen, other) = choose(name);
            (name.clone(), answer(advice, name, chosen, other))
        })
        .collect()
}

#[test]
fn only_the_work_nodes_the_save_created_or_changed_are_asked_about() {
    let advice = advice();
    let set = advice.questions();
    assert!(set.validate().is_ok());
    // Positions count every node of the draft: the kept work node is 3 and the two
    // decision nodes are 2 and 5.
    assert_eq!(
        set.questions.keys().collect::<Vec<_>>(),
        ["c1.pipeline", "c4.pipeline", "c6.pipeline"]
    );
    for (name, question) in &set.questions {
        let key = name.split('.').next().unwrap();
        assert!(
            question
                .instructions
                .contains(&format!("candidate {key} in state.candidates")),
            "{name}"
        );
        assert!(!question.instructions.contains('{'), "{name}");
    }

    // A draft that keeps every node it names asks nothing.
    let existing = Existing::new();
    let mut kept = draft(&existing);
    kept.nodes = vec![kept.nodes[2].clone()];
    let unchanged = stored(&[(existing.kept, 1)]);
    let ask = |draft: &SliceCandidateDraft, stored: &ResolvedSliceCandidateDraft| {
        PipelineDraft::slice_candidates(draft, stored, &catalogue(), scope_statement())
    };
    assert_eq!(ask(&kept, &unchanged), Ok(None));
    // The same node stored under the next revision was changed, and is asked about.
    let rewritten = ask(&kept, &stored(&[(existing.kept, 2)]))
        .unwrap()
        .unwrap()
        .advice(&[], ProgramMode::Prod);
    assert_eq!(
        rewritten.questions().questions.keys().collect::<Vec<_>>(),
        ["c1.pipeline"]
    );

    // Decision nodes are never asked about, new or changed; nor is a kept work node
    // beside them.
    let mut decisions = draft(&existing);
    decisions.nodes = vec![
        decisions.nodes[1].clone(),
        decisions.nodes[2].clone(),
        decisions.nodes[4].clone(),
    ];
    decisions.nodes[0] = serde_json::from_value(json!({"kind":"decision",
        "identity":{"local":"mail"},"title":"t","question":"q","resolution_criteria":["c"]}))
    .unwrap();
    let after = stored(&[
        (Uuid::new_v4(), 1),
        (existing.kept, 1),
        (existing.decision, 2),
    ]);
    assert_eq!(ask(&decisions, &after), Ok(None));
    // A draft of decision nodes alone has no work node at all.
    decisions.nodes.remove(1);
    let after = stored(&[(Uuid::new_v4(), 1), (existing.decision, 2)]);
    assert_eq!(ask(&decisions, &after), Ok(None));
}

#[test]
fn the_criteria_are_the_pipelines_of_the_catalogue_and_abstain() {
    let advice = advice();
    let catalogue = catalogue();
    let mut names = catalogue
        .entries
        .iter()
        .map(|entry| entry.kind.as_str().to_owned())
        .chain([ADVICE_ABSTAIN.to_owned()])
        .collect::<Vec<_>>();
    names.sort();
    assert_eq!(names.len(), 10);
    let asked = &advice.questions().questions;
    for name in ["c1.pipeline", "c4.pipeline", "c6.pipeline"] {
        assert_eq!(criteria(&advice, name), names, "{name}");
        // Each pipeline is described with the three texts of its own entry.
        for entry in &catalogue.entries {
            assert_eq!(
                asked[name].criteria[entry.kind.as_str()],
                format!(
                    "Choose when: {} Do not choose when: {} Expected result: {}",
                    entry.choose_when, entry.do_not_choose_when, entry.expected_result
                )
            );
        }
        assert_eq!(
            asked[name].criteria[ADVICE_ABSTAIN],
            "The state does not say enough to choose a pipeline."
        );
    }
    // Whatever the draft chose for a node, Jev is offered the same for each.
    assert_eq!(asked["c1.pipeline"].criteria, asked["c4.pipeline"].criteria);
    assert_eq!(asked["c1.pipeline"].criteria, asked["c6.pipeline"].criteria);
    // The wording judges against the user's words and the candidate's product level.
    let instructions = &asked["c4.pipeline"].instructions;
    assert!(instructions.contains("state.user_words"), "{instructions}");
    assert!(
        instructions.contains(
            "the candidate's product level: its own mode_override when it carries one, \
             else state.mode"
        ),
        "{instructions}"
    );

    // A catalogue of another snapshot gives its own criteria.
    let existing = Existing::new();
    let mut historical = catalogue.clone();
    historical
        .entries
        .retain(|entry| entry.kind != PipelineKind::DeepBrainstorming);
    let fewer = PipelineDraft::slice_candidates(
        &draft(&existing),
        &saved(&existing),
        &historical,
        scope_statement(),
    )
    .unwrap()
    .unwrap()
    .advice(&[], ProgramMode::Prod);
    let offered = criteria(&fewer, "c1.pipeline");
    assert_eq!(offered.len(), 9);
    assert!(!offered.contains(&"slice.deep-brainstorming".to_owned()));
}

#[test]
fn the_state_is_what_sizing_sends_and_holds_nothing_of_the_agents_choice() {
    let existing = Existing::new();
    let draft = draft(&existing);
    let sizing = SizingDraft::slice_candidates(&draft, &saved(&existing), scope_statement())
        .unwrap()
        .unwrap();
    for mode in ProgramMode::ALL {
        let advice = PipelineDraft::slice_candidates(
            &draft,
            &saved(&existing),
            &catalogue(),
            scope_statement(),
        )
        .unwrap()
        .unwrap()
        .advice(&words(), mode);
        let state = &advice.questions().state;
        assert_eq!(
            state,
            &sizing.clone().advice(&words(), mode).questions().state
        );
        assert_eq!(state["level"], "scope_to_slices");
        assert_eq!(
            state["mode"],
            json!({"level":mode.as_str(),"description":mode.description()})
        );
        // Every node of the draft is shown, asked about or not.
        assert_eq!(state["candidates"].as_array().unwrap().len(), 6);
        assert_eq!(state["candidates"][2]["title"], "Accept");
        assert_eq!(state["candidates"][1]["kind"], "decision");
        // No pipeline name, no reason for one and no key that would hold either.
        let text = state.to_string();
        for absent in [
            "pipeline",
            "slice.",
            "agent reason",
            "why_",
            "change_rationale",
        ] {
            assert!(!text.contains(absent), "{absent}: {text}");
        }
    }
    // Nor do the questions say which pipeline the draft chose: no reason travels.
    let request = json!(advice().questions()).to_string();
    assert!(!request.contains("agent reason"), "{request}");
}

#[test]
fn a_draft_the_save_cannot_have_stored_is_refused() {
    let existing = Existing::new();
    let ask = |stored: &ResolvedSliceCandidateDraft, catalogue: &PipelineCatalogueSnapshot| {
        PipelineDraft::slice_candidates(&draft(&existing), stored, catalogue, scope_statement())
    };
    // The stored draft lacks a node the draft names by id.
    let mut lacking = saved(&existing);
    lacking.nodes.remove(3);
    assert_eq!(
        ask(&lacking, &catalogue()).unwrap_err(),
        Error::InternalInvariant
    );
    // A written work node names a pipeline the catalogue does not hold.
    let mut narrow = catalogue();
    narrow
        .entries
        .retain(|entry| entry.kind != PipelineKind::DebugRootCause);
    assert_eq!(
        ask(&saved(&existing), &narrow).unwrap_err(),
        Error::InternalInvariant
    );
    // The pipeline of a kept node is not asked about and need not be offered.
    let mut without_research = catalogue();
    without_research
        .entries
        .retain(|entry| entry.kind != PipelineKind::Research);
    assert!(ask(&saved(&existing), &without_research).is_ok());
}
