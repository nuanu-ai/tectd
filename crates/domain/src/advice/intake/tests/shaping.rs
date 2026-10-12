//! What the answers become: the one short field, the abstain rules and the worst case.
use super::*;

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

fn completed(advice: &IntakeAdvice, answers: BTreeMap<String, AdviceAnswer>) -> AdviceField {
    let reply = AdviceReply {
        answers,
        exchange: AdviceExchange::default(),
    };
    AdviceCompletion::from_provider(Ok(reply), advice).advice
}

#[test]
fn answers_become_tokens_a_draft_position_and_two_decimal_probabilities() {
    let draft = slice_draft();
    let advice = slice_advice(Some(&draft));
    let given = answers(&advice, |name| match name {
        "work" => ("bounded", 0.6249),
        "placement" => ("extends", 0.999),
        "nearest" => ("c3", 0.805),
        "fit" => ("in_line", 0.3),
        other => panic!("unexpected question {other}"),
    });
    let field = advice.shape(&given);
    assert_eq!(
        content(&field),
        json!({"status":"advised","note":INTAKE_NOTE,"level":"scope_to_slices",
            "work":"bounded","work_p":0.62,"placement":"extends","placement_p":1.0,
            "nearest":3,"nearest_p":0.81,"fit":"in_line","fit_p":0.3})
    );
    // Tokens and a position only: no text of the input or the draft travels back.
    let text = json!(field).to_string();
    for absent in ["revoke", "Delivery", "confidence", "probabilities", "c3"] {
        assert!(!text.contains(absent), "{absent}");
    }
    for part in [
        "Jev",
        "work",
        "placement",
        "nearest",
        "fit",
        "1-based",
        "saved draft",
    ] {
        assert!(INTAKE_NOTE.contains(part), "{part}");
    }
    assert!(INTAKE_NOTE.contains("*_p is Jev's probability") && INTAKE_NOTE.contains("you decide"));
    assert_eq!(INTAKE_NOTE.matches(". ").count(), 0, "one sentence");
}

#[test]
fn nearest_is_a_position_or_none_or_abstain_and_absent_when_not_asked() {
    let draft = scope_draft(12);
    let advice = scope_advice(Some(&draft));
    let nearest = |choice: &'static str| {
        let given = answers(&advice, |name| match name {
            "nearest" => (choice, 0.5),
            "work" => ("none", 0.9),
            "placement" => ("covered", 0.6),
            _ => ("unclear", 0.7),
        });
        content(&advice.shape(&given))
    };
    assert_eq!(nearest("c1")["nearest"], json!(1));
    assert_eq!(nearest("c12")["nearest"], json!(12));
    assert_eq!(nearest("none")["nearest"], json!("none"));
    assert_eq!(nearest("ABSTAIN")["nearest"], json!("ABSTAIN"));
    assert_eq!(
        nearest("none"),
        json!({"status":"advised","note":INTAKE_NOTE,"level":"program_to_scopes",
            "work":"none","work_p":0.9,"placement":"covered","placement_p":0.6,
            "nearest":"none","nearest_p":0.5,"fit":"unclear","fit_p":0.7})
    );

    // No saved draft: the question was not asked and its two keys are not in the field.
    let bare = scope_advice(None);
    let given = answers(&bare, |name| match name {
        "work" => ("large", 0.7),
        "placement" => ("new_candidate", 0.6),
        _ => ("beyond_mode", 0.8),
    });
    assert_eq!(
        content(&bare.shape(&given)),
        json!({"status":"advised","note":INTAKE_NOTE,"level":"program_to_scopes",
            "work":"large","work_p":0.7,"placement":"new_candidate","placement_p":0.6,
            "fit":"beyond_mode","fit_p":0.8})
    );
}

