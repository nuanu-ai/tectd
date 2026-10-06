use super::*;
use sha2::{Digest, Sha256};

/// Advisory-only resolution: local labels receive stable, request-scoped
/// entity IDs. The normal saved-draft path above retains its random IDs.
pub(crate) async fn resolve_authored(
    transaction: &mut Transaction<'_, Postgres>,
    context: &ResolveContext,
    draft: &ScopeCandidateDraft,
    previous: Option<&ResolvedCandidateDraft>,
    seed: &[u8; 32],
    allowed_source_ids: &BTreeSet<Uuid>,
    constructor: &tect_domain::ScopeConstructorIdentity,
) -> Result<ResolvedCandidateDraft> {
    require_authored_grounding(draft, constructor)?;
    require_authored_source_refs(draft, allowed_source_ids)?;
    resolve_with_allocator(
        transaction,
        context,
        draft,
        previous,
        Some(allowed_source_ids),
        &|kind, local| authored_local_id(seed, kind, local),
    )
    .await
}

fn require_authored_grounding(
    draft: &ScopeCandidateDraft,
    constructor: &tect_domain::ScopeConstructorIdentity,
) -> Result<()> {
    draft.validate()?;
    if constructor != &crate::scope_advisory::source_authored_identity() {
        draft.require_source_grounded()?;
    }
    Ok(())
}

fn require_authored_source_refs(
    draft: &ScopeCandidateDraft,
    allowed: &BTreeSet<Uuid>,
) -> Result<()> {
    let cited = draft
        .goals
        .iter()
        .map(|value| value.source_ref_id)
        .chain(draft.evidence.iter().map(|value| value.source_ref_id))
        .chain(draft.blockers.iter().map(|value| value.source_ref_id))
        .chain(
            draft
                .empty_disposition
                .iter()
                .map(|value| value.source_ref_id),
        )
        .chain(
            draft
                .protected_changes
                .iter()
                .map(|value| value.authority_source_ref_id),
        );
    if cited.into_iter().all(|id| allowed.contains(&id)) {
        Ok(())
    } else {
        Err(Error::InvalidSource)
    }
}

fn authored_local_id(seed: &[u8; 32], kind: Kind, local: &str) -> Uuid {
    let mut hash = Sha256::new();
    hash.update(b"tect.scope-authored-entity-id/source-authored-v1\0");
    hash.update(seed);
    hash.update(kind.noun().as_bytes());
    hash.update([0]);
    hash.update(local.as_bytes());
    let digest = hash.finalize();
    let mut bytes = [0_u8; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x80;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    Uuid::from_bytes(bytes)
}

#[cfg(test)]
mod authored_identity_tests {
    use super::*;

    #[test]
    fn optional_authored_material_requires_exact_current_identity_and_policy_hash() {
        let draft: ScopeCandidateDraft = serde_json::from_value(serde_json::json!({
        "boundary": "finite",
        "goals": [{
            "identity": {"local": "goal"},
            "text": "Preserve the source result",
            "source_ref_id": Uuid::new_v4(),
            "resolution": {"kind": "candidate", "reference": {"local": "candidate"}}
        }],
        "candidates": [{
            "identity": {"local": "candidate"},
            "title": "Required result",
            "outcome": "Required result",
            "trigger": "Source",
            "delivered_behavior": "Deliver the required result",
            "proof": "Acceptance test",
            "coverage_goals": [{"local": "goal"}]
        }, {
            "identity": {"local": "exploratory"},
            "grounding": {"kind": "exploratory_unrequested", "provenance": "source_authored_v2"},
            "title": "Unrequested exploratory dashboard",
            "outcome": "Optional dashboard",
            "trigger": "Exploration",
            "delivered_behavior": "Show a dashboard",
            "proof": "Optional visual check",
            "coverage_goals": []
        }]
    }))
    .unwrap();
        let trusted = crate::scope_advisory::source_authored_identity();
        assert_eq!(require_authored_grounding(&draft, &trusted), Ok(()));
        let mut wrong_policy = trusted.clone();
        wrong_policy.digest = "0".repeat(64);
        let mut legacy = trusted.clone();
        legacy.id = "source-authored-v1".into();
        let mut other = trusted.clone();
        other.id = "caller-supplied-v2".into();
        for constructor in [wrong_policy, legacy, other] {
            assert_eq!(
                require_authored_grounding(&draft, &constructor),
                Err(Error::InvalidArguments)
            );
        }
        assert_eq!(
            draft.require_source_grounded(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn authored_citations_reject_blank_ref_across_every_source_field() {
        let nonblank = Uuid::from_u128(1);
        let blank = Uuid::from_u128(2);
        let allowed = BTreeSet::from([nonblank]);
        let base = serde_json::json!({
            "boundary": "finite",
            "goals": [{
                "identity": {"local": "goal"}, "text": "goal",
                "source_ref_id": nonblank,
                "resolution": {"kind": "candidate", "reference": {"local": "candidate"}}
            }],
            "evidence": [{
                "identity": {"local": "evidence"}, "kind": "verified_evidence",
                "summary": "evidence", "source_ref_id": nonblank
            }],
            "candidates": [],
            "blockers": [{
                "identity": {"local": "blocker"}, "summary": "blocker",
                "source_ref_id": nonblank
            }],
            "empty_disposition": {
                "kind": "needs_input", "reason": "pending", "source_ref_id": nonblank
            },
            "protected_changes": [{
                "accepted_evidence_id": nonblank, "disposition": "delete",
                "rationale": "change", "authority_source_ref_id": nonblank
            }]
        });
        let draft: ScopeCandidateDraft = serde_json::from_value(base.clone()).unwrap();
        assert_eq!(require_authored_source_refs(&draft, &allowed), Ok(()));
        for pointer in [
            "/goals/0/source_ref_id",
            "/evidence/0/source_ref_id",
            "/blockers/0/source_ref_id",
            "/empty_disposition/source_ref_id",
            "/protected_changes/0/authority_source_ref_id",
        ] {
            let mut value = base.clone();
            *value.pointer_mut(pointer).unwrap() = serde_json::json!(blank);
            let draft: ScopeCandidateDraft = serde_json::from_value(value).unwrap();
            assert_eq!(
                require_authored_source_refs(&draft, &allowed),
                Err(Error::InvalidSource),
                "{pointer}"
            );
        }
    }

    #[test]
    fn advisory_ids_are_stable_and_kind_scoped_while_saved_ids_remain_random() {
        let first = [1_u8; 32];
        let second = [2_u8; 32];
        let id = authored_local_id(&first, Kind::Candidate, "local");
        assert_eq!(id, authored_local_id(&first, Kind::Candidate, "local"));
        assert_ne!(id, authored_local_id(&second, Kind::Candidate, "local"));
        assert_ne!(id, authored_local_id(&first, Kind::Goal, "local"));
        assert_ne!(id, authored_local_id(&first, Kind::Candidate, "other"));
        let identity = DraftIdentity {
            local: Some("local".into()),
            id: None,
            revision: None,
        };
        let mut saved_first = BTreeMap::new();
        let mut saved_second = BTreeMap::new();
        allocate(
            &mut saved_first,
            Kind::Candidate,
            [&identity].into_iter(),
            &|_, _| Uuid::new_v4(),
        )
        .unwrap();
        allocate(
            &mut saved_second,
            Kind::Candidate,
            [&identity].into_iter(),
            &|_, _| Uuid::new_v4(),
        )
        .unwrap();
        assert!(saved_first != saved_second);
    }
}
