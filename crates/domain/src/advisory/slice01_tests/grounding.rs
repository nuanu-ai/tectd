use super::fixtures::{digest, fixture_manifest};
use crate::*;

const LEGACY_ENTITY: &str = r#"{"id":"00000000-0000-0000-0000-000000000034","revision":1,"title":"legacy","outcome":"outcome","trigger":"trigger","delivered_behavior":"behavior","proof":"proof","includes":[],"excludes":[],"dependencies":[],"coverage_goal_ids":["00000000-0000-0000-0000-000000000033"],"evidence_ids":[]}"#;

#[test]
fn legacy_entity_roundtrip_and_explicit_default_are_byte_exact() {
    let entity: CandidateEntity = serde_json::from_str(LEGACY_ENTITY).unwrap();
    assert_eq!(entity.grounding, CandidateGrounding::SourceGrounded);
    assert_eq!(serde_json::to_string(&entity).unwrap(), LEGACY_ENTITY);
    let mut explicit: serde_json::Value = serde_json::from_str(LEGACY_ENTITY).unwrap();
    explicit["grounding"] = serde_json::json!({"kind":"source_grounded"});
    let restored: CandidateEntity = serde_json::from_value(explicit).unwrap();
    assert_eq!(restored, entity);
    assert_eq!(serde_json::to_string(&restored).unwrap(), LEGACY_ENTITY);
}

#[test]
fn current_fixture_serialization_is_self_consistent_not_a_legacy_oracle() {
    let material = fixture_manifest().emitted.remove(0).material;
    let legacy_bytes = serde_json::to_vec(&material).unwrap();
    assert!(
        !String::from_utf8(legacy_bytes.clone())
            .unwrap()
            .contains("grounding")
    );
    let restored: ResolvedCandidateDraft = serde_json::from_slice(&legacy_bytes).unwrap();
    assert_eq!(serde_json::to_vec(&restored).unwrap(), legacy_bytes);
    assert_eq!(
        scope_candidate_material_digest(&digest(), &restored).unwrap(),
        digest().sha256("tect.scope-candidate-material/2", &legacy_bytes),
    );
}

#[test]
fn source_grounded_coverage_and_exploratory_representation_are_separate() {
    let mut material = fixture_manifest().emitted.remove(0).material;
    material.validate().unwrap();
    material.require_source_grounded().unwrap();
    material.candidates[0].coverage_goal_ids.clear();
    assert!(material.validate().is_err());
    material = exploratory_material();
    material.validate().unwrap();
    assert_eq!(
        material.require_source_grounded(),
        Err(Error::InvalidArguments)
    );
    // Otherwise-valid nonzero coverage is also permitted for representation.
    material = fixture_manifest().emitted.remove(0).material;
    material.candidates[0].grounding = CandidateGrounding::ExploratoryUnrequested {
        provenance: ExploratoryProvenance::SourceAuthoredV2,
    };
    material.validate().unwrap();
    assert_eq!(
        material.require_source_grounded(),
        Err(Error::InvalidArguments)
    );
    material.candidates[0]
        .coverage_goal_ids
        .push(uuid::Uuid::from_u128(51));
    assert!(material.validate().is_err());
    material.candidates[0].coverage_goal_ids = vec![uuid::Uuid::from_u128(999)];
    assert!(material.validate().is_err());
}

#[test]
fn forged_draft_provenance_does_not_authorize_ordinary_save() {
    let value = serde_json::json!({
        "boundary":"ongoing","goals":[],"evidence":[],
        "candidates":[{
            "identity":{"local":"optional"},
            "grounding":{"kind":"exploratory_unrequested","provenance":"source_authored_v2"},
            "title":"optional","outcome":"optional","trigger":"request",
            "delivered_behavior":"optional","proof":"proof","coverage_goals":[]
        }],
        "blockers":[],"protected_changes":[],"supersessions":[]
    });
    let draft: ScopeCandidateDraft = serde_json::from_value(value).unwrap();
    draft.validate().unwrap();
    assert_eq!(
        draft.require_source_grounded(),
        Err(Error::InvalidArguments)
    );
}

#[test]
fn exploratory_provenance_is_closed() {
    for value in [
        serde_json::json!({"kind":"exploratory_unrequested"}),
        serde_json::json!({"kind":"exploratory_unrequested","provenance":"source_authored_v1"}),
        serde_json::json!({"kind":"exploratory_unrequested","provenance":"source_authored_v2","extra":true}),
    ] {
        assert!(serde_json::from_value::<CandidateGrounding>(value).is_err());
    }
}

fn exploratory_material() -> ResolvedCandidateDraft {
    let mut material = fixture_manifest().emitted.remove(0).material;
    let mut optional = material.candidates[0].clone();
    optional.id = uuid::Uuid::from_u128(53);
    optional.coverage_goal_ids.clear();
    optional.grounding = CandidateGrounding::ExploratoryUnrequested {
        provenance: ExploratoryProvenance::SourceAuthoredV2,
    };
    material.delta.added.push(CandidateAdded {
        candidate_id: optional.id,
        revision: optional.revision,
    });
    material.candidates.push(optional);
    material
}

