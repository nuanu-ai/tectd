use super::*;
use serde_json::json;

const WORDS: &str = "Fine. I allow demo for the export Scope, nothing else.";

fn scope_draft(mode: &str, quote: &str) -> ScopeModeOverrideDraft {
    ScopeModeOverrideDraft {
        mode: mode.into(),
        source_ref_id: Uuid::from_u128(7),
        exact_quote: quote.into(),
    }
}

fn slice_draft(mode: &str, quote: &str) -> SliceModeOverrideDraft {
    SliceModeOverrideDraft {
        mode: mode.into(),
        input_id: Uuid::from_u128(9),
        exact_quote: quote.into(),
    }
}

/// The refusal a check ended with: its rule, the part it points at and what it found.
fn refusal<T: std::fmt::Debug>(result: Result<T>) -> (String, String, String) {
    let error = result.unwrap_err();
    assert_eq!(error.code(), "MODE_OVERRIDE_NOT_PERMITTED");
    let refusal = error.refusal().unwrap();
    assert_eq!(refusal.code, RefusalCode::ModeOverrideNotPermitted);
    assert_eq!(
        refusal.message.as_deref(),
        Some("a mode override is recorded only with the user's explicit written permission")
    );
    assert_eq!(refusal.required.as_deref(), Some("user_permission_quote"));
    (
        refusal.rule.unwrap(),
        refusal.path.unwrap(),
        refusal.actual.unwrap(),
    )
}

#[test]
fn the_effective_level_is_the_own_override_then_the_scopes_then_the_programs() {
    use ProgramMode::{Demo, Mvp, Prod, SeriousProd};
    assert_eq!(effective_mode(None, None, Prod), Prod);
    assert_eq!(effective_mode(None, Some(Mvp), Prod), Mvp);
    assert_eq!(effective_mode(Some(Demo), Some(Mvp), Prod), Demo);
    assert_eq!(effective_mode(Some(SeriousProd), None, Prod), SeriousProd);
    // An override may also restate the level it would have inherited.
    assert_eq!(effective_mode(Some(Prod), Some(Demo), Mvp), Prod);
}

#[test]
fn a_quote_that_stands_in_the_users_input_permits_the_override() {
    let kept = scope_draft("demo", "I allow demo for the export Scope")
        .permitted(0, PermissionRecord::UserInput(WORDS))
        .unwrap();
    assert_eq!(
        kept,
        ScopeCandidateModeOverride {
            mode: ProgramMode::Demo,
            source_ref_id: Uuid::from_u128(7),
            exact_quote: "I allow demo for the export Scope".into(),
        }
    );
    assert_eq!(
        json!(kept.carried()),
        json!({"mode":"demo","source_ref_id":Uuid::from_u128(7)})
    );
    let kept = slice_draft("serious_prod", WORDS)
        .permitted(3, PermissionRecord::UserInput(WORDS))
        .unwrap();
    assert_eq!(kept.mode, ProgramMode::SeriousProd);
    assert_eq!(
        json!(kept),
        json!({"mode":"serious_prod","input_id":Uuid::from_u128(9),"exact_quote":WORDS})
    );
    assert_eq!(
        json!(kept.carried()),
        json!({"mode":"serious_prod","input_id":Uuid::from_u128(9)})
    );
}

#[test]
fn a_quote_must_be_present_and_verbatim() {
    let record = PermissionRecord::UserInput(WORDS);
    for blank in ["", "   ", "\n\t"] {
        assert_eq!(
            refusal(scope_draft("demo", blank).permitted(2, record)),
            (
                "SCOPE-DRAFT-MODE-OVERRIDE-QUOTE".into(),
                "/params/draft/candidates/2/mode_override/exact_quote".into(),
                "blank".into()
            )
        );
    }
    // A paraphrase, another case and words joined across a gap are not the quote.
    for other in [
        "I allow a demo",
        "i allow demo",
        "I allow demo nothing else",
        "I allow demo for the export Scope, nothing else. ",
    ] {
        assert_eq!(
            refusal(slice_draft("demo", other).permitted(1, record)),
            (
                "SLICE-DRAFT-MODE-OVERRIDE-QUOTE".into(),
                "/params/draft/nodes/1/mode_override/exact_quote".into(),
                "text the cited input does not contain".into()
            ),
            "{other}"
        );
    }
}

