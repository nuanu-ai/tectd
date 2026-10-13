//! What the answers become: one short entry per asked node, the abstain rules and
//! the worst case.
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

fn completed(advice: &PipelineAdvice, answers: BTreeMap<String, AdviceAnswer>) -> AdviceField {
    let reply = AdviceReply {
        answers,
        exchange: AdviceExchange::default(),
    };
    AdviceCompletion::from_provider(Ok(reply), advice).advice
}

#[test]
fn answers_become_jevs_choice_its_probability_and_the_one_for_the_drafts_choice() {
    let advice = advice();
    // The draft chose Lightweight for node 1, Debug for node 4 and Full for node 6.
    let given = answers(&advice, |name| match name {
        // Jev agrees.
        "c1.pipeline" => ((LIGHTWEIGHT, 0.7449), (LIGHTWEIGHT, 0.7449)),
        // Jev chooses another pipeline and gives the draft's a share of its own.
        "c4.pipeline" => ((LIGHTWEIGHT, 0.74), (DEBUG, 0.1249)),
        "c6.pipeline" => ((RESEARCH, 0.505), (FULL, 0.2)),
        other => panic!("unexpected question {other}"),
    });
    let field = advice.shape(&given);
    assert_eq!(
        content(&field),
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":1,"pipeline":LIGHTWEIGHT,"pipeline_p":0.74,"draft_pipeline_p":0.74,
             "differs":false},
            {"candidate":4,"pipeline":LIGHTWEIGHT,"pipeline_p":0.74,"draft_pipeline_p":0.12,
             "differs":true},
            {"candidate":6,"pipeline":RESEARCH,"pipeline_p":0.51,"draft_pipeline_p":0.2,
             "differs":true}
        ]})
    );
    // Positions, names of the catalogue and probabilities only: no text of the draft
    // or of the catalogue and no confidence travel back.
    let text = json!(field).to_string();
    for absent in [
        "Invitation form",
        "Delivery",
        "agent reason",
        "Fits",
        "Choose when",
        "confidence",
        "probabilities",
        "level",
    ] {
        assert!(!text.contains(absent), "{absent}");
    }
}

#[test]
fn the_note_is_one_sentence_that_explains_the_four_keys_and_says_what_to_do() {
    for part in [
        "Jev's",
        "1-based in draft order",
        "pipeline is",
        "pipeline_p",
        "draft_pipeline_p",
        "differs",
        "where it differs follow the advice by changing the draft and saving it again",
        "where the advice is plainly wrong give the reason in advice_departures of the review",
        "a review that says ready is refused until then",
    ] {
        assert!(PIPELINE_NOTE.contains(part), "{part}");
    }
    assert!(!PIPELINE_NOTE.contains("you decide"));
    assert_eq!(PIPELINE_NOTE.matches(". ").count(), 0, "one sentence");
    assert_eq!(PIPELINE_NOTE.matches('.').count(), 1, "one sentence");
    assert!(PIPELINE_NOTE.ends_with('.'));
    // It travels only inside an advised field.
    let advice = advice();
    assert_eq!(content(&advice.largest())["note"], PIPELINE_NOTE);
    let all = answers(&advice, |_| ((ADVICE_ABSTAIN, 0.9), (ADVICE_ABSTAIN, 0.9)));
    assert!(!json!(completed(&advice, all)).to_string().contains("note"));
}

