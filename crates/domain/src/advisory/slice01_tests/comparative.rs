use super::super::scope_source::canonical_digest;
use super::fixtures::{answers, digest, fixture_manifest};
use crate::*;

#[test]
fn comparative_selection_leads_even_when_rounded_scores_tie() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let mut normalized = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::Fit],
    );
    let selected = manifest.emitted[1].id.clone();
    normalized.answers[0].choice = ScopeAdviceChoice::NonPreferred;
    normalized.comparative_disposition = Some(ComparativeDisposition::Selected(selected.clone()));
    let advice = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    assert_eq!(advice.ranked_ids[0], selected);
    assert_eq!(advice.ranked_ids[1], manifest.emitted[0].id);
    assert_eq!(
        advice.comparative_disposition,
        normalized.comparative_disposition
    );
    validate_guarded_advice_binding(&digest(), &manifest, &advice).unwrap();
    let mut tampered = advice.clone();
    tampered.ranked_ids.swap(0, 1);
    assert!(validate_guarded_advice_binding(&digest(), &manifest, &tampered).is_err());
    let mut tampered = advice;
    tampered.comparative_disposition = Some(ComparativeDisposition::Abstain);
    assert!(validate_guarded_advice_binding(&digest(), &manifest, &tampered).is_err());
}

#[test]
fn comparative_abstain_has_no_ranking_but_keeps_scores() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let mut normalized = answers(
        &manifest,
        [
            ScopeAdviceScoreBand::StrongFit,
            ScopeAdviceScoreBand::WeakFit,
        ],
    );
    for answer in &mut normalized.answers {
        answer.choice = ScopeAdviceChoice::NonPreferred;
    }
    normalized.comparative_disposition = Some(ComparativeDisposition::Abstain);
    let advice = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    assert!(advice.ranked_ids.is_empty());
    assert_eq!(advice.items.len(), 2);
    assert_eq!(
        advice.comparative_disposition,
        Some(ComparativeDisposition::Abstain)
    );
    validate_guarded_advice_binding(&digest(), &manifest, &advice).unwrap();
}

#[test]
fn comparative_disposition_rejects_unknown_or_choice_mismatch() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let mut normalized = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::Fit],
    );
    normalized.answers[0].choice = ScopeAdviceChoice::NonPreferred;
    normalized.comparative_disposition = Some(ComparativeDisposition::Selected(
        ScopeAlternativeId("a".repeat(64)),
    ));
    assert!(
        guard_scope_advice(
            &digest(),
            uuid::Uuid::from_u128(200),
            &manifest,
            &request,
            &normalized
        )
        .is_err()
    );
    normalized.comparative_disposition = Some(ComparativeDisposition::Selected(
        manifest.emitted[0].id.clone(),
    ));
    assert!(
        guard_scope_advice(
            &digest(),
            uuid::Uuid::from_u128(200),
            &manifest,
            &request,
            &normalized
        )
        .is_err()
    );
    normalized.comparative_disposition = Some(ComparativeDisposition::Abstain);
    assert!(
        guard_scope_advice(
            &digest(),
            uuid::Uuid::from_u128(200),
            &manifest,
            &request,
            &normalized
        )
        .is_err()
    );
}

#[test]
fn legacy_all_nonpreferred_retains_score_order_and_old_serialized_shape() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let mut normalized = answers(
        &manifest,
        [
            ScopeAdviceScoreBand::WeakFit,
            ScopeAdviceScoreBand::StrongFit,
        ],
    );
    for answer in &mut normalized.answers {
        answer.choice = ScopeAdviceChoice::NonPreferred;
    }
    let advice = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    assert_eq!(advice.ranked_ids[0], manifest.emitted[1].id);
    assert!(
        serde_json::to_value(&normalized)
            .unwrap()
            .get("comparative_disposition")
            .is_none()
    );
    assert!(
        serde_json::to_value(&advice)
            .unwrap()
            .get("comparative_disposition")
            .is_none()
    );
    validate_guarded_advice_binding(&digest(), &manifest, &advice).unwrap();
}

