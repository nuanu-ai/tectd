use super::fixtures::{digest, fixture_manifest};
use crate::*;

#[test]
fn supplied_manifest_is_deterministic_and_contains_no_constructor_algorithm() {
    let manifest = fixture_manifest();
    manifest.validate(&digest()).unwrap();
    assert_eq!(
        manifest.source.canonical_digest(&digest()).unwrap(),
        manifest.source.digest
    );
    assert_eq!(
        manifest.canonical_eligible_set_digest(&digest()).unwrap(),
        manifest.eligible_set_digest
    );
    assert_eq!(
        manifest.canonical_whole_set_digest(&digest()).unwrap(),
        manifest.whole_set_digest
    );
    let first = &manifest.emitted[0];
    assert_eq!(
        stable_scope_alternative_id(
            &digest(),
            &manifest.constructor,
            &manifest.source.digest,
            first.kind,
            &first.material_digest,
            &first.coverage
        )
        .unwrap(),
        first.id
    );
}

#[test]
fn resolved_material_uses_version_two_digest_domain() {
    let manifest = fixture_manifest();
    let material = &manifest.emitted[0].material;
    let bytes = serde_json::to_vec(material).unwrap();
    assert_eq!(
        scope_candidate_material_digest(&digest(), material).unwrap(),
        digest().sha256("tect.scope-candidate-material/2", &bytes)
    );
    assert_ne!(
        scope_candidate_material_digest(&digest(), material).unwrap(),
        digest().sha256("tect.scope-candidate-material/1", &bytes)
    );
}

#[test]
fn missing_obligation_condition_exception_or_coverage_fails_closed() {
    let mut manifest = fixture_manifest();
    manifest.emitted[0].coverage.clear();
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.emitted[0].coverage[0].condition_ids.clear();
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.emitted[0].coverage[0].exception_ids.clear();
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.emitted[0].coverage.push(ObligationCoverage {
        obligation_id: "obligation.unknown".into(),
        condition_ids: Vec::new(),
        exception_ids: Vec::new(),
    });
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.obligations.push(manifest.obligations[0].clone());
    assert!(manifest.validate(&digest()).is_err());
}

#[test]
fn duplicate_alternatives_bad_order_and_unknown_gap_conflict_are_rejected() {
    let mut manifest = fixture_manifest();
    manifest.emitted.push(manifest.emitted[0].clone());
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.ordered_ids.reverse();
    assert!(manifest.validate(&digest()).is_err());

    for applicability in [
        SourceApplicability::Unknown,
        SourceApplicability::Gap,
        SourceApplicability::Conflict,
        SourceApplicability::Invalid,
    ] {
        let mut manifest = fixture_manifest();
        manifest.source.inputs[0].applicability = applicability;
        assert_eq!(manifest.validate(&digest()), Err(Error::InvalidSource));
    }
}

#[test]
fn distinct_ids_cannot_represent_identical_saved_material() {
    let mut manifest = fixture_manifest();
    let material = manifest.emitted[0].material.clone();
    let digest_value = manifest.emitted[0].material_digest.clone();
    let other = &mut manifest.emitted[1];
    other.material = material;
    other.material_digest = digest_value;
    other.id = stable_scope_alternative_id(
        &digest(),
        &manifest.constructor,
        &manifest.source.digest,
        other.kind,
        &other.material_digest,
        &other.coverage,
    )
    .unwrap();
    manifest
        .emitted
        .sort_by(|left, right| left.id.cmp(&right.id));
    manifest.ordered_ids = manifest
        .emitted
        .iter()
        .map(|value| value.id.clone())
        .collect();
    manifest.baseline_id = manifest.emitted[0].id.clone();
    manifest.eligible_set_digest = manifest.canonical_eligible_set_digest(&digest()).unwrap();
    manifest.whole_set_digest = manifest.canonical_whole_set_digest(&digest()).unwrap();
    assert!(manifest.validate(&digest()).is_err());
}

#[test]
fn alternative_material_requires_resolved_saved_identities_and_complete_links() {
    let mut manifest = fixture_manifest();
    manifest.emitted[0].material.goals[0].resolution.id = uuid::Uuid::from_u128(999);
    assert!(manifest.validate(&digest()).is_err());

    let manifest = fixture_manifest();
    let mut wire = serde_json::to_value(&manifest.emitted[0]).unwrap();
    wire["material"]["candidates"][0]["identity"] = serde_json::json!({"local": "candidate"});
    assert!(serde_json::from_value::<ScopeDecompositionAlternative>(wire).is_err());
}

