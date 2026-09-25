use super::fixtures::{digest, fixture_manifest};
use crate::*;
use uuid::Uuid;

const DEPENDENCIES: &str = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";

fn corpus(dependent: bool) -> (AntiBloatInput, Uuid) {
    let mut manifest = fixture_manifest();
    let extra_id = Uuid::from_u128(70);
    let extra_goal_id = Uuid::from_u128(71);
    let alternative = &mut manifest.emitted[0];
    let material = &mut alternative.material;
    let mut extra = material.candidates[0].clone();
    extra.id = extra_id;
    extra.title = "Optional dashboard".into();
    extra.outcome = "Optional dashboard".into();
    extra.delivered_behavior = "Show an optional dashboard".into();
    extra.coverage_goal_ids = vec![extra_goal_id];
    material.candidates.push(extra);
    let mut goal = material.goals[0].clone();
    goal.id = extra_goal_id;
    goal.text = "Optional dashboard".into();
    goal.resolution.id = extra_id;
    material.goals.push(goal);
    material.delta.added.push(CandidateAdded {
        candidate_id: extra_id,
        revision: 1,
    });
    if dependent {
        material.candidates[0].dependencies.push(extra_id);
    }
    alternative.material_digest = scope_candidate_material_digest(&digest(), material).unwrap();
    alternative.id = stable_scope_alternative_id(
        &digest(),
        &manifest.constructor,
        &manifest.source.digest,
        alternative.kind,
        &alternative.material_digest,
        &alternative.coverage,
    )
    .unwrap();
    let selected_id = alternative.id.clone();
    manifest
        .emitted
        .sort_by(|left, right| left.id.cmp(&right.id));
    manifest.baseline_id = selected_id.clone();
    manifest.ordered_ids = manifest
        .emitted
        .iter()
        .map(|item| item.id.clone())
        .collect();
    manifest.eligible_set_digest = manifest.canonical_eligible_set_digest(&digest()).unwrap();
    manifest.whole_set_digest = manifest.canonical_whole_set_digest(&digest()).unwrap();
    manifest.validate(&digest()).unwrap();
    (
        AntiBloatInput {
            selected_revision: manifest.source.candidate_set_revision + 1,
            manifest,
            selected_id,
            graph_provenance: "trusted-fixture-binding".into(),
            dependency_digest: DEPENDENCIES.into(),
            obligation_links: vec![AntiBloatObligationLink {
                obligation_id: Uuid::from_u128(50).to_string(),
                goal_id: Uuid::from_u128(51),
            }],
            non_goal_source_obligation_ids: vec![],
            mandatory_policy_obligation_ids: vec![Uuid::from_u128(50).to_string()],
        },
        extra_id,
    )
}

fn removal(
    input: &AntiBloatInput,
    candidate_id: Uuid,
) -> (CandidateDeltaBatch, ResolvedCandidateDraft) {
    let before = &input
        .manifest
        .eligible(&input.selected_id)
        .unwrap()
        .material;
    let candidate = before
        .candidates
        .iter()
        .find(|item| item.id == candidate_id)
        .unwrap();
    let mut after = before.clone();
    after.candidates.retain(|item| item.id != candidate_id);
    after
        .goals
        .retain(|item| !candidate.coverage_goal_ids.contains(&item.id));
    after
        .delta
        .added
        .retain(|item| item.candidate_id != candidate_id);
    after
        .delta
        .changed
        .retain(|item| item.candidate_id != candidate_id);
    after
        .delta
        .unchanged
        .retain(|item| item.candidate_id != candidate_id);
    (
        CandidateDeltaBatch {
            candidate_set_id: input.manifest.source.candidate_set_id,
            expected_revision: input.selected_revision,
            idempotency_key: "fresh-corpus-removal".into(),
            operations: vec![CandidateDeltaOperation::CandidateRemove {
                candidate_id,
                expected_revision: candidate.revision,
            }],
        },
        after,
    )
}