#[test]
fn an_abstained_question_keeps_abstain_and_only_all_abstain_abstains() {
    let draft = slice_draft();
    let advice = slice_advice(Some(&draft));
    let partly = answers(&advice, |name| match name {
        "work" => ("small", 0.7),
        _ => (ADVICE_ABSTAIN, 0.55),
    });
    assert_eq!(
        content(&completed(&advice, partly)),
        json!({"status":"advised","note":INTAKE_NOTE,"level":"scope_to_slices",
            "work":"small","work_p":0.7,"placement":"ABSTAIN","placement_p":0.55,
            "nearest":"ABSTAIN","nearest_p":0.55,"fit":"ABSTAIN","fit_p":0.55})
    );
    let all = answers(&advice, |_| (ADVICE_ABSTAIN, 0.9));
    let field = completed(&advice, all);
    assert_eq!(field, AdviceField::Abstained);
    assert_eq!(json!(field), json!({"status":"abstained"}));

    // An answer that is not there is never guessed, nor is a candidate never offered.
    let mut missing = answers(&advice, |_| ("ABSTAIN", 0.9));
    missing.insert("work".into(), answer(&advice, "work", "small", 0.9));
    missing.remove("fit");
    let field = advice.shape(&missing);
    assert_eq!(
        field.reason(),
        Some(AdviceUnavailableReason::InvalidResponse)
    );
    assert!(json!(field).to_string().contains("`fit`"));
    let mut unoffered = answers(&advice, |_| ("ABSTAIN", 0.9));
    unoffered.get_mut("nearest").unwrap().choice = "c2".into();
    let field = advice.shape(&unoffered);
    assert_eq!(
        field.reason(),
        Some(AdviceUnavailableReason::InvalidResponse)
    );
    assert!(json!(field).to_string().contains("`nearest`"));
}

#[test]
fn no_shaped_field_is_larger_than_the_worst_case_sample() {
    let (few, many, nodes) = (scope_draft(2), scope_draft(120), slice_draft());
    let cases = [
        scope_advice(None),
        scope_advice(Some(&few)),
        scope_advice(Some(&many)),
        slice_advice(None),
        slice_advice(Some(&nodes)),
    ];
    for advice in &cases {
        let largest = advice.largest();
        let sample = content(&largest);
        let names = asked(advice);
        for name in &names {
            assert_eq!(sample[format!("{name}_p")], json!(0.99), "{name}");
        }
        assert_eq!(
            sample.as_object().unwrap().len(),
            3 + 2 * names.len(),
            "status, note and level, then a value and a probability per question"
        );
        // One question at a time takes each of its criteria at each probability, while
        // the others hold their longest value: every shaped field stays within the sample.
        for varied in &names {
            for choice in criteria(advice, varied) {
                for probability in [0.0, 0.004, 0.005, 0.1234567, 0.5, 0.985, 0.994, 0.995, 1.0] {
                    let given = names
                        .iter()
                        .map(|name| {
                            let token = if name == varied {
                                choice.clone()
                            } else {
                                longest(advice, name)
                            };
                            let given = answer(advice, name, &token, probability);
                            ((*name).to_owned(), given)
                        })
                        .collect();
                    let shaped = advice.shape(&given);
                    assert!(matches!(shaped, AdviceField::Advised { .. }));
                    assert!(
                        reply_bytes(&shaped) <= reply_bytes(&largest),
                        "{varied} {choice} {probability}"
                    );
                    assert!(json!(shaped).to_string().len() <= sample.to_string().len());
                }
            }
        }
    }
    assert_eq!(
        content(&cases[4].largest()),
        json!({"status":"advised","note":INTAKE_NOTE,"level":"scope_to_slices",
            "work":"oversized","work_p":0.99,"placement":"outside_program","placement_p":0.99,
            "nearest":"ABSTAIN","nearest_p":0.99,"fit":"beyond_mode","fit_p":0.99})
    );
    assert_eq!(
        content(&cases[0].largest()),
        json!({"status":"advised","note":INTAKE_NOTE,"level":"program_to_scopes",
            "work":"oversized","work_p":0.99,"placement":"outside_program","placement_p":0.99,
            "fit":"beyond_mode","fit_p":0.99})
    );
}

/// The criterion of a question whose name is longest, as Jev may choose it.
fn longest(advice: &IntakeAdvice, name: &str) -> String {
    criteria(advice, name)
        .into_iter()
        .max_by_key(String::len)
        .unwrap()
}

#[test]
fn the_subject_is_the_recorded_input() {
    assert_eq!(scope_advice(None).subject(), "this input");
    let draft = slice_draft();
    assert_eq!(slice_advice(Some(&draft)).subject(), "this input");
}
