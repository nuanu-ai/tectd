use super::*;

#[test]
fn source_amendment_accepts_one_bounded_hash_valid_artifact() {
    assert_eq!(request("changed".to_owned()).validate(), Ok(()));
}

#[test]
fn source_amendment_rejects_empty_even_with_the_empty_sha256() {
    assert_source_amendment_refusal(
        request(String::new()).validate(),
        "WP6-SOURCE-AMENDMENT-BODY-BLANK",
        "successor.artifact.body",
    );
}

#[test]
fn source_amendment_rejects_unsafe_and_untrimmed_paths() {
    for path in ["../source.md", "/source.md", " source.md", "source.md "] {
        let mut value = request("changed".to_owned());
        let amendment = value.source_amendment.as_mut().unwrap();
        amendment.successor.path = path.to_owned();
        amendment.successor.artifact.name = path.to_owned();
        assert_source_amendment_refusal(
            value.validate(),
            "WP6-SOURCE-AMENDMENT-SUCCESSOR-PATH",
            "successor.path",
        );
    }
}

#[test]
fn source_amendment_rejects_oversized_body() {
    assert_source_amendment_refusal(
        request("x".repeat(MAX_PIPELINE_OUTPUT_BYTES + 1)).validate(),
        "WP6-SOURCE-AMENDMENT-BODY-SIZE",
        "successor.artifact.body",
    );
}

#[test]
fn source_amendment_rejects_path_name_mismatch() {
    let mut path = request("changed".to_owned());
    path.source_amendment
        .as_mut()
        .unwrap()
        .successor
        .artifact
        .name = "other.md".to_owned();
    assert_source_amendment_refusal(
        path.validate(),
        "WP6-SOURCE-AMENDMENT-PATH-NAME-MATCH",
        "successor.artifact.name",
    );
}

#[test]
fn source_amendment_leaves_digest_integrity_to_application() {
    let mut digest = request("changed".to_owned());
    digest
        .source_amendment
        .as_mut()
        .unwrap()
        .successor
        .artifact
        .digest = "0".repeat(64);
    assert_eq!(digest.validate(), Ok(()));
}

#[test]
fn missing_test_target_has_typed_refusal_predicate() {
    let phase = PipelinePhaseDefinition {
        id: "tdd".into(),
        ordinal: 1,
        title: "TDD".into(),
        required: true,
        disposition_required: false,
        instructions: vec![],
        skills: vec![],
        resources: vec![],
        required_artifacts: vec![],
        validator_contracts: vec![],
        followup_contracts: vec![],
        required_fields: vec!["selected_test_target".into()],
        allowed_verdicts: vec![],
        required_dispositions: vec![],
        allowed_dispositions: vec![],
        output_constraints: vec![],
        verdict_routes: vec![],
        allowed_backward_to: vec![],
        fresh_reviewer_input: false,
        retry_policy: PipelinePhaseRetryPolicy::Repeatable,
        output_contract: "target".into(),
    };
    let output = PipelinePhaseOutputDraft {
        body: "body".into(),
        producer_context_id: "ctx".into(),
        fields: BTreeMap::new(),
        verdict: None,
        dispositions: vec![],
        skill_reads: vec![],
        resource_reads: vec![],
        artifacts: vec![],
        evidence_artifacts: vec![],
        validator_receipts: vec![],
        followup_proposal: None,
        knowledge_publication: None,
        reviewer_context: None,
        reference: None,
    };
    assert!(missing_test_target(&phase, &output));
}
