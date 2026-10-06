use super::*;

#[tokio::test]
async fn required_enabler_duplicate_and_unknown_stay_source_relative() {
    let mut cases = Vec::new();
    let baseline = input(true);
    cases.push((baseline.clone(), AntiBloatClass::UnsupportedMechanism, true));
    let mut enabler = baseline.clone();
    enabler.manifest.emitted[0].material.candidates[0]
        .dependencies
        .push(Uuid::from_u128(70));
    cases.push((refresh(enabler), AntiBloatClass::NecessaryEnabler, false));
    let mut duplicate = baseline.clone();
    let required = duplicate.manifest.emitted[0].material.candidates[0].clone();
    let extra = &mut duplicate.manifest.emitted[0].material.candidates[1];
    extra.outcome = required.outcome;
    extra.trigger = required.trigger;
    extra.delivered_behavior = required.delivered_behavior;
    extra.proof = required.proof;
    cases.push((refresh(duplicate), AntiBloatClass::Duplicate, true));
    let mut unknown = baseline;
    let evidence_id = Uuid::from_u128(72);
    unknown.manifest.emitted[0]
        .material
        .evidence
        .push(EvidenceEntity {
            id: evidence_id,
            revision: 1,
            kind: EvidenceKind::VerifiedEvidence,
            summary: "Relevant prior evidence".into(),
            source_ref_id: Uuid::from_u128(50),
            authority_input_sequence: None,
        });
    unknown.manifest.emitted[0].material.candidates[1]
        .evidence_ids
        .push(evidence_id);
    cases.push((refresh(unknown), AntiBloatClass::Unknown, false));
    for (source, class, rankable) in cases {
        let mut app = app(true, false);
        app.store.input = Some(source);
        let saved = prepare(
            &mut app,
            WorkspaceAdvisoryMode::Optional,
            AdvisoryRequestPreference::UseWorkspace,
        )
        .await;
        let finding = saved
            .review
            .findings
            .iter()
            .find(|finding| finding.candidate_id == Uuid::from_u128(70))
            .unwrap();
        assert_eq!((finding.class, finding.rankable), (class, rankable));
    }
}
