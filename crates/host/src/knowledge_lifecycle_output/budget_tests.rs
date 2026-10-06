use super::*;
use crate::knowledge_lifecycle_encoding::KnowledgeEncoding;
use tect_application::KnowledgeOutputGuard;

const TRANSPORT: usize = 8 * 1024 * 1024;

fn query(change_id: Option<Uuid>) -> KnowledgeLifecycleQuery {
    KnowledgeLifecycleQuery {
        change_id,
        view: KnowledgeLifecycleView::Current,
        output_id: None,
        digest: None,
        fragment: None,
    }
}

fn bounded(value: &Value) {
    assert!(
        serde_json::to_vec(&responses::success(value.clone()))
            .unwrap()
            .len()
            <= 8192
    );
    if let Some(fragment) = value.get("fragment") {
        assert!(fragment["byte_length"].as_u64().unwrap() <= 4096);
    }
    for action in value["actions"].as_array().unwrap() {
        if let Some(fragment) = action["arguments"]["params"].get("fragment") {
            assert!(fragment["limit"].as_u64().unwrap() <= 4096);
        }
    }
}

#[test]
fn transport_capacity_cannot_escape_public_current_overview_history_and_unit_budget() {
    let mut current = tests::context(3);
    current.origin.as_mut().unwrap().intent = "Ж🦀".repeat(30_000);
    let current_query = query(Some(current.change_id));
    let value = KnowledgeLifecycleResponse::Current(Box::new(current));
    KnowledgeEncoding::lifecycle(TRANSPORT, current_query.clone())
        .lifecycle(&value)
        .unwrap();
    let rendered = lifecycle(value, &current_query, TRANSPORT).unwrap();
    assert!(rendered.get("fragment").is_some());
    bounded(&rendered);

    let overview = KnowledgeLifecycleResponse::Overview(KnowledgeLifecycleOverview {
        workspace_generation: 1,
        active: (0..100)
            .map(|_| KnowledgeLifecycleSummary {
                change_id: Uuid::new_v4(),
                run_id: Uuid::new_v4(),
                status: PipelineRunStatus::Active,
                current_phase_id: None,
                operation_count: 1,
            })
            .collect(),
    });
    let history = KnowledgeLifecycleResponse::History(
        (0..100)
            .map(|_| KnowledgePhaseAttempt {
                id: Uuid::new_v4(),
                run_id: Uuid::new_v4(),
                phase_id: KnowledgeChangePhaseId::KcIntake,
                attempt: 1,
                outcome: PipelinePhaseOutcome::Completed,
                transition: PipelineTransition::Continue,
                output_id: None,
                output_digest: None,
                actor_session_id: Uuid::new_v4(),
            })
            .collect(),
    );
    let mut history_query = query(Some(Uuid::new_v4()));
    history_query.view = KnowledgeLifecycleView::History;
    for (value, query) in [(overview, query(None)), (history, history_query)] {
        query.validate().unwrap();
        KnowledgeEncoding::lifecycle(TRANSPORT, query.clone())
            .lifecycle(&value)
            .unwrap();
        let rendered = lifecycle(value, &query, TRANSPORT).unwrap();
        assert!(rendered.get("fragment").is_some());
        bounded(&rendered);
    }
    let unit_id = Uuid::new_v4();
    let document = serde_json::from_value(json!({"title":"large unit","canonical_text":"Ж🦀".repeat(30_000),
        "knowledge_kind":"claim","epistemic_state":"observed","target_iris":[],"conditions":[],"exceptions":[],"sources":[],"bindings":[],"profiles":[],
        "access_scope":"workspace_members","owner_ref":"fixture","authority_basis":"fixture",
        "sections":{"constraint":null,"general":null,"runbook":null,"protocol":null,"devops":null,"operations":null,"product_research":null,"security":null}})).unwrap();
    let value = KnowledgeUnitResponse::Document(Box::new(KnowledgeDocumentRevision {
        unit_id,
        revision: 7,
        lifecycle: KnowledgeLifecycleState::Active,
        document,
        source_digests: vec![],
        rdf_digest: "rdf".into(),
        unit_iri: "unit".into(),
        revision_iri: "revision".into(),
    }));
    let unit_query = KnowledgeUnitQuery {
        unit_id,
        revision: Some(7),
        fragment: None,
    };
    KnowledgeEncoding::unit(TRANSPORT, unit_query.clone())
        .unit(&value)
        .unwrap();
    let rendered = unit(value, &unit_query, TRANSPORT).unwrap();
    assert!(rendered.get("fragment").is_some());
    bounded(&rendered);
}