fn reseal(manifest: &mut ScopeConstructorManifest) {
    for alternative in manifest.emitted.iter_mut().chain(
        manifest
            .rejected
            .iter_mut()
            .map(|value| &mut value.alternative),
    ) {
        alternative.material_digest =
            scope_candidate_material_digest(&digest(), &alternative.material).unwrap();
        alternative.id = stable_scope_alternative_id(
            &digest(),
            &manifest.constructor,
            &manifest.source.digest,
            alternative.kind,
            &alternative.material_digest,
            &alternative.coverage,
        )
        .unwrap();
    }
    manifest.emitted.sort_by(|a, b| a.id.cmp(&b.id));
    manifest
        .rejected
        .sort_by(|a, b| a.alternative.id.cmp(&b.alternative.id));
    manifest.baseline_id = manifest.emitted[0].id.clone();
    manifest.ordered_ids = manifest
        .emitted
        .iter()
        .map(|value| value.id.clone())
        .chain(
            manifest
                .rejected
                .iter()
                .map(|value| value.alternative.id.clone()),
        )
        .collect();
    manifest.ordered_ids.sort();
    manifest.eligible_set_digest = manifest.canonical_eligible_set_digest(&digest()).unwrap();
    manifest.whole_set_digest = manifest.canonical_whole_set_digest(&digest()).unwrap();
}

#[test]
fn legacy_v1_rejects_both_emitted_and_rejected_exploratory_alternatives() {
    for rejected in [false, true] {
        let mut manifest = fixture_manifest();
        manifest.constructor.id = "source-authored-v1".into();
        reseal(&mut manifest);
        manifest.validate(&digest()).unwrap();
        let mut alternative = manifest.emitted.remove(0);
        alternative.material = exploratory_material();
        alternative.material.validate().unwrap();
        if rejected {
            manifest.rejected.push(RejectedScopeAlternative {
                alternative,
                reason_codes: vec!["test".into()],
            });
        } else {
            manifest.emitted.push(alternative);
        }
        reseal(&mut manifest);
        assert_eq!(manifest.validate(&digest()), Err(Error::InvalidArguments));
    }
}

#[test]
fn exploratory_superseded_prior_is_not_authorized_by_grounded_current_candidates() {
    let mut material = fixture_manifest().emitted.remove(0).material;
    let mut prior = material.candidates[0].clone();
    prior.id = uuid::Uuid::from_u128(54);
    prior.grounding = CandidateGrounding::ExploratoryUnrequested {
        provenance: ExploratoryProvenance::SourceAuthoredV2,
    };
    material.delta.superseded.push(CandidateSuperseded {
        prior,
        reason: "historical replacement".into(),
        replacement_candidate_ids: vec![material.candidates[0].id],
    });
    material.validate().unwrap();
    assert!(
        material
            .candidates
            .iter()
            .all(|value| value.grounding.is_source_grounded())
    );
    assert_eq!(
        material.require_source_grounded(),
        Err(Error::InvalidArguments)
    );
    material.delta.superseded[0].prior.coverage_goal_ids.clear();
    material.validate().unwrap();
    assert_eq!(
        material.require_source_grounded(),
        Err(Error::InvalidArguments)
    );
}

// I882 pre-P1 original Domain rlib oracle; synthetic Source fixture, not a DB row.
// No newline belongs to this canonical JSON or the fixed Domain digest.
const PRE_P1_WHOLE_MATERIAL: &str = r#"{"boundary":"finite","goals":[{"id":"00000000-0000-0000-0000-000000000033","revision":1,"text":"Preserve the source outcome","source_ref_id":"00000000-0000-0000-0000-000000000032","exact_quote":null,"resolution":{"kind":"candidate","id":"00000000-0000-0000-0000-000000000034"}}],"evidence":[],"candidates":[{"id":"00000000-0000-0000-0000-000000000034","revision":1,"title":"cohesive","outcome":"Exact supplied outcome","trigger":"Exact supplied trigger","delivered_behavior":"Exact supplied behavior","proof":"Exact supplied proof","includes":["supplied"],"excludes":[],"dependencies":[],"coverage_goal_ids":["00000000-0000-0000-0000-000000000033"],"evidence_ids":[]}],"blockers":[],"pending_question":null,"empty_disposition":null,"protected_changes":[],"delta":{"added":[{"candidate_id":"00000000-0000-0000-0000-000000000034","revision":1}],"changed":[],"unchanged":[],"superseded":[{"prior":{"id":"00000000-0000-0000-0000-0000000003e8","revision":1,"title":"cohesive","outcome":"Exact supplied outcome","trigger":"Exact supplied trigger","delivered_behavior":"Exact supplied behavior","proof":"Exact supplied proof","includes":["supplied"],"excludes":[],"dependencies":[],"coverage_goal_ids":["00000000-0000-0000-0000-000000000033"],"evidence_ids":[]},"reason":"Superseded","replacement_candidate_ids":["00000000-0000-0000-0000-000000000034"]}]}}"#;
const PRE_P1_MATERIAL_DIGEST: &str =
    "942a46d8a5c674feade6cc2452a756dc529618522267c66d3159178d3ce588f5";

#[test]
fn independent_pre_p1_whole_material_with_nested_prior_is_byte_and_digest_exact() {
    assert_eq!(PRE_P1_WHOLE_MATERIAL.len(), 1349);
    let material: ResolvedCandidateDraft = serde_json::from_str(PRE_P1_WHOLE_MATERIAL).unwrap();
    material.validate().unwrap();
    material.require_source_grounded().unwrap();
    assert!(!material.delta.superseded.is_empty());
    for candidate in material
        .candidates
        .iter()
        .chain(material.delta.superseded.iter().map(|value| &value.prior))
    {
        assert_eq!(candidate.grounding, CandidateGrounding::SourceGrounded);
    }
    assert_eq!(
        serde_json::to_vec(&material).unwrap(),
        PRE_P1_WHOLE_MATERIAL.as_bytes(),
    );
    assert_eq!(
        scope_candidate_material_digest(&digest(), &material).unwrap(),
        PRE_P1_MATERIAL_DIGEST,
    );
}
