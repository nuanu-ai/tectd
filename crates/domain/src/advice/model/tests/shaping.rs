//! What the answers become: the one short field, the abstain rules, the worst case and
//! the field the point gives without room.
use super::*;

/// An answer the acceptance rules take: `choice` at `probability`, the rest shared by
/// the other criteria.
fn answer(advice: &ModelAdvice, name: &str, choice: &str, probability: f64) -> AdviceAnswer {
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

/// Both questions answered: `practical` and `specialist`, each a choice and its probability.
fn answers(
    advice: &ModelAdvice,
    practical: (&str, f64),
    specialist: (&str, f64),
) -> BTreeMap<String, AdviceAnswer> {
    [("practical", practical), ("specialist", specialist)]
        .into_iter()
        .map(|(name, (choice, probability))| {
            (name.to_owned(), answer(advice, name, choice, probability))
        })
        .collect()
}

fn completed(advice: &ModelAdvice, answers: BTreeMap<String, AdviceAnswer>) -> AdviceCompletion {
    let reply = AdviceReply {
        answers,
        exchange: AdviceExchange::default(),
    };
    AdviceCompletion::from_provider(Ok(reply), advice)
}

/// Bytes the field costs inside a reply, where the data travels as a JSON string.
fn reply_bytes(field: &AdviceField) -> usize {
    serde_json::to_string(&json!(field).to_string())
        .unwrap()
        .len()
}

#[test]
fn both_answers_become_two_tokens_and_two_decimal_probabilities() {
    let advice = advice();
    let given = answers(&advice, ("implementation", 0.7149), ("none", 0.875));
    assert!(advice.questions().accept(&given).is_ok());
    let completion = completed(&advice, given);
    assert_eq!(
        json!(completion.advice),
        json!({"status":"advised","note":MODEL_NOTE,
            "practical":"implementation","practical_p":0.71,
            "specialist":"none","specialist_p":0.88})
    );
    // Tokens only: no text of the input, nothing else of Jev's answer.
    let text = json!(completion.advice).to_string();
    for absent in [
        "Compare",
        "mail",
        "cheaper",
        "confidence",
        "probabilities",
        "level",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
    // No review is held to this point: nothing is kept beside the journal row.
    assert!(completion.kept.is_empty());
    assert!(advice.kept(&BTreeMap::new()).is_empty());
}

#[test]
fn an_abstained_question_keeps_abstain_and_only_both_abstaining_abstains() {
    let advice = advice();
    let practical_only = answers(&advice, ("routine", 0.6), (ADVICE_ABSTAIN, 0.55));
    assert_eq!(
        json!(completed(&advice, practical_only).advice),
        json!({"status":"advised","note":MODEL_NOTE,"practical":"routine","practical_p":0.6,
            "specialist":"ABSTAIN","specialist_p":0.55})
    );
    let specialist_only = answers(&advice, (ADVICE_ABSTAIN, 0.5), ("demanding", 1.0));
    assert_eq!(
        json!(completed(&advice, specialist_only).advice),
        json!({"status":"advised","note":MODEL_NOTE,"practical":"ABSTAIN","practical_p":0.5,
            "specialist":"demanding","specialist_p":1.0})
    );
    let neither = answers(&advice, (ADVICE_ABSTAIN, 0.9), (ADVICE_ABSTAIN, 0.8));
    let field = completed(&advice, neither).advice;
    assert_eq!(field, AdviceField::Abstained);
    assert_eq!(json!(field), json!({"status":"abstained"}));

    // An answer that is not there is never guessed.
    for missing in ["practical", "specialist"] {
        let mut given = answers(&advice, ("routine", 0.6), ("standard", 0.7));
        given.remove(missing);
        let field = advice.shape(&given);
        assert_eq!(
            field.reason(),
            Some(AdviceUnavailableReason::InvalidResponse)
        );
        assert!(json!(field).to_string().contains(&format!("`{missing}`")));
    }
}

#[test]
fn no_shaped_field_is_larger_than_the_worst_case_sample() {
    let advice = advice();
    let largest = advice.largest();
    assert_eq!(
        json!(largest),
        json!({"status":"advised","note":MODEL_NOTE,
            "practical":"implementation","practical_p":0.99,
            "specialist":"demanding","specialist_p":0.99})
    );
    // Every pair of criteria at every probability stays within the sample.
    let probabilities = [0.0, 0.004, 0.005, 0.1234567, 0.5, 0.985, 0.994, 0.995, 1.0];
    for practical in criteria(&advice, "practical") {
        for specialist in criteria(&advice, "specialist") {
            for probability in probabilities {
                let given = answers(
                    &advice,
                    (practical.as_str(), probability),
                    (specialist.as_str(), probability),
                );
                let shaped = advice.shape(&given);
                assert!(matches!(shaped, AdviceField::Advised { .. }));
                assert!(
                    reply_bytes(&shaped) <= reply_bytes(&largest),
                    "{practical} {specialist} {probability}"
                );
                assert!(json!(shaped).to_string().len() <= json!(largest).to_string().len());
            }
        }
    }
    // Nor is a field the point gives without advice: Jev abstaining, or no room.
    for small in [AdviceField::Abstained, ModelAdvice::without_room()] {
        assert!(reply_bytes(&small) < reply_bytes(&largest));
    }
}

#[test]
fn the_note_is_one_sentence_that_says_what_to_do() {
    assert_eq!(MODEL_NOTE.matches('.').count(), 1, "one sentence");
    assert!(MODEL_NOTE.ends_with('.'));
    for part in [
        "Jev's advice on the executors that the work of the input just recorded needs",
        "practical is the kind of executor its practical work needs",
        "specialist is whether it needs bounded intellectual work of its own",
        "*_p is Jev's probability for its choice",
        "start the executor of the practical work of this input on the route your \
         orchestration policy gives the named kind, routine or implementation",
        "hand separate intellectual work to the specialist at its standard effort for \
         standard and at its raised effort for demanding",
        "depart only where the advice is plainly wrong, and tell the user so",
    ] {
        assert!(MODEL_NOTE.contains(part), "{part}");
    }
    // There is no gate behind this point, and the agent is not left to decide.
    assert!(!MODEL_NOTE.contains("advice_departures"));
    assert!(!MODEL_NOTE.contains("you decide"));
}

#[test]
fn without_room_the_point_says_so_in_the_field_a_full_reply_would_get() {
    let advice = advice();
    assert_eq!(advice.subject(), "the executors of this input");
    let short = ModelAdvice::without_room();
    assert_eq!(short, AdviceField::too_large(&advice.subject()));
    assert_eq!(
        json!(short),
        json!({"status":"unavailable","reason":"advice_too_large",
            "message":"Jev advice is not available: advice for the executors of this input \
                would not fit the command reply"})
    );
}