#[test]
fn only_an_input_the_user_gave_to_the_candidate_set_is_permission() {
    let quote = "I allow demo";
    for (record, found) in [
        (
            PermissionRecord::Unknown,
            "an id this candidate set does not have",
        ),
        (
            PermissionRecord::OutsideSnapshot,
            "an id that snapshot does not hold",
        ),
        (
            PermissionRecord::NotUserInput,
            "a record that is not the user's input to this candidate set",
        ),
    ] {
        assert_eq!(
            refusal(scope_draft("demo", quote).permitted(0, record)),
            (
                "SCOPE-DRAFT-MODE-OVERRIDE-INPUT".into(),
                "/params/draft/candidates/0/mode_override/source_ref_id".into(),
                found.into()
            )
        );
        assert_eq!(
            refusal(slice_draft("demo", quote).permitted(4, record)),
            (
                "SLICE-DRAFT-MODE-OVERRIDE-INPUT".into(),
                "/params/draft/nodes/4/mode_override/input_id".into(),
                found.into()
            )
        );
    }
}

#[test]
fn an_unknown_level_is_refused_whatever_the_quote() {
    for unknown in ["", "Demo", "production", "serious-prod", " demo"] {
        assert_eq!(
            refusal(scope_draft(unknown, WORDS).permitted(0, PermissionRecord::UserInput(WORDS))),
            (
                "SCOPE-DRAFT-MODE-OVERRIDE-LEVEL".into(),
                "/params/draft/candidates/0/mode_override/mode".into(),
                unknown.into()
            )
        );
    }
    let long = "x".repeat(500);
    let (rule, _, shown) =
        refusal(slice_draft(&long, WORDS).permitted(0, PermissionRecord::UserInput(WORDS)));
    assert_eq!(rule, "SLICE-DRAFT-MODE-OVERRIDE-LEVEL");
    assert_eq!(shown, "x".repeat(64));
    for mode in ProgramMode::ALL {
        let kept = slice_draft(mode.as_str(), WORDS)
            .permitted(0, PermissionRecord::UserInput(WORDS))
            .unwrap();
        assert_eq!(kept.mode, mode);
    }
}

#[test]
fn a_refusal_names_the_level_whose_input_command_records_the_answer() {
    let record = PermissionRecord::Unknown;
    let scope = scope_draft("demo", "x").permitted(0, record).unwrap_err();
    let slice = slice_draft("demo", "x").permitted(0, record).unwrap_err();
    let scope = scope.refusal().unwrap();
    let slice = slice.refusal().unwrap();
    assert_eq!(
        scope.next_action.as_deref(),
        Some("scope.candidates.record_input")
    );
    assert_eq!(slice.next_action.as_deref(), Some("slice.candidates.input"));
    assert_eq!(
        ModeOverrideLevel::of(&scope),
        Some(ModeOverrideLevel::Scope)
    );
    assert_eq!(
        ModeOverrideLevel::of(&slice),
        Some(ModeOverrideLevel::Slice)
    );
    // Any other refusal is not one of an override, whatever it points to next.
    let other = Refusal::new(RefusalCode::StateConflict).with_next_action("slice.candidates.input");
    assert_eq!(ModeOverrideLevel::of(&other), None);
}

