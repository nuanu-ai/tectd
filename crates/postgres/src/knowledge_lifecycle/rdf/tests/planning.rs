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