#[test]
fn safe_narrowing_preserves_whole_plan_and_policy() {
    let (input, extra_id) = corpus(false);
    let review = review_anti_bloat(&digest(), &input).unwrap();
    let finding = review
        .findings
        .iter()
        .find(|item| item.candidate_id == extra_id)
        .unwrap();
    assert_eq!(finding.class, AntiBloatClass::UnsupportedMechanism);
    assert!(finding.rankable);
    let (delta, after) = removal(&input, extra_id);
    let pass = check_anti_bloat_delta(
        &digest(),
        &input,
        &review,
        &finding.id,
        AntiBloatDisposition::Narrow,
        &delta,
        &after,
    )
    .unwrap();
    assert_ne!(pass.before_material_digest, pass.after_material_digest);
    assert_eq!(pass.source_digest, input.manifest.source.digest);
    assert_eq!(after.goals.len(), 1);
    assert_eq!(after.candidates.len(), 1);
    assert_eq!(review, review_anti_bloat(&digest(), &input).unwrap());
}

#[test]
fn last_source_support_and_keep_cannot_delete() {
    let (input, _) = corpus(false);
    let review = review_anti_bloat(&digest(), &input).unwrap();
    let required_id = Uuid::from_u128(52);
    let finding = review
        .findings
        .iter()
        .find(|item| item.candidate_id == required_id)
        .unwrap();
    assert_eq!(finding.class, AntiBloatClass::NecessaryResult);
    let (delta, after) = removal(&input, required_id);
    assert_eq!(
        check_anti_bloat_delta(
            &digest(),
            &input,
            &review,
            &finding.id,
            AntiBloatDisposition::Narrow,
            &delta,
            &after,
        ),
        Err(AntiBloatRefusal::NotNarrowable)
    );
    assert_eq!(
        check_anti_bloat_delta(
            &digest(),
            &input,
            &review,
            &finding.id,
            AntiBloatDisposition::Keep,
            &delta,
            &after,
        ),
        Err(AntiBloatRefusal::KeepForbidden)
    );
}

#[test]
fn dependency_deletion_and_destructive_multi_step_are_refused() {
    let (input, extra_id) = corpus(true);
    let review = review_anti_bloat(&digest(), &input).unwrap();
    let finding = review
        .findings
        .iter()
        .find(|item| item.candidate_id == extra_id)
        .unwrap();
    assert_eq!(finding.class, AntiBloatClass::NecessaryEnabler);
    let (delta, after) = removal(&input, extra_id);
    assert_eq!(
        check_anti_bloat_delta(
            &digest(),
            &input,
            &review,
            &finding.id,
            AntiBloatDisposition::Narrow,
            &delta,
            &after,
        ),
        Err(AntiBloatRefusal::NotNarrowable)
    );

    let (input, extra_id) = corpus(false);
    let review = review_anti_bloat(&digest(), &input).unwrap();
    let finding = review
        .findings
        .iter()
        .find(|item| item.candidate_id == extra_id)
        .unwrap();
    let (mut delta, after) = removal(&input, extra_id);
    delta.operations.push(CandidateDeltaOperation::GoalResolve {
        goal_id: Uuid::from_u128(71),
        expected_revision: 1,
    });
    assert_eq!(
        check_anti_bloat_delta(
            &digest(),
            &input,
            &review,
            &finding.id,
            AntiBloatDisposition::Narrow,
            &delta,
            &after,
        ),
        Err(AntiBloatRefusal::DestructiveMultiStep)
    );
}

#[test]
fn stale_revision_or_unexpected_whole_plan_change_is_refused() {
    let (input, extra_id) = corpus(false);
    let review = review_anti_bloat(&digest(), &input).unwrap();
    let finding = review
        .findings
        .iter()
        .find(|item| item.candidate_id == extra_id)
        .unwrap();
    let (mut delta, mut after) = removal(&input, extra_id);
    delta.expected_revision += 1;
    assert_eq!(
        check_anti_bloat_delta(
            &digest(),
            &input,
            &review,
            &finding.id,
            AntiBloatDisposition::Narrow,
            &delta,
            &after,
        ),
        Err(AntiBloatRefusal::Stale)
    );
    delta.expected_revision -= 1;
    after.candidates[0].proof = "weaker proof".into();
    assert_eq!(
        check_anti_bloat_delta(
            &digest(),
            &input,
            &review,
            &finding.id,
            AntiBloatDisposition::Narrow,
            &delta,
            &after,
        ),
        Err(AntiBloatRefusal::PlanMismatch)
    );
}
