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
