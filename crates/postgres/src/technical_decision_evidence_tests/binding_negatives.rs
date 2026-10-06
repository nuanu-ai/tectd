#[test]
fn technical_decision_pure_roundtrip_reuse_and_constraint_separation() {
    for required in [false, true] {
        let (a, body) = fixture(required);
        let resolved = resolve(&a, &body, 1000).unwrap().unwrap();
        assert_eq!(resolved.binding, a.binding);
        assert_eq!(resolved.reference, a.reference);
        assert_eq!(resolved.facts, resolved.card.facts);
        let comparison = tect_domain::compare_delivery_mechanisms_with_trust(
            &resolved.card,
            &resolved.card.task_id,
            &resolved.card.task_revision,
            &resolved.card.matrix_verification_digest,
            1000,
            &SnapshotTrust,
        )
        .unwrap();
        assert_eq!(
            comparison.eligible_approach_ids,
            vec![if required { "separate" } else { "reuse" }]
        );
    }
}

#[test]
fn technical_decision_strict_json_rejects_unknown_approval_accepted_and_legacy_fields() {
    let (_, body) = fixture(false);
    for pointer in [
        "/approval",
        "/owner_approval",
        "/principal",
        "/validation_outcome",
        "/facts/0/validation_outcome",
        "/facts/0/value/unknown",
        "/candidate_mapping/0/technical_approach/unknown",
    ] {
        let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
        let (parent, key) = pointer.rsplit_once('/').unwrap();
        value
            .pointer_mut(parent)
            .unwrap()
            .as_object_mut()
            .unwrap()
            .insert(key.into(), "accepted".into());
        assert!(
            parse_technical_artifact(&serde_json::to_string(&value).unwrap()).is_err(),
            "{pointer}"
        );
    }
    assert!(parse_technical_artifact("{\"schema\":\"tect.matrix-operating-evidence/1\"}").is_err());
    for malformed in [
        body.replacen(
            "\"observed_at\":900",
            "\"observed_at\":900,\"observed_at\":900",
            1,
        ),
        body.replacen("\"value\":\"supported\"", "\"value\":true", 1),
    ] {
        assert!(parse_technical_artifact(&malformed).is_err());
    }
}

#[test]
fn technical_decision_malformed_facts_and_mapping_deny_even_approved_bytes() {
    for mutation in 0..7 {
        let (mut a, body) = fixture(false);
        let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
        match mutation {
            0 => {
                value["facts"].as_array_mut().unwrap().pop();
            }
            1 => {
                let duplicate = value["facts"][0].clone();
                value["facts"][6] = duplicate;
            }
            2 => {
                value["facts"][2]["value"] =
                    serde_json::json!({"kind":"source_support","value":"supported"})
            }
            3 => value["candidate_mapping"][0]["frozen_candidate"]["approach"] = "forged".into(),
            4 => value["candidate_mapping"][0]["technical_approach"]["mechanism"] = "forged".into(),
            5 => {
                value["candidate_mapping"][0]["frozen_candidate"]["assumption_fact_ids"] =
                    serde_json::json!([])
            }
            _ => {
                value["candidate_mapping"][0]["technical_approach"]["kind"] =
                    "separate_mechanism".into()
            }
        }
        let changed = approved_bytes(&mut a, &value);
        rejected(&a, &changed, 1000);
    }
}

#[test]
fn technical_decision_full_context_binding_mismatch_denies() {
    for pointer in [
        "/tenant_id",
        "/workspace_id",
        "/task_id",
        "/task_revision",
        "/operating_verification_digest",
        "/operating_policy_version",
        "/choice_set_digest",
        "/requirements/snapshot_id",
        "/requirements/semantic_digest",
        "/requirements/authority_schema",
        "/requirements/locator/program_id",
        "/decision_question",
        "/required_outcome",
    ] {
        let (mut a, body) = fixture(false);
        let mut value: serde_json::Value = serde_json::from_str(&body).unwrap();
        *value.pointer_mut(pointer).unwrap() = if pointer == "/task_revision" {
            4.into()
        } else if pointer.ends_with("_id") {
            Uuid::new_v4().to_string().into()
        } else {
            "changed".into()
        };
        let changed = approved_bytes(&mut a, &value);
        rejected(&a, &changed, 1000);
    }
}

