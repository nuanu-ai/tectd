//! What the answers become: the compact field, the abstain rules and the worst case.
use super::*;

#[test]
fn answers_become_positions_tokens_and_two_decimal_probabilities() {
    let advice = slice_advice();
    let given = answers(&advice, |name| match name {
        "c1.size" => ("small", 0.6249),
        "c1.advice" => ("keep", 0.999),
        "c1.i1" => ("requested", 0.9),
        "c3.size" => ("oversized", 0.505),
        "c3.advice" => ("separate_scope", 0.3),
        "c3.i1" => ("enabler", 0.5),
        "c3.i2" => ("extra", 0.7),
        "c3.i3" => ("extra", 0.34),
        other => panic!("unexpected question {other}"),
    });
    let field = advice.shape(&given);
    assert_eq!(
        content(&field),
        json!({"status":"advised","note":SIZING_NOTE,"level":"scope_to_slices","candidates":[
            {"candidate":1,"size":"small","size_p":0.62,"advice":"keep","advice_p":1.0,
             "extra_items":[],"extra_p":[]},
            {"candidate":3,"size":"oversized","size_p":0.51,"advice":"separate_scope",
             "advice_p":0.3,"extra_items":[2,3],"extra_p":[0.7,0.34]}
        ]})
    );
    // Positions, tokens and probabilities only: no text of the draft and no confidence
    // travel back.
    let text = json!(field).to_string();
    for absent in ["Invitation form", "retries", "confidence", "probabilities"] {
        assert!(!text.contains(absent), "{absent}");
    }
}

#[test]
fn the_note_is_one_sentence_that_says_to_follow_the_advice_or_give_the_reason() {
    for part in [
        "Jev's advice",
        "each candidate this save created or changed",
        "1-based in draft order",
        "*_p is Jev's probability",
        "follow the advice by changing the draft and saving it again",
        "where the advice is plainly wrong give the reason in advice_departures of the review",
        "a review that says ready is refused until then",
    ] {
        assert!(SIZING_NOTE.contains(part), "{part}");
    }
    assert!(!SIZING_NOTE.contains("you decide"));
    assert_eq!(SIZING_NOTE.matches('.').count(), 1, "one sentence");
    assert!(SIZING_NOTE.ends_with('.'));
    // It travels only inside an advised field, the largest one too.
    let advice = slice_advice();
    assert_eq!(content(&advice.largest())["note"], SIZING_NOTE);
}

#[test]
fn an_abstained_candidate_keeps_abstain_and_only_all_abstain_abstains() {
    let advice = scope_advice();
    let partly = answers(&advice, |name| match name {
        "c1.size" | "c1.advice" | "c1.i1" => (ADVICE_ABSTAIN, 0.55),
        "c1.i2" => ("extra", 0.8),
        "c2.size" => ("bounded", 0.7),
        _ => ("keep", 0.7),
    });
    let reply = |answers| AdviceReply {
        answers,
        exchange: AdviceExchange::default(),
    };
    let completed = AdviceCompletion::from_provider(Ok(reply(partly)), &advice);
    assert_eq!(
        content(&completed.advice)["candidates"],
        json!([
            {"candidate":1,"size":"ABSTAIN","size_p":0.55,"advice":"ABSTAIN","advice_p":0.55,
             "extra_items":[2],"extra_p":[0.8]},
            {"candidate":2,"size":"bounded","size_p":0.7,"advice":"keep","advice_p":0.7,
             "extra_items":[],"extra_p":[]}
        ])
    );
    let all = answers(&advice, |_| (ADVICE_ABSTAIN, 0.9));
    let completed = AdviceCompletion::from_provider(Ok(reply(all)), &advice);
    assert_eq!(completed.advice, AdviceField::Abstained);
    assert!(!json!(completed.advice).to_string().contains("note"));

    // An answer that is not there is never guessed.
    let mut missing = answers(&advice, |_| (ADVICE_ABSTAIN, 0.9));
    missing.insert("c2.size".into(), answer(&advice, "c2.size", "small", 0.9));
    missing.remove("c1.i2");
    let field = advice.shape(&missing);
    assert_eq!(
        field.reason(),
        Some(AdviceUnavailableReason::InvalidResponse)
    );
    assert!(json!(field).to_string().contains("c1.i2"));
}

#[test]
fn no_shaped_field_is_larger_than_the_worst_case_sample() {
    for advice in [scope_advice(), slice_advice()] {
        let largest = advice.largest();
        let sample = content(&largest);
        // The sample lists every included item and prints the widest probability.
        for (entry, asked) in sample["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .zip(&advice.asked)
        {
            assert_eq!(entry["candidate"], asked.written.position);
            assert_eq!(
                entry["extra_items"],
                json!((1..=asked.items).collect::<Vec<_>>())
            );
            assert_eq!(entry["extra_p"], json!(vec![0.99; asked.items]));
            assert_eq!(
                (&entry["size_p"], &entry["advice_p"]),
                (&json!(0.99), &json!(0.99))
            );
        }
        let tokens = |suffix: &str| {
            let name = advice
                .questions()
                .questions
                .keys()
                .find(|name| name.ends_with(suffix))
                .unwrap();
            criteria(&advice, name)
        };
        for size in tokens(".size") {
            for choice in tokens(".advice") {
                for item in tokens(".i1") {
                    for probability in [0.0, 0.004, 0.005, 0.1234567, 0.5, 0.985, 0.994, 0.995, 1.0]
                    {
                        let given = advice
                            .questions()
                            .questions
                            .keys()
                            .map(|name| {
                                let token = match name.rsplit('.').next().unwrap() {
                                    "size" => &size,
                                    "advice" => &choice,
                                    _ => &item,
                                };
                                (name.clone(), answer(&advice, name, token, probability))
                            })
                            .collect();
                        let shaped = advice.shape(&given);
                        assert!(matches!(shaped, AdviceField::Advised { .. }));
                        assert!(
                            reply_bytes(&shaped) <= reply_bytes(&largest),
                            "{size} {choice} {item} {probability}"
                        );
                        assert!(json!(shaped).to_string().len() <= sample.to_string().len());
                    }
                }
            }
        }
    }
    assert_eq!(
        content(&slice_advice().largest())["candidates"][1],
        json!({"candidate":3,"size":"oversized","size_p":0.99,"advice":"separate_scope",
            "advice_p":0.99,"extra_items":[1,2,3],"extra_p":[0.99,0.99,0.99]})
    );
    assert_eq!(
        content(&scope_advice().largest())["candidates"][0]["advice"],
        "simplify"
    );
}

#[test]
fn the_subject_counts_candidates_and_included_items() {
    assert_eq!(
        scope_advice().subject(),
        "this draft (2 candidates, 2 included items)"
    );
    assert_eq!(
        slice_advice().subject(),
        "this draft (2 candidates, 4 included items)"
    );
    let mut one = scope_draft(Uuid::new_v4());
    one.candidates.truncate(1);
    one.candidates[0].dependencies.clear();
    one.candidates[0].includes.truncate(1);
    let stored = stored_scopes(&[(Uuid::new_v4(), 1)]);
    let one = SizingDraft::scope_candidates(&one, &stored)
        .unwrap()
        .unwrap();
    assert_eq!(
        one.advice(&[], ProgramMode::Prod).subject(),
        "this draft (1 candidate, 1 included item)"
    );
}