#[test]
fn empty_ordinary_action_lists_have_no_recommended_index() {
    for value in [
        KnowledgeLifecycleResponse::Overview(KnowledgeLifecycleOverview {
            workspace_generation: 1,
            active: vec![],
        }),
        KnowledgeLifecycleResponse::History(vec![]),
    ] {
        let rendered = lifecycle(value, &query(None), TRANSPORT).unwrap();
        assert_eq!(rendered["actions"], json!([]));
        assert!(
            rendered
                .get("recommended_action")
                .is_some_and(Value::is_null)
        );
    }
}

#[test]
fn guard_and_final_large_mutation_commit_and_settle_use_the_same_read_budget() {
    let guard = KnowledgeEncoding::new(TRANSPORT);
    let mut current = tests::context(3);
    current.origin.as_mut().unwrap().intent = "large".repeat(30_000);
    let begun = BeginKnowledgeChangeOutcome::Created(Box::new(current.clone()));
    let mutation_value = KnowledgeChangeMutationOutcome::Advanced(Box::new(current));
    guard.begin(&begun).unwrap();
    guard.mutation(&mutation_value).unwrap();
    for rendered in [
        begin(begun, TRANSPORT).unwrap(),
        mutation(mutation_value, TRANSPORT).unwrap(),
    ] {
        assert!(rendered.get("outcome").is_some());
        bounded(&rendered);
    }
    let change_id = Uuid::new_v4();
    let effect = KnowledgeEffectReceipt {
        effect_id: Uuid::new_v4(),
        kind: KnowledgeEffectKind::Impact,
        status: KnowledgeEffectStatus::Ready,
        generation: 1,
        owner_ref: "fixture".into(),
        detail: "large".repeat(30_000),
    };
    let committed = CommitKnowledgeChangeOutcome::Replay(KnowledgePublisherReceipt {
        id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        change_id,
        run_id: Uuid::new_v4(),
        sealed_command_digest: "seal".into(),
        workspace_generation: 1,
        applied_operations: vec![],
        effects: vec![effect.clone()],
        digest: "receipt".into(),
    });
    let settled = SettleKnowledgeChangeEffectsOutcome::Replay(KnowledgeEffectsReport {
        publisher_receipt_id: Uuid::new_v4(),
        effects: vec![effect],
        required_complete: true,
        remaining_work: vec![],
    });
    guard.commit(&committed).unwrap();
    guard.effects(&settled).unwrap();
    for rendered in [
        commit(committed, TRANSPORT).unwrap(),
        settle(change_id, settled, TRANSPORT).unwrap(),
    ] {
        assert_eq!(rendered["outcome"], "replay");
        bounded(&rendered);
    }
}

#[test]
fn complete_settle_retains_no_recommendation_in_small_and_compact_results() {
    let guard = KnowledgeEncoding::new(TRANSPORT);
    for required_complete in [false, true] {
        for large in [false, true] {
            let change_id = Uuid::new_v4();
            let report = KnowledgeEffectsReport {
                publisher_receipt_id: Uuid::new_v4(),
                effects: vec![],
                required_complete,
                remaining_work: if large {
                    vec!["large".repeat(30_000)]
                } else {
                    vec![]
                },
            };
            let value = SettleKnowledgeChangeEffectsOutcome::Settled(report);
            guard.effects(&value).unwrap();
            let rendered = settle(change_id, value, TRANSPORT).unwrap();
            bounded(&rendered);
            assert_eq!(rendered["actions"].as_array().unwrap().len(), 1);
            let action = &rendered["actions"][0];
            assert_eq!(action["kind"], "ready_call");
            assert_eq!(action["arguments"]["params"]["change_id"], json!(change_id));
            assert_eq!(action["arguments"]["params"]["view"], "current");
            if required_complete {
                assert!(
                    rendered
                        .get("recommended_action")
                        .is_some_and(Value::is_null)
                );
            } else {
                assert_eq!(rendered["recommended_action"], 0);
            }
            if large {
                assert_eq!(action["arguments"]["params"]["fragment"]["limit"], 4096);
            }
        }
    }
}
