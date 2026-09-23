use super::fixtures::{answers, digest, fixture_manifest};
use crate::*;

#[test]
fn request_is_provider_neutral_and_exactly_bound_to_manifest() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    assert_eq!(request.questions.len(), manifest.emitted.len());
    assert_eq!(request.manifest_digest, manifest.whole_set_digest);
    assert_eq!(request.eligible_set_digest, manifest.eligible_set_digest);
    let text = serde_json::to_string(&request).unwrap();
    for forbidden in [
        "model",
        "legend",
        "probabilities",
        "raw",
        "usage",
        "provider",
    ] {
        assert!(!text.contains(forbidden));
    }
    let mut changed = request.clone();
    changed.eligible_set_digest = "a".repeat(64);
    assert!(changed.validate(&digest(), &manifest).is_err());
}

#[test]
fn normalized_answers_reject_duplicates_missing_unknown_and_bad_confidence() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let valid = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::WeakFit],
    );
    guard_scope_advice(&digest(), &manifest, &request, &valid).unwrap();

    let mut duplicate = valid.clone();
    duplicate.answers[1].alternative_id = duplicate.answers[0].alternative_id.clone();
    assert!(guard_scope_advice(&digest(), &manifest, &request, &duplicate).is_err());
    let mut missing = valid.clone();
    missing.answers.pop();
    assert!(guard_scope_advice(&digest(), &manifest, &request, &missing).is_err());
    let mut unknown = valid.clone();
    unknown.answers[0].alternative_id = ScopeAlternativeId("a".repeat(64));
    assert!(guard_scope_advice(&digest(), &manifest, &request, &unknown).is_err());
    let mut bad_confidence = valid;
    bad_confidence.answers[0].score_confidence = ConfidenceBasisPoints(10_001);
    assert!(guard_scope_advice(&digest(), &manifest, &request, &bad_confidence).is_err());
}

#[test]
fn ranking_uses_discrete_band_then_stable_id_and_identity_is_canonical() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let tied = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::Fit],
    );
    let advice = guard_scope_advice(&digest(), &manifest, &request, &tied).unwrap();
    let mut expected = manifest
        .emitted
        .iter()
        .map(|value| value.id.clone())
        .collect::<Vec<_>>();
    expected.sort();
    assert_eq!(advice.ranked_ids, expected);

    let mut reversed = tied;
    reversed.answers.reverse();
    let same = guard_scope_advice(&digest(), &manifest, &request, &reversed).unwrap();
    assert_eq!(advice.id, same.id);
    assert_eq!(
        advice.normalized_answers_digest,
        same.normalized_answers_digest
    );

    let ranked = answers(
        &manifest,
        [
            ScopeAdviceScoreBand::WeakFit,
            ScopeAdviceScoreBand::StrongFit,
        ],
    );
    let advice = guard_scope_advice(&digest(), &manifest, &request, &ranked).unwrap();
    assert_eq!(advice.ranked_ids[0], manifest.emitted[1].id);
    assert_eq!(ScopeAdviceScoreBand::StrongFit.ordinal(), 3);
}

#[test]
fn guarded_identity_rejects_tampering() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let normalized = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::WeakFit],
    );
    let mut advice = guard_scope_advice(&digest(), &manifest, &request, &normalized).unwrap();
    advice.id.0 = "b".repeat(64);
    assert!(validate_guarded_advice_binding(&digest(), &manifest, &advice).is_err());
}
