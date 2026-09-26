use super::*;

pub(crate) fn saved() -> tect_application::StoredAntiBloatReview {
    use tect_domain::*;
    use uuid::Uuid;
    let id = Uuid::from_u128(1);
    let digest = "a".repeat(64);
    let source = FrozenScopeSource {
        candidate_set_id: id,
        candidate_set_revision: 1,
        snapshot_id: id,
        input_cursor: 0,
        program_id: id,
        program_revision: 1,
        program_latest_input: 0,
        planning_latest_input: 0,
        selected_sources_digest: digest.clone(),
        method_revision: "1".into(),
        method_digest: digest.clone(),
        registry_revision: "1".into(),
        registry_digest: digest.clone(),
        inputs: vec![],
        digest: digest.clone(),
    };
    let selected_id = ScopeAlternativeId("selected".into());
    let input = AntiBloatInput {
        manifest: ScopeConstructorManifest {
            constructor: ScopeConstructorIdentity {
                id: "test".into(),
                version: "1".into(),
                digest: digest.clone(),
            },
            source,
            obligations: vec![],
            emitted: vec![],
            rejected: vec![],
            baseline_id: selected_id.clone(),
            ordered_ids: vec![],
            eligible_set_digest: digest.clone(),
            whole_set_digest: digest.clone(),
        },
        selected_id: selected_id.clone(),
        selected_revision: 1,
        graph_provenance: "test".into(),
        dependency_digest: digest.clone(),
        obligation_links: vec![],
        non_goal_source_obligation_ids: vec![],
        mandatory_policy_obligation_ids: vec![],
    };
    tect_application::StoredAntiBloatReview {
        review_id: id,
        workspace_id: id,
        actor_id: id,
        input,
        review: AntiBloatReview {
            source_digest: digest.clone(),
            whole_set_digest: digest.clone(),
            material_digest: digest.clone(),
            candidate_set_id: id,
            plan_revision: 1,
            dependency_digest: digest,
            selected_id,
            findings: (0..7)
                .rev()
                .map(|i| AntiBloatFinding {
                    id: format!("id-{i}"),
                    candidate_id: id,
                    class: AntiBloatClass::Duplicate,
                    rankable: true,
                    reason: "duplicate source outcome".into(),
                })
                .collect(),
        },
        state: tect_application::AntiBloatAttemptState::Prepared,
    }
}

#[test]
fn canonical_full_request_and_persisted_binding_restore() {
    use sha2::{Digest, Sha256};
    let saved = saved();
    let ids = saved
        .review
        .findings
        .iter()
        .map(|f| f.id.clone())
        .collect::<Vec<_>>();
    let p = prepare_choice_request(
        "jev-1.13.0",
        &"f".repeat(64),
        &AntiBloatRankingMaterial {
            saved: &saved,
            eligible_ids: &ids,
        },
        100000,
    )
    .unwrap();
    assert_eq!(p.token_to_finding_id.len(), 7);
    let mut reversed = ids.clone();
    reversed.reverse();
    assert_eq!(
        p,
        prepare_choice_request(
            "jev-1.13.0",
            &"f".repeat(64),
            &AntiBloatRankingMaterial {
                saved: &saved,
                eligible_ids: &reversed
            },
            100000
        )
        .unwrap()
    );
    let v: Value = serde_json::from_slice(p.body()).unwrap();
    assert_eq!(v["questions"].as_object().unwrap().len(), 1);
    assert_eq!(v["questions"][QUESTION]["type"], "choice");
    let permit = tect_application::AntiBloatSendPermit {
        review_id: saved.review_id,
        request: tect_application::AntiBloatPreparedRequest {
            bytes: p.body.clone(),
            sha256: format!("{:x}", Sha256::digest(&p.body)),
            material_sha256: p.material_sha256.clone(),
            adapter_identity: "test".into(),
        },
    };
    assert_eq!(
        restore_choice_request(&permit, "jev-1.13.0", "test", &"f".repeat(64), 100000).unwrap(),
        p
    );
    assert!(
        restore_choice_request(&permit, "jev-latest", "test", &"f".repeat(64), 100000).is_err()
    );
    let mut changed = permit.clone();
    changed.request.material_sha256 = "b".repeat(64);
    assert!(
        restore_choice_request(&changed, "jev-1.13.0", "test", &"f".repeat(64), 100000).is_err()
    );
    let mut changed = permit.clone();
    changed.request.bytes.push(b' ');
    changed.request.sha256 = format!("{:x}", Sha256::digest(&changed.request.bytes));
    assert!(
        restore_choice_request(&changed, "jev-1.13.0", "test", &"f".repeat(64), 100000).is_err()
    );
    let mut incomplete = ids;
    incomplete.pop();
    assert!(
        prepare_choice_request(
            "jev-1.13.0",
            &"f".repeat(64),
            &AntiBloatRankingMaterial {
                saved: &saved,
                eligible_ids: &incomplete
            },
            100000
        )
        .is_err()
    );
}

