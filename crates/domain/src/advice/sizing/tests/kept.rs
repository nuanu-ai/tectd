//! What is kept of Jev's answers for the review of the draft: one answer per asked
//! candidate under the id and revision the store holds it under, and whether it asks
//! for the candidate to be changed.
use super::*;

/// The Slice draft with every work node written: position 1 is new and held under
/// `Uuid::from_u128(1)` at revision 1, position 3 is `existing` at revision 2.
fn advice(existing: Uuid) -> SizingAdvice {
    let stored = slice_stored(existing);
    SizingDraft::slice_candidates(&slice_draft(existing), &stored, scope_statement())
        .unwrap()
        .unwrap()
        .advice(&words(), ProgramMode::Prod)
}

/// What is kept when Jev gives the first candidate this advice and classes its one
/// included item this way, and keeps the third candidate at a size alone.
fn first(advice: &SizingAdvice, choice: &'static str, item: &'static str) -> CandidateAdvice {
    let given = answers(advice, |name| match name {
        "c1.advice" => (choice, 0.6),
        "c1.i1" => (item, 0.6),
        "c3.size" => ("oversized", 0.9),
        "c3.advice" => ("keep", 0.6),
        name if name.ends_with(".size") => ("bounded", 0.6),
        _ => ("requested", 0.6),
    });
    advice.kept(&given).remove(0)
}

#[test]
fn an_answer_asks_for_a_change_by_its_advice_or_by_an_extra_item() {
    let advice = advice(Uuid::new_v4());
    for (choice, item, asks) in [
        ("keep", "requested", false),
        ("keep", "enabler", false),
        (ADVICE_ABSTAIN, "requested", false),
        (ADVICE_ABSTAIN, ADVICE_ABSTAIN, false),
        ("split", "requested", true),
        ("simplify", "requested", true),
        ("clarify", "requested", true),
        ("separate_scope", "requested", true),
        // An item Jev classed as extra asks for it to be cut, whatever the advice.
        ("keep", "extra", true),
        (ADVICE_ABSTAIN, "extra", true),
    ] {
        assert_eq!(
            first(&advice, choice, item).asks_change,
            asks,
            "{choice} {item}"
        );
    }
    // Every choice the advice question offers is one of these: a token added to the
    // catalogue has to be placed here too.
    for level in [scope_advice(), slice_advice()] {
        let name = level
            .questions()
            .questions
            .keys()
            .find(|name| name.ends_with(".advice"))
            .unwrap()
            .clone();
        for token in criteria(&level, &name) {
            let asks_nothing = token == "keep" || token == ADVICE_ABSTAIN;
            assert_ne!(
                ASKS_CHANGE.contains(&token.as_str()),
                asks_nothing,
                "{token}"
            );
        }
    }
}

#[test]
fn each_asked_candidate_is_kept_under_the_id_and_revision_the_store_holds() {
    let existing = Uuid::new_v4();
    let advice = advice(existing);
    let given = answers(&advice, |name| match name {
        "c1.size" => ("small", 0.6249),
        "c1.advice" => ("keep", 0.999),
        "c3.size" => ("oversized", 0.505),
        "c3.advice" => ("separate_scope", 0.3),
        "c3.i2" => ("extra", 0.7),
        _ => ("requested", 0.9),
    });
    let kept = advice.kept(&given);
    let field = content(&advice.shape(&given));
    assert_eq!(
        kept,
        [
            CandidateAdvice {
                level: SizingLevel::ScopeToSlices,
                candidate_id: Uuid::from_u128(1),
                candidate_revision: 1,
                asks_change: false,
                answer: field["candidates"][0].clone(),
            },
            CandidateAdvice {
                level: SizingLevel::ScopeToSlices,
                candidate_id: existing,
                candidate_revision: 2,
                asks_change: true,
                answer: field["candidates"][1].clone(),
            }
        ]
    );
    // The answer is the candidate's entry of the reply field: what Jev chose and its
    // probabilities, and no text of the draft. A large size alone asks for nothing.
    assert_eq!(
        kept[1].answer,
        json!({"candidate":3,"size":"oversized","size_p":0.51,"advice":"separate_scope",
            "advice_p":0.3,"extra_items":[2],"extra_p":[0.7]})
    );
    let text = json!(kept.iter().map(|kept| &kept.answer).collect::<Vec<_>>()).to_string();
    for absent in ["Invitation form", "Delivery", "retries", "sending"] {
        assert!(!text.contains(absent), "{absent}");
    }

    // The Scope level keeps its own level, and the changed candidate under the
    // revision the save wrote.
    let scope = scope_advice_of(existing);
    let kept = scope.kept(&answers(&scope, |name| match name.rsplit('.').next() {
        Some("size") => ("bounded", 0.7),
        Some("advice") => ("keep", 0.7),
        _ => ("requested", 0.7),
    }));
    assert_eq!(
        kept.iter()
            .map(|kept| (kept.level, kept.candidate_id, kept.candidate_revision))
            .collect::<Vec<_>>(),
        [
            (SizingLevel::ProgramToScopes, Uuid::from_u128(1), 1),
            (SizingLevel::ProgramToScopes, existing, 3)
        ]
    );
}

#[test]
fn only_an_advised_completion_keeps_answers() {
    let advice = advice(Uuid::new_v4());
    let reply = |answers| AdviceReply {
        answers,
        exchange: AdviceExchange::default(),
    };
    let split = answers(&advice, |name| match name {
        "c1.advice" => ("split", 0.8),
        name if name.ends_with(".advice") => ("keep", 0.8),
        name if name.ends_with(".size") => ("bounded", 0.8),
        _ => ("requested", 0.8),
    });
    let advised = AdviceCompletion::from_provider(Ok(reply(split.clone())), &advice);
    assert_eq!(
        advised
            .kept
            .iter()
            .map(|kept| kept.asks_change)
            .collect::<Vec<_>>(),
        [true, false]
    );
    // Jev abstained on everything: nothing is kept, so nothing can be open.
    let abstained = answers(&advice, |_| (ADVICE_ABSTAIN, 0.9));
    let completed = AdviceCompletion::from_provider(Ok(reply(abstained)), &advice);
    assert_eq!(completed.advice, AdviceField::Abstained);
    assert!(completed.kept.is_empty());
    // Answers that make no advised field keep nothing either.
    let mut missing = split;
    missing.remove("c3.size");
    let completed = AdviceCompletion::from_provider(Ok(reply(missing.clone())), &advice);
    assert_eq!(
        completed.advice.reason(),
        Some(AdviceUnavailableReason::InvalidResponse)
    );
    assert!(completed.kept.is_empty());
    assert!(advice.kept(&missing).is_empty());
}
