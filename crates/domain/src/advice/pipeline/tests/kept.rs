//! What is kept of Jev's answers for the review of the draft: one answer per asked
//! work node, which asks for a change when Jev chose another pipeline than the draft.
use super::*;

#[test]
fn a_node_whose_pipeline_jev_chose_otherwise_asks_for_a_change() {
    let existing = Existing::new();
    let stored = saved(&existing);
    let advice = PipelineDraft::slice_candidates(
        &draft(&existing),
        &stored,
        &catalogue(),
        scope_statement(),
    )
    .unwrap()
    .unwrap()
    .advice(&words(), ProgramMode::Prod);
    // The draft chose Lightweight for node 1, Debug for node 4 and Full for node 6.
    let given = answers(&advice, |name| match name {
        // Jev agrees.
        "c1.pipeline" => ((LIGHTWEIGHT, 0.74), (LIGHTWEIGHT, 0.74)),
        // Jev chooses another pipeline.
        "c4.pipeline" => ((LIGHTWEIGHT, 0.74), (DEBUG, 0.12)),
        // Jev abstains: no choice, so nothing to follow.
        "c6.pipeline" => ((ADVICE_ABSTAIN, 0.6), (FULL, 0.2)),
        other => panic!("unexpected question {other}"),
    });
    let kept = advice.kept(&given);
    let field = json!(advice.shape(&given));
    let node = |position: usize| {
        (
            stored.nodes[position - 1].id(),
            stored.nodes[position - 1].revision(),
        )
    };
    assert_eq!(
        kept.iter()
            .map(|kept| (
                (kept.candidate_id, kept.candidate_revision),
                kept.asks_change
            ))
            .collect::<Vec<_>>(),
        [(node(1), false), (node(4), true), (node(6), false)]
    );
    // The changed node is kept under the revision the save wrote, not the one named.
    assert_eq!(node(4), (existing.changed, 3));
    for (index, kept) in kept.iter().enumerate() {
        assert_eq!(kept.level, SizingLevel::ScopeToSlices);
        assert_eq!(kept.answer, field["candidates"][index]);
        assert_eq!(kept.asks_change, kept.answer["differs"]);
    }
    assert_eq!(
        kept[1].answer,
        json!({"candidate":4,"pipeline":LIGHTWEIGHT,"pipeline_p":0.74,"draft_pipeline_p":0.12,
            "differs":true})
    );

    // Jev abstained on every node: nothing is kept.
    let reply = AdviceReply {
        answers: answers(&advice, |_| ((ADVICE_ABSTAIN, 0.6), (LIGHTWEIGHT, 0.1))),
        exchange: AdviceExchange::default(),
    };
    let completed = AdviceCompletion::from_provider(Ok(reply), &advice);
    assert_eq!(completed.advice, AdviceField::Abstained);
    assert!(completed.kept.is_empty());
}