fn prepared(count: usize) -> PreparedChoiceRequest {
    PreparedChoiceRequest {
        body: vec![],
        model: "jev-1.13.0".into(),
        material_sha256: "a".repeat(64),
        token_to_finding_id: (0..count)
            .map(|i| (format!("R{i}"), format!("id-{i}")))
            .collect(),
    }
}
fn response() -> Value {
    json!({"model":"jev-1.13.0","answers":{QUESTION:{"type":"choice","choice":"R0","probabilities":{"R0":0.6,"R1":0.3,"ABSTAIN":0.1},"confidence":0.01}},"usage":{"input_tokens":20,"output_tokens":30}})
}
fn decode(v: Value) -> AntiBloatRankingOutcome {
    parse_choice_response(&serde_json::to_vec(&v).unwrap(), &prepared(2), 16384)
}

#[test]
fn full_order_without_confidence_threshold_and_single_finding() {
    assert_eq!(
        decode(response()),
        AntiBloatRankingOutcome::Ranked(vec!["id-0".into(), "id-1".into()])
    );
    let v = json!({"model":"jev-1.13.0","answers":{QUESTION:{"type":"choice","choice":"R0","probabilities":{"R0":0.51,"ABSTAIN":0.49}}}});
    assert_eq!(
        parse_choice_response(&serde_json::to_vec(&v).unwrap(), &prepared(1), 16384),
        AntiBloatRankingOutcome::Ranked(vec!["id-0".into()])
    );
}
#[test]
fn explicit_abstention_and_ties_abstain() {
    for (choice, distribution) in [
        ("ABSTAIN", json!({"R0":0.1,"R1":0.2,"ABSTAIN":0.7})),
        ("R0", json!({"R0":0.4,"R1":0.4,"ABSTAIN":0.2})),
        ("R0", json!({"R0":0.45,"R1":0.1,"ABSTAIN":0.45})),
    ] {
        let mut v = response();
        v["answers"][QUESTION]["choice"] = json!(choice);
        v["answers"][QUESTION]["probabilities"] = distribution;
        assert_eq!(decode(v), AntiBloatRankingOutcome::Abstained);
    }
    let p = prepared(3);
    let v = json!({"model":p.model,"answers":{QUESTION:{"type":"choice","choice":"R0","probabilities":{"R0":0.6,"R1":0.1,"R2":0.1,"ABSTAIN":0.2}}}});
    assert_eq!(
        parse_choice_response(&serde_json::to_vec(&v).unwrap(), &p, 16384),
        AntiBloatRankingOutcome::Abstained
    );
}
#[test]
fn invalid_body_labels_model_selection_distribution_and_confidence() {
    for (path, value) in [
        (vec!["model"], json!("jev-latest")),
        (vec!["answers", QUESTION, "choice"], json!("R1")),
        (vec!["answers", QUESTION, "type"], json!("score")),
        (vec!["answers", QUESTION, "confidence"], json!(1.1)),
        (vec!["answers", QUESTION, "probabilities", "R0"], json!(0.7)),
        (vec!["answers", QUESTION, "probabilities", "R2"], json!(0.0)),
    ] {
        let mut v = response();
        let mut cursor = &mut v;
        for key in path {
            cursor = &mut cursor[key];
        }
        *cursor = value;
        assert_eq!(decode(v), AntiBloatRankingOutcome::InvalidResponse);
    }
    let mut v = response();
    v["answers"][QUESTION]["probabilities"]
        .as_object_mut()
        .unwrap()
        .remove("R1");
    assert_eq!(decode(v), AntiBloatRankingOutcome::InvalidResponse);
    let mut v = response();
    v["answers"]["other"] = v["answers"][QUESTION].clone();
    assert_eq!(decode(v), AntiBloatRankingOutcome::InvalidResponse);
    assert_eq!(
        parse_choice_response(b"{", &prepared(2), 16384),
        AntiBloatRankingOutcome::InvalidResponse
    );
}
#[test]
fn optional_usage_is_unknown_and_invalid_counters_error() {
    assert_eq!(
        decode_choice_usage(b"{}", 100).unwrap(),
        AntiBloatUsage {
            input_tokens: None,
            output_tokens: None
        }
    );
    assert_eq!(
        decode_choice_usage(br#"{"usage":{"input_tokens":0}}"#, 100).unwrap(),
        AntiBloatUsage {
            input_tokens: Some(0),
            output_tokens: None
        }
    );
    for v in [
        json!(-1),
        json!(1.5),
        json!("2"),
        json!(null),
        json!(u64::MAX),
    ] {
        assert!(
            decode_choice_usage(
                &serde_json::to_vec(&json!({"usage":{"input_tokens":v}})).unwrap(),
                100
            )
            .is_err()
        );
    }
    assert_eq!(
        decode_choice_usage(&serde_json::to_vec(&response()).unwrap(), 16384).unwrap(),
        AntiBloatUsage {
            input_tokens: Some(20),
            output_tokens: Some(30)
        }
    );
}