#[test]
fn resolved_coverage_rejects_evidence_or_other_candidate_resolutions() {
    let mut manifest = fixture_manifest();
    manifest.emitted[0].material.goals[0].resolution.kind = CoverageResolutionKind::Evidence;
    manifest.emitted[0].material.goals[0].resolution.id = uuid::Uuid::from_u128(60);
    manifest.emitted[0].material.evidence.push(EvidenceEntity {
        id: uuid::Uuid::from_u128(60),
        revision: 1,
        kind: EvidenceKind::VerifiedEvidence,
        summary: "Evidence".into(),
        source_ref_id: uuid::Uuid::from_u128(50),
        authority_input_sequence: None,
    });
    assert!(manifest.emitted[0].material.validate().is_err());

    let mut manifest = fixture_manifest();
    let second = uuid::Uuid::from_u128(61);
    let mut candidate = manifest.emitted[0].material.candidates[0].clone();
    candidate.id = second;
    manifest.emitted[0].material.candidates.push(candidate);
    manifest.emitted[0]
        .material
        .delta
        .added
        .push(CandidateAdded {
            candidate_id: second,
            revision: 1,
        });
    manifest.emitted[0].material.goals[0].resolution.id = second;
    assert!(manifest.emitted[0].material.validate().is_err());
}

#[test]
fn resolved_delta_rejects_missing_duplicate_and_invalid_prior() {
    let mut manifest = fixture_manifest();
    manifest.emitted[0].material.delta.added.clear();
    assert!(manifest.emitted[0].material.validate().is_err());

    let mut manifest = fixture_manifest();
    let added = manifest.emitted[0].material.delta.added[0].clone();
    manifest.emitted[0].material.delta.added.push(added);
    assert!(manifest.emitted[0].material.validate().is_err());

    let mut manifest = fixture_manifest();
    let prior = manifest.emitted[0].material.candidates[0].clone();
    manifest.emitted[0]
        .material
        .delta
        .superseded
        .push(CandidateSuperseded {
            prior,
            reason: "Superseded".into(),
            replacement_candidate_ids: vec![],
        });
    assert!(manifest.emitted[0].material.validate().is_err());

    let mut manifest = fixture_manifest();
    manifest.emitted[0].material.delta.added.clear();
    let candidate_id = manifest.emitted[0].material.candidates[0].id;
    manifest.emitted[0]
        .material
        .delta
        .changed
        .push(CandidateChanged {
            candidate_id,
            from_revision: 1,
            to_revision: 1,
            rationale: "Reason".into(),
        });
    assert!(manifest.emitted[0].material.validate().is_err());
}

#[test]
fn resolved_delta_rejects_more_than_one_hundred_supersessions() {
    let mut manifest = fixture_manifest();
    let prior = manifest.emitted[0].material.candidates[0].clone();
    manifest.emitted[0].material.delta.superseded = (0..101)
        .map(|index| {
            let mut prior = prior.clone();
            prior.id = uuid::Uuid::from_u128(1_000 + index);
            CandidateSuperseded {
                prior,
                reason: "Superseded".into(),
                replacement_candidate_ids: vec![],
            }
        })
        .collect();
    assert!(manifest.emitted[0].material.validate().is_err());
}

#[test]
fn missing_or_changed_digests_and_invalid_supplied_draft_are_rejected() {
    let mut manifest = fixture_manifest();
    manifest.source.digest = "0".repeat(64);
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.emitted[0].material.candidates[0].title.clear();
    assert!(manifest.validate(&digest()).is_err());

    let mut manifest = fixture_manifest();
    manifest.eligible_set_digest = "0".repeat(64);
    assert!(manifest.validate(&digest()).is_err());
}

#[test]
fn alternative_identity_canonicalizes_coverage_condition_and_exception_sets() {
    let manifest = fixture_manifest();
    let alternative = &manifest.emitted[0];
    let mut first = vec![
        ObligationCoverage {
            obligation_id: "obligation.b".into(),
            condition_ids: vec!["condition.b".into(), "condition.a".into()],
            exception_ids: vec!["exception.b".into(), "exception.a".into()],
        },
        alternative.coverage[0].clone(),
    ];
    let mut second = first.clone();
    second.reverse();
    second[1].condition_ids.reverse();
    second[1].exception_ids.reverse();
    assert_eq!(
        stable_scope_alternative_id(
            &digest(),
            &manifest.constructor,
            &manifest.source.digest,
            alternative.kind,
            &alternative.material_digest,
            &first,
        )
        .unwrap(),
        stable_scope_alternative_id(
            &digest(),
            &manifest.constructor,
            &manifest.source.digest,
            alternative.kind,
            &alternative.material_digest,
            &second,
        )
        .unwrap()
    );

    first[0].condition_ids.push("condition.a".into());
    let mut invalid = fixture_manifest();
    invalid.emitted[0].coverage[0]
        .condition_ids
        .push("condition.a".into());
    assert!(invalid.validate(&digest()).is_err());
}
