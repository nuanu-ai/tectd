use super::*;

/// A second, independent decision after the reviewed provider send. One line
/// must bind the exact saved advice digest and one eligible choice ID.
pub(crate) fn selection_confirmation(
    reader: &mut impl BufRead,
    advice_digest: &str,
    eligible: &[String],
) -> Option<String> {
    let mut line = String::new();
    reader.read_line(&mut line).ok()?;
    let line = line.strip_suffix('\n')?;
    let line = line.strip_suffix('\r').unwrap_or(line);
    eligible
        .iter()
        .find(|choice| line == format!("SELECT JEV MATRIX {advice_digest} {choice}"))
        .cloned()
}

pub(super) fn owner_case_draft(choice: &Value, full_cards: &[Value; 2]) -> Value {
    let choice_id = choice["candidate_id"].as_str().expect("selected choice ID");
    assert!(matches!(
        choice_id,
        "matrix-local-evidence-first" | "matrix-trust-first"
    ));
    let title = choice["title"].as_str().expect("source-backed title");
    let approach = choice["approach"].as_str().expect("source-backed approach");
    let card_body = |index: usize, expected: &str| -> &str {
        let card = &full_cards[index]["selected_card"];
        assert_eq!(card["id"], expected);
        card["body"].as_str().expect("approved mandatory card body")
    };
    let scope_body = card_body(0, "EM02-SCOPE@0.1");
    let protect_body = card_body(1, "EM02-PROTECT@0.1");
    let mut selected = json!({
        "kind":"work","identity":{"local":"selected-matrix-approach"},
        "title":title,"outcome":format!("Deliver the bounded {choice_id} approach"),
        "includes":[approach],"excludes":["production promotion without separate approval"],
        "dependencies":[],"proof":["Evidence of the selected approach's bounded implementation"],
        "pipeline":"slice.lightweight-tdd-development",
        "pipeline_reason":"A bounded implementation and focused acceptance proof are sufficient"
    });
    if choice_id == "matrix-trust-first" {
        selected["pipeline"] = json!("slice.full-design-to-execution");
        selected["pipeline_reason"] = json!(
            "The production evidence resolver and signed trust policy cross authorization and persistence boundaries"
        );
        selected["why_lightweight_insufficient"] = json!(
            "A narrow code change cannot establish the resolver, owner key and signed policy together"
        );
        selected["why_further_vertical_split_not_viable"] = json!(
            "This Work is the bounded trust-boundary design; later implementation remains separately reviewable"
        );
    }
    json!({
        "coverage_summary":format!(
            "Selected {choice_id}; preserve applicable EM02-SCOPE@0.1 and EM02-PROTECT@0.1 obligations."
        ),
        "nodes":[
            selected,
            {"kind":"work","identity":{"local":"mandatory-em02-scope"},
             "title":"Preserve EM02-SCOPE@0.1", "outcome":"The Owner-established scope and proof remain explicit",
             "includes":[scope_body],"excludes":[],"dependencies":[],
             "proof":["Verify the exact Owner declarations, operating evidence and promised proof"],
             "pipeline":"slice.lightweight-tdd-development",
             "pipeline_reason":"Mandatory scope obligations need focused evidence checks"},
            {"kind":"work","identity":{"local":"mandatory-em02-protect"},
             "title":"Preserve EM02-PROTECT@0.1", "outcome":"Data and secret guarantees remain protected with proof",
             "includes":[protect_body,"Affected guarantees: data and secret"],
             "excludes":[],"dependencies":[],
             "proof":["Verify affected data and secret guarantees and their proof"],
             "pipeline":"slice.lightweight-tdd-development",
             "pipeline_reason":"The Owner-confirmed data and secret guarantees need focused proof"}
        ],
        "supersessions":[]
    })
}

#[test]
fn second_confirmation_is_exact_and_owner_drafts_preserve_both_applicable_cards() {
    let digest = "a".repeat(64);
    let choices = vec![
        "matrix-local-evidence-first".into(),
        "matrix-trust-first".into(),
    ];
    for line in [
        "".to_owned(),
        format!("SELECT JEV MATRIX {digest} matrix-local-evidence-first"),
        format!(
            "SELECT JEV MATRIX {} matrix-local-evidence-first\n",
            "b".repeat(64)
        ),
        format!("SELECT JEV MATRIX {digest} unknown\n"),
    ] {
        assert_eq!(
            selection_confirmation(&mut std::io::Cursor::new(line), &digest, &choices),
            None
        );
    }
    let cards = [
        json!({"selected_card":{"id":"EM02-SCOPE@0.1","body":"Exact scope duty"}}),
        json!({"selected_card":{"id":"EM02-PROTECT@0.1","body":"Exact protection duty"}}),
    ];
    let task = Uuid::new_v4();
    for choice in owner_choices(task)["candidates"].as_array().unwrap() {
        let choice_id = choice["candidate_id"].as_str().unwrap();
        let line = format!("SELECT JEV MATRIX {digest} {choice_id}\n");
        assert_eq!(
            selection_confirmation(&mut std::io::Cursor::new(line), &digest, &choices),
            Some(choice_id.into())
        );
        let draft = owner_case_draft(choice, &cards);
        assert_eq!(draft["nodes"][0]["title"], choice["title"]);
        assert_eq!(draft["nodes"][0]["includes"][0], choice["approach"]);
        assert_eq!(draft["nodes"][1]["includes"][0], "Exact scope duty");
        assert_eq!(draft["nodes"][2]["includes"][0], "Exact protection duty");
    }
}