#[test]
fn an_abstained_node_keeps_abstain_without_differing_and_only_all_abstain_abstains() {
    let advice = advice();
    let partly = answers(&advice, |name| match name {
        "c4.pipeline" => ((ADVICE_ABSTAIN, 0.55), (DEBUG, 0.2)),
        "c1.pipeline" => ((FULL, 0.6), (LIGHTWEIGHT, 0.3)),
        _ => ((FULL, 0.8), (FULL, 0.8)),
    });
    assert_eq!(
        content(&completed(&advice, partly))["candidates"],
        json!([
            {"candidate":1,"pipeline":FULL,"pipeline_p":0.6,"draft_pipeline_p":0.3,
             "differs":true},
            {"candidate":4,"pipeline":"ABSTAIN","pipeline_p":0.55,"draft_pipeline_p":0.2,
             "differs":false},
            {"candidate":6,"pipeline":FULL,"pipeline_p":0.8,"draft_pipeline_p":0.8,
             "differs":false}
        ])
    );
    let all = answers(&advice, |_| ((ADVICE_ABSTAIN, 0.9), (ADVICE_ABSTAIN, 0.9)));
    let field = completed(&advice, all);
    assert_eq!(field, AdviceField::Abstained);
    assert_eq!(json!(field), json!({"status":"abstained"}));

    // An answer that is not there is never guessed.
    let mut missing = answers(&advice, |_| ((FULL, 0.8), (FULL, 0.8)));
    missing.remove("c4.pipeline");
    let field = advice.shape(&missing);
    assert_eq!(
        field.reason(),
        Some(AdviceUnavailableReason::InvalidResponse)
    );
    assert!(json!(field).to_string().contains("`c4.pipeline`"));
    // Nor is a probability Jev did not give for the pipeline the draft chose.
    let mut partial = answers(&advice, |_| ((FULL, 0.8), (FULL, 0.8)));
    let answer = partial.get_mut("c1.pipeline").unwrap();
    answer.probabilities.remove(LIGHTWEIGHT);
    let field = advice.shape(&partial);
    assert_eq!(
        field.reason(),
        Some(AdviceUnavailableReason::InvalidResponse)
    );
    assert!(json!(field).to_string().contains("`c1.pipeline`"));
}

#[test]
fn no_shaped_field_is_larger_than_the_worst_case_sample() {
    let advice = advice();
    let largest = advice.largest();
    let sample = content(&largest);
    // The sample names every asked node with the longest name Jev may choose.
    let longest = criteria(&advice, "c1.pipeline")
        .into_iter()
        .max_by_key(String::len)
        .unwrap();
    assert_eq!(longest, "slice.promote-to-durable-knowledge");
    assert_eq!(
        sample,
        json!({"status":"advised","note":PIPELINE_NOTE,"candidates":[
            {"candidate":1,"pipeline":longest,"pipeline_p":0.99,"draft_pipeline_p":0.99,
             "differs":false},
            {"candidate":4,"pipeline":longest,"pipeline_p":0.99,"draft_pipeline_p":0.99,
             "differs":false},
            {"candidate":6,"pipeline":longest,"pipeline_p":0.99,"draft_pipeline_p":0.99,
             "differs":false}
        ]})
    );
    // One node at a time takes each criterion at each probability, with each share
    // for the pipeline the draft chose, while the others hold the longest name: every
    // shaped field stays within the sample.
    let names = advice.questions().questions.keys().collect::<Vec<_>>();
    let drafts = [LIGHTWEIGHT, DEBUG, FULL];
    let probabilities = [0.34, 0.5, 0.505, 0.6249, 0.985, 0.994, 0.995, 1.0];
    for (varied, draft) in names.iter().zip(drafts) {
        for choice in criteria(&advice, varied) {
            for probability in probabilities {
                for share in [0.0, 0.004, 0.005, 0.1234567, 1.0] {
                    let share = if choice == draft {
                        probability
                    } else {
                        f64::min(share, 1.0 - probability)
                    };
                    let given = names
                        .iter()
                        .map(|name| {
                            let given = if name == varied {
                                answer(&advice, name, (&choice, probability), (draft, share))
                            } else {
                                answer(&advice, name, (&longest, 0.99), (&longest, 0.99))
                            };
                            ((*name).clone(), given)
                        })
                        .collect();
                    let shaped = advice.shape(&given);
                    assert!(matches!(shaped, AdviceField::Advised { .. }));
                    assert!(
                        reply_bytes(&shaped) <= reply_bytes(&largest),
                        "{varied} {choice} {probability} {share}"
                    );
                    assert!(json!(shaped).to_string().len() <= sample.to_string().len());
                }
            }
        }
    }
}

#[test]
fn the_subject_counts_the_work_nodes_asked_about() {
    assert_eq!(
        advice().subject(),
        "the pipelines of this draft (work nodes asked about: 3)"
    );
}
