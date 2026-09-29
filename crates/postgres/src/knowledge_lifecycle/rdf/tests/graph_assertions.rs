use super::*;

#[test]
fn graph_assertions_publish_four_direct_iri_triples() {
    let mut value = input();
    let assertions = [
        (
            "urn:concept:child",
            KnowledgeGraphPredicate::BroaderConcept,
            "urn:concept:parent",
        ),
        (
            "urn:resource:one",
            KnowledgeGraphPredicate::ClassifiedAs,
            "urn:class:one",
        ),
        (
            "urn:resource:one",
            KnowledgeGraphPredicate::HasEnvironment,
            "urn:env:prod",
        ),
        (
            "urn:rule:one",
            KnowledgeGraphPredicate::AppliesTo,
            "urn:target:one",
        ),
    ];
    value.planned.document.as_mut().unwrap().graph_assertions = assertions
        .iter()
        .rev()
        .map(
            |(subject_iri, predicate, object_iri)| KnowledgeGraphAssertion {
                subject_iri: (*subject_iri).into(),
                predicate: *predicate,
                object_iri: (*object_iri).into(),
            },
        )
        .collect();

    let encoded = build(&value).unwrap();
    for (subject, predicate, object) in assertions {
        let line = format!("<{subject}> <{}> <{object}> .", predicate.iri());
        assert_eq!(encoded.payload.matches(&line).count(), 1, "{line}");
    }
    assert_eq!(validate_rows(&rows(&encoded), &encoded), Ok(()));

    value
        .planned
        .document
        .as_mut()
        .unwrap()
        .graph_assertions
        .reverse();
    assert_eq!(build(&value).unwrap().payload, encoded.payload);
}
