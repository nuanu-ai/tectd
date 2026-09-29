use super::*;

const ASSERTIONS: [(&str, &str); 4] = [
    ("broader_concept", "broaderConcept"),
    ("classified_as", "classifiedAs"),
    ("has_environment", "hasEnvironment"),
    ("applies_to", "appliesTo"),
];

pub(super) fn add(document: &mut Value) {
    document["graph_assertions"] = json!(
        ASSERTIONS
            .iter()
            .map(|(relation, _)| json!({
                "subject_iri":format!("urn:tect:dk3:graph:{relation}:subject"),
                "predicate":relation,
                "object_iri":format!("urn:tect:dk3:graph:{relation}:object")
            }))
            .collect::<Vec<_>>()
    );
}

pub(super) async fn verify(owner: &mut Mcp, rich_id: Uuid, revision_iri: &Value) {
    let v2 = "urn:tect:dk:v2:";
    for (relation, predicate) in ASSERTIONS {
        let subject = format!("urn:tect:dk3:graph:{relation}:subject");
        let object = format!("urn:tect:dk3:graph:{relation}:object");
        let path = [format!("{v2}{predicate}")];
        let incoming = assert_edge(owner, rich_id, &object, relation, &[&path[0]], None).await;
        let incoming_hop = hop_for(&incoming, rich_id);
        assert_eq!(incoming_hop["from_iri"], subject);
        assert_eq!(incoming_hop["to_iri"], object);
        assert_eq!(incoming_hop["supporting_unit_id"], rich_id.to_string());
        assert_eq!(incoming_hop["supporting_revision"], 1);
        assert_eq!(&incoming_hop["supporting_revision_iri"], revision_iri);
        let mut outgoing_params = graph_params(&subject, relation, None);
        outgoing_params["direction"] = json!("outgoing");
        let outgoing = search(owner, outgoing_params).await;
        assert!(result_ids(&outgoing).contains(&rich_id));
        let outgoing_hop = hop_for(&outgoing, rich_id);
        assert_eq!(outgoing_hop["traversed_in_reverse"], false);
        assert_eq!(outgoing_hop["predicate_path"], json!(path));
        let filtered = search(owner, graph_params(&object, "targets", None)).await;
        assert!(!result_ids(&filtered).contains(&rich_id));
    }
}