#[test]
fn none_digest_matches_historical_canonical_reference() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let normalized = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::WeakFit],
    );
    let advice = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    let mut canonical = normalized.answers.clone();
    canonical.sort_by(|left, right| left.alternative_id.cmp(&right.alternative_id));
    let old_digest = canonical_digest(
        &digest(),
        "tect.normalized-scope-advice-answers/1",
        &canonical,
    )
    .unwrap();
    assert_eq!(advice.normalized_answers_digest, old_digest);
    let old_content = canonical_digest(
        &digest(),
        "tect.guarded-scope-advice/1",
        &(
            &request.digest,
            &manifest.whole_set_digest,
            &manifest.eligible_set_digest,
            &old_digest,
        ),
    )
    .unwrap();
    assert_eq!(advice.content_digest(&digest()).unwrap(), old_content);
    let old_id = canonical_digest(
        &digest(),
        "tect.guarded-scope-advice-occurrence/1",
        &(uuid::Uuid::from_u128(200), &old_content),
    )
    .unwrap();
    assert_eq!(advice.id.0, old_id);
    assert_eq!(advice.ranked_ids[0], manifest.emitted[0].id);
    let payload = serde_json::to_value(&advice).unwrap();
    assert!(payload.get("comparative_disposition").is_none());
    let restored: GuardedScopeAdvice = serde_json::from_value(payload).unwrap();
    assert_eq!(restored, advice);
    validate_guarded_advice_binding(&digest(), &manifest, &restored).unwrap();
}

#[test]
fn selected_requires_single_matching_preferred_but_does_not_compare_score_bands() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let mut normalized = answers(
        &manifest,
        [
            ScopeAdviceScoreBand::StrongFit,
            ScopeAdviceScoreBand::WeakFit,
        ],
    );
    normalized.comparative_disposition = Some(ComparativeDisposition::Selected(
        manifest.emitted[1].id.clone(),
    ));
    assert!(
        guard_scope_advice(
            &digest(),
            uuid::Uuid::from_u128(200),
            &manifest,
            &request,
            &normalized
        )
        .is_err()
    );
    normalized.answers[0].choice = ScopeAdviceChoice::NonPreferred;
    let advice = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    assert_eq!(advice.ranked_ids[0], manifest.emitted[1].id);
    assert_eq!(advice.items[0].alternative_id, manifest.emitted[0].id);
    validate_guarded_advice_binding(&digest(), &manifest, &advice).unwrap();
    normalized.comparative_disposition = Some(ComparativeDisposition::Selected(
        ScopeAlternativeId("invalid".into()),
    ));
    assert!(
        guard_scope_advice(
            &digest(),
            uuid::Uuid::from_u128(200),
            &manifest,
            &request,
            &normalized
        )
        .is_err()
    );
    normalized.answers[1].choice = ScopeAdviceChoice::NonPreferred;
    normalized.comparative_disposition = Some(ComparativeDisposition::Selected(
        manifest.emitted[1].id.clone(),
    ));
    assert!(
        guard_scope_advice(
            &digest(),
            uuid::Uuid::from_u128(200),
            &manifest,
            &request,
            &normalized
        )
        .is_err()
    );
}

#[test]
fn disposition_is_digest_bound_even_when_answer_choices_remain_consistent() {
    let manifest = fixture_manifest();
    let request = ScopeAdviceRequest::from_manifest(&digest(), &manifest).unwrap();
    let mut normalized = answers(
        &manifest,
        [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::WeakFit],
    );
    for answer in &mut normalized.answers {
        answer.choice = ScopeAdviceChoice::NonPreferred;
    }
    let legacy = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    normalized.comparative_disposition = Some(ComparativeDisposition::Abstain);
    let abstained = guard_scope_advice(
        &digest(),
        uuid::Uuid::from_u128(200),
        &manifest,
        &request,
        &normalized,
    )
    .unwrap();
    assert_ne!(
        legacy.normalized_answers_digest,
        abstained.normalized_answers_digest
    );
    assert_ne!(legacy.id, abstained.id);
    let mut tampered = legacy;
    tampered.comparative_disposition = Some(ComparativeDisposition::Abstain);
    tampered.ranked_ids.clear();
    assert!(validate_guarded_advice_binding(&digest(), &manifest, &tampered).is_err());
    let mut tampered = abstained;
    tampered.comparative_disposition = None;
    assert!(validate_guarded_advice_binding(&digest(), &manifest, &tampered).is_err());
}
