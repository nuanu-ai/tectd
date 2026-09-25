fn allocate<'a>(
    handles: &mut BTreeMap<String, (Kind, Uuid)>,
    kind: Kind,
    identities: impl Iterator<Item = &'a DraftIdentity>,
    local_id: &impl Fn(Kind, &str) -> Uuid,
) -> Result<()> {
    for identity in identities {
        if let Some(local) = &identity.local
            && handles
                .insert(local.clone(), (kind, local_id(kind, local)))
                .is_some()
        {
            return Err(reason(format!(
                "local label `{local}` is used by more than one draft entity"
            )));
        }
    }
    Ok(())
}

fn old_map<T>(values: Option<&[T]>, key: impl Fn(&T) -> (Uuid, i64)) -> BTreeMap<Uuid, i64> {
    values
        .unwrap_or_default()
        .iter()
        .map(key)
        .collect::<BTreeMap<_, _>>()
}

fn entity_ids<T>(
    values: &[T],
    handles: &BTreeMap<String, (Kind, Uuid)>,
    kind: Kind,
    old: &BTreeMap<Uuid, i64>,
    identity: impl Fn(&T) -> &DraftIdentity,
) -> Result<Vec<(Uuid, i64)>> {
    values
        .iter()
        .map(|value| {
            let identity = identity(value);
            if let Some(local) = &identity.local {
                let (actual_kind, id) = handles.get(local).ok_or(Error::InvalidArguments)?;
                return (*actual_kind == kind).then_some((*id, 1)).ok_or_else(|| {
                    reason(format!(
                        "local label `{local}` names a {}, not a {}",
                        actual_kind.noun(),
                        kind.noun()
                    ))
                });
            }
            let id = identity.id.ok_or_else(|| {
                reason(format!(
                    "a {} identity needs a local label or an id",
                    kind.noun()
                ))
            })?;
            let revision = old.get(&id).copied().ok_or_else(|| {
                reason(format!(
                    "{} id {id} is not in the previous revision; new entities use local labels",
                    kind.noun()
                ))
            })?;
            if identity.revision != Some(revision) {
                return Err(Error::StaleRevision);
            }
            Ok((id, revision))
        })
        .collect()
}

pub(super) fn resolve_ref(
    value: &CandidateRef,
    handles: &BTreeMap<String, (Kind, Uuid)>,
    kind: Kind,
    ids: &[(Uuid, i64)],
) -> Result<Uuid> {
    if let Some(local) = &value.local {
        let (actual, id) = handles.get(local).ok_or_else(|| {
            reason(format!(
                "reference `{local}` is not a local label declared in this draft"
            ))
        })?;
        return (*actual == kind).then_some(*id).ok_or_else(|| {
            reason(format!(
                "reference `{local}` names a {}, where a {} is required",
                actual.noun(),
                kind.noun()
            ))
        });
    }
    let id = value.id.ok_or_else(|| {
        reason(format!(
            "a {} reference needs a local label or an id",
            kind.noun()
        ))
    })?;
    ids.iter()
        .any(|(candidate, _)| *candidate == id)
        .then_some(id)
        .ok_or_else(|| {
            reason(format!(
                "reference {id} is not a {} of this draft",
                kind.noun()
            ))
        })
}

fn candidate(
    value: &CandidateDraft,
    id: Uuid,
    revision: i64,
    handles: &BTreeMap<String, (Kind, Uuid)>,
    candidates: &[(Uuid, i64)],
    goals: &[(Uuid, i64)],
    evidence: &[(Uuid, i64)],
) -> Result<CandidateEntity> {
    Ok(CandidateEntity {
        id,
        revision,
        grounding: value.grounding,
        title: value.title.clone(),
        outcome: value.outcome.clone(),
        trigger: value.trigger.clone(),
        delivered_behavior: value.delivered_behavior.clone(),
        proof: value.proof.clone(),
        includes: value.includes.clone(),
        excludes: value.excludes.clone(),
        dependencies: value
            .dependencies
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Candidate, candidates))
            .collect::<Result<_>>()?,
        coverage_goal_ids: value
            .coverage_goals
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Goal, goals))
            .collect::<Result<_>>()?,
        evidence_ids: value
            .evidence
            .iter()
            .map(|v| resolve_ref(v, handles, Kind::Evidence, evidence))
            .collect::<Result<_>>()?,
    })
}

#[cfg(test)]
mod authored_identity_tests {
    use super::*;

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
