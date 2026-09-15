use super::*;

#[test]
fn planning_briefs_are_exact_native_rdf_and_change_the_payload() {
    let fixture: CorpusFixture = serde_json::from_str(CORPUS[7]).unwrap();
    let first = build(&corpus_input(&fixture, 20)).unwrap();
    for field in [
        "planningBrief",
        "localId",
        "stage",
        "instruction",
        "purpose",
        "condition",
        "exception",
        "targetSelector",
        "environmentSelector",
        "actionClassSelector",
    ] {
        assert!(first.payload.contains(&format!("{V2}{field}")), "{field}");
    }
    let mut changed = fixture;
    changed.document.planning_briefs[0]
        .instruction
        .push_str(" Changed.");
    let second = build(&corpus_input(&changed, 20)).unwrap();
    assert_ne!(first.payload, second.payload);
}

#[test]
fn historical_empty_planning_briefs_preserve_the_pre_dk3_graph() {
    let mut historical = input();
    historical.include_empty_planning_briefs = false;
    assert!(
        historical
            .planned
            .document
            .as_ref()
            .unwrap()
            .planning_briefs
            .is_empty()
    );
    let historical_graph = build(&historical).unwrap();
    assert!(!historical_graph.payload.contains("planningBriefs"));

    historical.include_empty_planning_briefs = true;
    let current_graph = build(&historical).unwrap();
    assert!(current_graph.payload.contains("planningBriefs"));

    let mut with_briefs = corpus_input(&serde_json::from_str(CORPUS[7]).unwrap(), 21);
    with_briefs.include_empty_planning_briefs = false;
    assert!(
        build(&with_briefs)
            .unwrap()
            .payload
            .contains("planningBriefs")
    );
}
