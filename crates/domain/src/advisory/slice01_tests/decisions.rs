use super::fixtures::{digest, fixture_manifest, guarded};
use crate::*;
use uuid::Uuid;

fn request(
    manifest: &ScopeConstructorManifest,
    advice: &GuardedScopeAdvice,
    expected_revision: i64,
) -> ScopeDispositionRequest {
    let selected_id = manifest.emitted[0].id.clone();
    ScopeDispositionRequest {
        request_id: Uuid::from_u128(100),
        advice_id: advice.id.clone(),
        expected_revision,
        action: ScopeDispositionAction::Accept,
        selected_id: Some(selected_id.clone()),
        items: manifest
            .emitted
            .iter()
            .map(|value| ScopeDispositionItem {
                state: if value.id == selected_id {
                    ScopeDispositionItemState::Selected
                } else {
                    ScopeDispositionItemState::NotSelected
                },
                alternative_id: value.id.clone(),
            })
            .collect(),
        rationale: "Agent selected one supplied eligible alternative".into(),
    }
}

#[test]
fn disposition_requires_complete_set_and_compare_and_swap_shape() {
    let manifest = fixture_manifest();
    let advice = guarded(&manifest);
    let first = request(&manifest, &advice, 0);
    first.validate(&digest(), &manifest, &advice, None).unwrap();
    let revision = first
        .into_revision(Uuid::from_u128(102), &digest(), &manifest, &advice, None)
        .unwrap();
    assert_eq!(revision.revision, 1);
    assert_eq!(revision.supersedes_id, None);

    let mut incomplete = request(&manifest, &advice, 0);
    incomplete.items.pop();
    assert!(
        incomplete
            .validate(&digest(), &manifest, &advice, None)
            .is_err()
    );

    let mut duplicate = request(&manifest, &advice, 0);
    duplicate.items.push(duplicate.items[0].clone());
    assert!(
        duplicate
            .validate(&digest(), &manifest, &advice, None)
            .is_err()
    );

    let stale = request(&manifest, &advice, 0);
    assert_eq!(
        stale.validate(&digest(), &manifest, &advice, Some(&revision)),
        Err(Error::StaleRevision)
    );

    let mut replay = request(&manifest, &advice, 1);
    replay.request_id = revision.request_id;
    assert!(
        replay
            .validate(&digest(), &manifest, &advice, Some(&revision))
            .is_err()
    );

    let mut second = request(&manifest, &advice, 1);
    second.request_id = Uuid::from_u128(104);
    second
        .validate(&digest(), &manifest, &advice, Some(&revision))
        .unwrap();
    let second = second
        .into_revision(
            Uuid::from_u128(103),
            &digest(),
            &manifest,
            &advice,
            Some(&revision),
        )
        .unwrap();
    assert_eq!(second.supersedes_id, Some(revision.id));
}

#[test]
fn reject_all_and_baseline_supersession_have_exact_cardinality() {
    let manifest = fixture_manifest();
    let advice = guarded(&manifest);
    let mut rejected = request(&manifest, &advice, 0);
    rejected.action = ScopeDispositionAction::RejectAll;
    rejected.selected_id = None;
    for item in &mut rejected.items {
        item.state = ScopeDispositionItemState::NotSelected;
    }
    rejected
        .validate(&digest(), &manifest, &advice, None)
        .unwrap();

    let mut superseded = request(&manifest, &advice, 0);
    superseded.action = ScopeDispositionAction::SupersedeWithDeterministicChoice;
    superseded.selected_id = Some(manifest.baseline_id.clone());
    for item in &mut superseded.items {
        item.state = if item.alternative_id == manifest.baseline_id {
            ScopeDispositionItemState::Selected
        } else {
            ScopeDispositionItemState::NotSelected
        };
    }
    superseded
        .validate(&digest(), &manifest, &advice, None)
        .unwrap();
}

#[test]
fn preservation_rechecks_source_and_set_but_grants_no_effect_authority() {
    let manifest = fixture_manifest();
    let advice = guarded(&manifest);
    let disposition = request(&manifest, &advice, 0)
        .into_revision(Uuid::from_u128(102), &digest(), &manifest, &advice, None)
        .unwrap();
    let mut observation = FreshScopeObservation {
        source: manifest.source.clone(),
        manifest: manifest.clone(),
        candidate_set_revision: manifest.source.candidate_set_revision,
        advice_id: advice.id.clone(),
    };
    let result =
        evaluate_scope_preservation(&digest(), &manifest, &advice, &disposition, &observation)
            .unwrap();
    assert_eq!(result.status, ScopePreservationStatus::Passed);
    let encoded = serde_json::to_string(&result).unwrap();
    for forbidden in ["authorized", "permitted", "ready", "execute", "effect"] {
        assert!(!encoded.contains(forbidden));
    }

    observation.source.candidate_set_revision += 1;
    observation.source.digest = observation.source.canonical_digest(&digest()).unwrap();
    observation.manifest.source = observation.source.clone();
    for alternative in &mut observation.manifest.emitted {
        alternative.id = stable_scope_alternative_id(
            &digest(),
            &observation.manifest.constructor,
            &observation.manifest.source.digest,
            alternative.kind,
            &alternative.material_digest,
            &alternative.coverage,
        )
        .unwrap();
    }
    observation
        .manifest
        .emitted
        .sort_by(|left, right| left.id.cmp(&right.id));
    observation.manifest.baseline_id = observation.manifest.emitted[0].id.clone();
    observation.manifest.ordered_ids = observation
        .manifest
        .emitted
        .iter()
        .map(|value| value.id.clone())
        .collect();
    observation.manifest.eligible_set_digest = observation
        .manifest
        .canonical_eligible_set_digest(&digest())
        .unwrap();
    observation.manifest.whole_set_digest = observation
        .manifest
        .canonical_whole_set_digest(&digest())
        .unwrap();
    observation.candidate_set_revision = observation.source.candidate_set_revision;
    let current_request =
        ScopeAdviceRequest::from_manifest(&digest(), &observation.manifest).unwrap();
    observation.advice_id = guard_scope_advice(
        &digest(),
        &observation.manifest,
        &current_request,
        &super::fixtures::answers(
            &observation.manifest,
            [ScopeAdviceScoreBand::Fit, ScopeAdviceScoreBand::WeakFit],
        ),
    )
    .unwrap()
    .id;
    let result =
        evaluate_scope_preservation(&digest(), &manifest, &advice, &disposition, &observation)
            .unwrap();
    assert_eq!(result.status, ScopePreservationStatus::Failed);
    assert!(result.reason_codes.contains(&"source_changed".into()));
    assert!(
        result
            .reason_codes
            .contains(&"candidate_revision_changed".into())
    );
    assert!(result.reason_codes.contains(&"advice_changed".into()));
}