#[test]
fn technical_decision_metadata_authorship_digests_policy_and_reference_deny() {
    for mutation in 0..10 {
        let (mut a, body) = fixture(false);
        match mutation {
            0 => a.approval.owner_author_principal_id = Uuid::new_v4(),
            1 => a.approval.owner_authorship_ref.clear(),
            2 => a.approval.recorded_by_principal_id = Uuid::new_v4(),
            3 => a.approval.card_digest = "d".repeat(64),
            4 => a.approval.candidate_digest = "d".repeat(64),
            5 => a.approval.choice_set_digest = "d".repeat(64),
            6 => a.validator_policy_version = "unapproved/1".into(),
            7 => a.reference.content_sha256 = "d".repeat(64),
            8 => a.candidate_mapping[0].technical_approach.mechanism = "forged".into(),
            _ => a.approval.claim.approved_at = 1001,
        }
        rejected(&a, &body, 1000);
    }
    let (a, body) = fixture(false);
    let mut reference = a.reference.clone();
    reference.artifact_version += 1;
    assert!(
        checked_technical_snapshot(Some(row(&body)), &a.binding, &reference, &a, 1000)
            .unwrap()
            .is_none()
    );
    let mut binding = a.binding.clone();
    binding.task_revision += 1;
    assert!(
        checked_technical_snapshot(Some(row(&body)), &binding, &a.reference, &a, 1000)
            .unwrap()
            .is_none()
    );
}

#[test]
fn technical_decision_raw_bytes_readiness_format_size_freshness_deny() {
    let (a, body) = fixture(false);
    for mutation in 0..4 {
        let mut artifact_row = row(&body);
        match mutation {
            0 => artifact_row.1 += 1,
            1 => artifact_row.2 = "legacy".into(),
            2 => artifact_row.3 = "draft".into(),
            _ => artifact_row.4.push(' '),
        }
        assert!(
            checked_technical_snapshot(Some(artifact_row), &a.binding, &a.reference, &a, 1000)
                .unwrap()
                .is_none()
        );
    }
    assert!(
        checked_technical_snapshot(None, &a.binding, &a.reference, &a, 1000)
            .unwrap()
            .is_none()
    );
    for now in [899, 1001, 1100] {
        rejected(&a, &body, now);
    }
}

#[tokio::test]
async fn technical_decision_absent_ambiguous_or_wrong_target_metadata_denies_before_query() {
    let (a, _) = fixture(false);
    let pool = PgPool::connect_lazy("postgres://unused:unused@127.0.0.1:1/unused").unwrap();
    let empty = PgTechnicalDecisionEvidenceResolver::new(pool.clone(), vec![]);
    assert!(
        empty
            .resolve(&a.binding, &a.reference, 1000)
            .await
            .unwrap()
            .is_none()
    );
    let ambiguous =
        PgTechnicalDecisionEvidenceResolver::new(pool.clone(), vec![a.clone(), a.clone()]);
    assert!(
        ambiguous
            .resolve(&a.binding, &a.reference, 1000)
            .await
            .unwrap()
            .is_none()
    );
    let resolver = PgTechnicalDecisionEvidenceResolver::new(pool, vec![a.clone()]);
    let mut binding = a.binding.clone();
    binding.tenant_id = Uuid::new_v4();
    assert!(
        resolver
            .resolve(&binding, &a.reference, 1000)
            .await
            .unwrap()
            .is_none()
    );
}

#[tokio::test]
async fn technical_decision_future_approval_denies_before_query() {
    let (mut a, _) = fixture(false);
    a.approval.claim.approved_at = 1001;
    let pool = PgPool::connect_lazy("postgres://unused:unused@127.0.0.1:1/unused").unwrap();
    let resolver = PgTechnicalDecisionEvidenceResolver::new(pool, vec![a.clone()]);
    assert!(
        resolver
            .resolve(&a.binding, &a.reference, 1000)
            .await
            .unwrap()
            .is_none()
    );
}

#[test]
fn technical_decision_owner_delegation_does_not_replace_authorship() {
    let (mut a, body) = fixture(false);
    a.approval.claim.authority = TechnicalApprovalAuthority::OwnerDelegated;
    a.approval.owner_authorship_ref.clear();
    rejected(&a, &body, 1000);
}

#[test]
fn technical_decision_observation_time_helper_checks_exact_boundaries_without_digest_gate() {
    assert!(current_observation(900, 1100, 1000, 100));
    assert!(current_observation(1000, 1001, 1000, 100));
    for (observed, expires, age) in [
        (1001, 1100, 100),
        (900, 1000, 100),
        (900, 900, 100),
        (899, 1100, 100),
        (i64::MIN, 1100, 100),
        (900, 1100, 0),
    ] {
        assert!(!current_observation(observed, expires, 1000, age));
    }
}

#[test]
fn technical_decision_delegated_approval_with_separate_owner_authorship_is_valid() {
    let (mut a, body) = fixture(false);
    let mut card = resolve(&a, &body, 1000).unwrap().unwrap().card;
    a.approval.claim.authority = TechnicalApprovalAuthority::OwnerDelegated;
    a.approval.claim.approving_principal = Uuid::new_v4().to_string();
    card.owner_approval = a.approval.claim.clone();
    a.approval.card_digest = card.canonical_digest().unwrap();
    let resolved = resolve(&a, &body, 1000).unwrap().unwrap();
    assert_eq!(
        resolved.approval.owner_author_principal_id,
        a.binding.recorded_by_principal_id
    );
    assert_ne!(
        resolved.approval.claim.approving_principal,
        a.binding.recorded_by_principal_id.to_string()
    );
}
