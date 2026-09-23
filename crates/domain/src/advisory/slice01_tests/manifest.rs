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
