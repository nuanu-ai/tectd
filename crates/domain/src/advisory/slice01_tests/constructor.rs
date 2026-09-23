use super::fixtures::{digest, fixture_manifest};
use crate::*;

fn authored_input() -> BuildSourceAuthoredScopeManifest {
    let manifest = fixture_manifest();
    let alternatives = manifest
        .emitted
        .into_iter()
        .map(|alternative| SourceAuthoredScopeAlternative {
            key: match alternative.kind {
                ScopeDecompositionKind::Cohesive => "cohesive".into(),
                ScopeDecompositionKind::Partitioned => "partitioned".into(),
            },
            kind: alternative.kind,
            material: alternative.material,
            coverage: alternative.coverage,
        })
        .collect();
    BuildSourceAuthoredScopeManifest {
        constructor: manifest.constructor,
        source: manifest.source,
        obligations: manifest.obligations,
        alternatives,
        baseline_key: "cohesive".into(),
    }
}

#[test]
fn source_authored_constructor_binds_supplied_material_and_baseline_deterministically() {
    let input = authored_input();
    let first = build_source_authored_scope_manifest(&digest(), input.clone()).unwrap();
    let mut reordered = input.clone();
    reordered.alternatives.reverse();
    for alternative in &mut reordered.alternatives {
        alternative.coverage.reverse();
        for row in &mut alternative.coverage {
            row.condition_ids.reverse();
            row.exception_ids.reverse();
        }
    }
    let second = build_source_authored_scope_manifest(&digest(), reordered).unwrap();

    assert_eq!(first, second);
    assert_eq!(first.constructor, input.constructor);
    assert_eq!(first.source, input.source);
    for authored in &input.alternatives {
        let resolved = first
            .emitted
            .iter()
            .find(|value| value.kind == authored.kind)
            .unwrap();
        assert_eq!(resolved.material, authored.material);
        assert_eq!(resolved.coverage, authored.coverage);
    }
    assert_eq!(
        first.eligible(&first.baseline_id).unwrap().kind,
        ScopeDecompositionKind::Cohesive
    );
    first.validate(&digest()).unwrap();
}

#[test]
fn source_authored_constructor_requires_baseline_and_accepts_one_alternative() {
    let mut input = authored_input();
    input.baseline_key = "missing".into();
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidArguments)
    );

    let mut input = authored_input();
    input.alternatives.truncate(1);
    input.baseline_key = input.alternatives[0].key.clone();
    let manifest = build_source_authored_scope_manifest(&digest(), input).unwrap();
    assert_eq!(manifest.emitted.len(), 1);
    assert_eq!(manifest.baseline_id, manifest.emitted[0].id);

    let mut input = authored_input();
    input.alternatives.clear();
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidArguments)
    );

    let mut input = authored_input();
    let exemplar = input.alternatives[0].clone();
    input.alternatives = (0..101)
        .map(|index| {
            let mut alternative = exemplar.clone();
            alternative.key = format!("candidate-{index}");
            alternative
        })
        .collect();
    input.baseline_key = "candidate-0".into();
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn source_authored_constructor_rejects_incomplete_or_duplicate_obligation_coverage() {
    let mut input = authored_input();
    input.alternatives[0].coverage.clear();
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidSource)
    );

    let mut input = authored_input();
    input.alternatives[0].coverage[0]
        .condition_ids
        .push("condition.a".into());
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidArguments)
    );

    let mut input = authored_input();
    input.alternatives[1].key = input.alternatives[0].key.clone();
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn source_authored_constructor_requires_explicit_valid_constructor_identity() {
    let mut input = authored_input();
    input.constructor.digest = "A".repeat(64);
    assert_eq!(
        build_source_authored_scope_manifest(&digest(), input),
        Err(Error::InvalidArguments)
    );
}