#[test]
fn an_override_is_three_named_parts_and_nothing_else() {
    let id = Uuid::from_u128(7);
    let whole = json!({"mode":"mvp","source_ref_id":id,"exact_quote":"I allow mvp"});
    let draft: ScopeModeOverrideDraft = serde_json::from_value(whole.clone()).unwrap();
    assert_eq!(json!(draft), whole);
    let whole = json!({"mode":"mvp","input_id":id,"exact_quote":"I allow mvp"});
    let draft: SliceModeOverrideDraft = serde_json::from_value(whole.clone()).unwrap();
    assert_eq!(json!(draft), whole);

    // Each level takes its own id name, every part is required and no part is added.
    let scope = |value| serde_json::from_value::<ScopeModeOverrideDraft>(value).is_err();
    let slice = |value| serde_json::from_value::<SliceModeOverrideDraft>(value).is_err();
    assert!(scope(json!({"mode":"mvp","input_id":id,"exact_quote":"q"})));
    assert!(slice(
        json!({"mode":"mvp","source_ref_id":id,"exact_quote":"q"})
    ));
    assert!(scope(json!({"mode":"mvp","source_ref_id":id})));
    assert!(scope(json!({"source_ref_id":id,"exact_quote":"q"})));
    assert!(slice(json!({"mode":"mvp","exact_quote":"q"})));
    assert!(scope(
        json!({"mode":"mvp","source_ref_id":id,"exact_quote":"q","why":"x"})
    ));
    assert!(slice(
        json!({"mode":"mvp","input_id":"not-an-id","exact_quote":"q"})
    ));
    assert!(scope(
        json!({"mode":null,"source_ref_id":id,"exact_quote":"q"})
    ));
}

fn scope_candidate_json() -> serde_json::Value {
    json!({"identity":{"local":"scope"},"title":"Export","outcome":"Rows leave as a file",
        "trigger":"A user asks for the file","delivered_behavior":"A file is written",
        "proof":"The file opens","includes":["csv"],"excludes":[],"dependencies":[],
        "coverage_goals":[{"local":"goal"}],"evidence":[]})
}

#[test]
fn a_scope_candidate_draft_reads_and_writes_with_and_without_an_override() {
    use crate::{CandidateDraft, CandidateEntity};
    let plain = scope_candidate_json();
    let draft: CandidateDraft = serde_json::from_value(plain.clone()).unwrap();
    assert_eq!(draft.mode_override, None);
    // Without an override the draft serializes as it did before the field existed.
    assert_eq!(json!(draft), plain);

    let stated = json!({"mode":"demo","source_ref_id":Uuid::from_u128(7),"exact_quote":"I allow"});
    let mut with = plain.clone();
    with["mode_override"] = stated.clone();
    let draft: CandidateDraft = serde_json::from_value(with.clone()).unwrap();
    assert_eq!(draft.mode_override, Some(scope_draft("demo", "I allow")));
    assert_eq!(json!(draft), with);
    // A level outside the scale is read as written; the save's check refuses it.
    with["mode_override"]["mode"] = json!("enterprise");
    let draft: CandidateDraft = serde_json::from_value(with.clone()).unwrap();
    assert_eq!(draft.mode_override.unwrap().mode, "enterprise");

    // Null, a missing part and an unknown part are not an override.
    for broken in [
        serde_json::Value::Null,
        json!({"mode":"demo","exact_quote":"I allow"}),
        json!({"mode":"demo","source_ref_id":Uuid::from_u128(7),"exact_quote":"q","by":"me"}),
    ] {
        with["mode_override"] = broken;
        assert!(serde_json::from_value::<CandidateDraft>(with.clone()).is_err());
    }

    // A stored candidate of before reads as one without an override and writes no key.
    let stored = json!({"id":Uuid::from_u128(1),"revision":1,"title":"Export",
        "outcome":"o","trigger":"t","delivered_behavior":"b","proof":"p","includes":[],
        "excludes":[],"dependencies":[],"coverage_goal_ids":[],"evidence_ids":[]});
    let mut entity: CandidateEntity = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(entity.mode_override, None);
    assert_eq!(json!(entity), stored);
    entity.mode_override = Some(
        scope_draft("demo", "I allow")
            .permitted(0, PermissionRecord::UserInput("Yes, I allow it."))
            .unwrap(),
    );
    assert_eq!(json!(entity)["mode_override"], stated);
    assert_eq!(
        serde_json::from_value::<CandidateEntity>(json!(entity)).unwrap(),
        entity
    );
}

#[test]
fn a_slice_work_node_reads_and_writes_with_and_without_an_override() {
    use crate::{SliceCandidateDraftNode, SliceCandidateNode};
    let plain = json!({"kind":"work","identity":{"local":"work"},"title":"Export",
        "outcome":"Rows leave as a file","includes":["csv"],"excludes":[],"dependencies":[],
        "proof":["The file opens"],"pipeline":"slice.lightweight-tdd-development",
        "pipeline_reason":"Bounded","source_result_ids":[]});
    let node: SliceCandidateDraftNode = serde_json::from_value(plain.clone()).unwrap();
    assert!(matches!(
        &node,
        SliceCandidateDraftNode::Work {
            mode_override: None,
            ..
        }
    ));
    // Without an override the node serializes as it did before the field existed.
    assert_eq!(json!(node), plain);

    let stated = json!({"mode":"mvp","input_id":Uuid::from_u128(9),"exact_quote":"I allow"});
    let mut with = plain.clone();
    with["mode_override"] = stated.clone();
    let node: SliceCandidateDraftNode = serde_json::from_value(with.clone()).unwrap();
    let SliceCandidateDraftNode::Work { mode_override, .. } = &node else {
        panic!("a work node");
    };
    assert_eq!(
        mode_override.as_deref(),
        Some(&slice_draft("mvp", "I allow"))
    );
    assert_eq!(json!(node), with);

    // Null, a part of the other level and an unknown part are not an override.
    for broken in [
        serde_json::Value::Null,
        json!({"mode":"mvp","source_ref_id":Uuid::from_u128(9),"exact_quote":"I allow"}),
        json!({"mode":"mvp","input_id":Uuid::from_u128(9),"exact_quote":"q","by":"me"}),
    ] {
        with["mode_override"] = broken;
        assert!(serde_json::from_value::<SliceCandidateDraftNode>(with.clone()).is_err());
    }
    // A decision node takes none.
    let mut decision = json!({"kind":"decision","identity":{"local":"choice"},
        "title":"Choose","question":"Which?","resolution_criteria":["One is chosen"]});
    assert!(serde_json::from_value::<SliceCandidateDraftNode>(decision.clone()).is_ok());
    decision["mode_override"] = stated.clone();
    assert!(serde_json::from_value::<SliceCandidateDraftNode>(decision).is_err());

    // A stored work node of before reads as one without an override and writes no key.
    let stored = json!({"kind":"work","id":Uuid::from_u128(1),"revision":1,"title":"Export",
        "outcome":"o","includes":[],"excludes":[],"dependencies":[],"proof":["p"],
        "pipeline":"slice.lightweight-tdd-development","pipeline_reason":"r",
        "why_lightweight_insufficient":null,"why_further_vertical_split_not_viable":null,
        "source_result_ids":[]});
    let mut node: SliceCandidateNode = serde_json::from_value(stored.clone()).unwrap();
    assert_eq!(json!(node), stored);
    let SliceCandidateNode::Work { mode_override, .. } = &mut node else {
        panic!("a work node");
    };
    assert_eq!(*mode_override, None);
    *mode_override = Some(Box::new(
        slice_draft("mvp", "I allow")
            .permitted(0, PermissionRecord::UserInput("Yes, I allow it."))
            .unwrap(),
    ));
    assert_eq!(json!(node)["mode_override"], stated);
    assert_eq!(
        serde_json::from_value::<SliceCandidateNode>(json!(node)).unwrap(),
        node
    );
}
