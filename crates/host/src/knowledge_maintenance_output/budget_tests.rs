use super::*;
use uuid::Uuid;

#[test]
fn large_canonical_maintenance_begin_created_and_replay_have_the_same_guarded_budget() {
    let mut context = tests::change_context();
    context.definition.validate().unwrap();
    let input = "Ж🦀".repeat(30_000);
    let digest = Sha256::digest(input.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    context.inputs.push(KnowledgeChangeInput {
        id: Uuid::new_v4(),
        sequence: 1,
        revisit_phase_id: KnowledgeChangePhaseId::KcIntake,
        reason: "maintenance fixture context".into(),
        input,
        digest,
        actor_session_id: Uuid::new_v4(),
        applied_basis_amendment: None,
    });
    let mut linked = tests::task(KnowledgeMaintenanceTaskState::Linked);
    linked.change_id = Some(context.change_id);
    linked.run_id = Some(context.run.id);
    let guard = KnowledgeMaintenanceEncoding::new(8 * 1024 * 1024);
    for replay in [false, true] {
        let value = if replay {
            BeginKnowledgeMaintenanceChangeOutcome::Replay {
                task: linked.clone(),
                change: BeginKnowledgeChangeOutcome::Replay(Box::new(context.clone())),
            }
        } else {
            BeginKnowledgeMaintenanceChangeOutcome::Created {
                task: linked.clone(),
                change: BeginKnowledgeChangeOutcome::Created(Box::new(context.clone())),
            }
        };
        let action = responses::action(
            "knowledge_lifecycle",
            json!({"change_id":context.change_id,"view":"current"}),
        )
        .unwrap();
        let full = responses::with_actions(json!(value), vec![action], Some(0));
        assert!(serde_json::to_vec(&responses::success(full)).unwrap().len() > 8192);
        guard.begin(&value).unwrap();
        let compact = begin(value.clone(), 8 * 1024 * 1024).unwrap();
        assert!(
            serde_json::to_vec(&responses::success(compact.clone()))
                .unwrap()
                .len()
                <= 8192
        );
        assert_eq!(
            compact["outcome"],
            if replay { "replay" } else { "created" }
        );
        assert_eq!(compact["changed"], !replay);
        assert_eq!(compact["task_id"], json!(linked.id));
        assert_eq!(compact["task_revision"], linked.revision);
        assert_eq!(compact["task_state"], json!(linked.state));
        assert_eq!(compact["change_id"], json!(context.change_id));
        assert_eq!(compact["run_id"], json!(context.run.id));
        assert_eq!(compact["run_revision"], context.run.revision);
        assert_eq!(compact["run_status"], json!(context.run.status));
        assert_eq!(
            compact["current_phase_id"],
            json!(context.run.current_phase_id)
        );
        assert_eq!(compact["recommended_action"], 0);
        assert_eq!(compact["actions"].as_array().unwrap().len(), 1);
        let action = &compact["actions"][0];
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(action["tool"], "query");
        assert_eq!(action["arguments"]["route"], "knowledge.lifecycle");
        assert_eq!(
            action["arguments"]["params"],
            json!({"change_id":context.change_id,"view":"current"})
        );
        crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
        let too_small = responses::encoded_len(&compact).unwrap() - 1;
        assert_eq!(begin(value.clone(), too_small), Err(Error::RequestTooLarge));
        assert_eq!(
            KnowledgeMaintenanceEncoding::new(too_small).begin(&value),
            Err(Error::RequestTooLarge)
        );
    }
}

fn bounded(value: &Value) {
    assert!(
        serde_json::to_vec(&responses::success(value.clone()))
            .unwrap()
            .len()
            <= 8192
    );
}

#[test]
fn large_source_bounded_observation_task_is_compact_with_exact_retrieval_pins() {
    let mut task = tests::task(KnowledgeMaintenanceTaskState::NeedsReview);
    ObserveKnowledgeMaintenanceSignal {
        request_id: Uuid::new_v4(),
        unit_id: task.signal.unit_id,
        unit_revision: task.signal.unit_revision,
        basis: task.signal.basis.clone(),
    }
    .validate()
    .unwrap();
    task.affected_consumers = (0..64)
        .map(|index| KnowledgeMaintenanceConsumer {
            consumer_ref: format!("pipeline-manifest:{index}:{}", "x".repeat(128)),
            required: true,
            relation_name: "pipeline_knowledge_manifests".into(),
            row_id: Uuid::new_v4(),
        })
        .collect();
    assert!(
        task.affected_consumers.iter().all(
            |consumer| !consumer.consumer_ref.is_empty() && consumer.consumer_ref.len() <= 4096
        )
    );
    let guard = KnowledgeMaintenanceEncoding::new(8 * 1024 * 1024);
    for (value, outcome, changed) in [
        (
            ObserveKnowledgeMaintenanceOutcome::Created(task.clone()),
            "created",
            true,
        ),
        (
            ObserveKnowledgeMaintenanceOutcome::Existing(task.clone()),
            "existing",
            false,
        ),
        (
            ObserveKnowledgeMaintenanceOutcome::Replay(task.clone()),
            "replay",
            false,
        ),
    ] {
        let full = responses::with_actions(json!(value), task_actions(&task).unwrap(), Some(0));
        assert!(serde_json::to_vec(&responses::success(full)).unwrap().len() > 8192);
        guard.observe(&value).unwrap();
        let compact = observe(value.clone(), 8 * 1024 * 1024).unwrap();
        bounded(&compact);
        assert_eq!(compact["outcome"], outcome);
        assert_eq!(compact["changed"], changed);
        assert_eq!(compact["task_id"], json!(task.id));
        assert_eq!(compact["task_revision"], task.revision);
        assert_eq!(compact["state"], json!(task.state));
        assert_eq!(compact["unit_id"], json!(task.signal.unit_id));
        assert_eq!(compact["unit_revision"], task.signal.unit_revision);
        assert!(compact.get("change_id").is_some_and(Value::is_null));
        assert!(compact.get("run_id").is_some_and(Value::is_null));
        assert_eq!(compact["actions"].as_array().unwrap().len(), 1);
        let action = &compact["actions"][0];
        assert_eq!(action["kind"], "ready_call");
        assert_eq!(action["arguments"]["route"], "knowledge.maintenance");
        assert_eq!(
            action["arguments"]["params"],
            json!({"unit_id":task.signal.unit_id,"states":[task.state],"limit":100})
        );
        crate::api::decode_public_call("query", action["arguments"].clone()).unwrap();
        let too_small = responses::encoded_len(&compact).unwrap() - 1;
        assert_eq!(
            observe(value.clone(), too_small),
            Err(Error::RequestTooLarge)
        );
        assert_eq!(
            KnowledgeMaintenanceEncoding::new(too_small).observe(&value),
            Err(Error::RequestTooLarge)
        );
    }
}

#[test]
fn large_valid_collection_reassembles_without_executing_its_collection_next_page() {
    let mut query_value = KnowledgeMaintenanceQuery {
        unit_id: None,
        states: vec![],
        after: None,
        limit: 100,
        fragment: None,
    };
    query_value.validate().unwrap();
    let task = tests::task(KnowledgeMaintenanceTaskState::Pending);
    task.signal.basis.validate().unwrap();
    let context = KnowledgeMaintenanceContext {
        workspace_generation: 7,
        method: PipelineInstructionSnapshot {
            id: "tect:knowledge-maintenance:method".into(),
            version: "1".into(),
            digest: "digest".into(),
            body: "method".into(),
            origin_refs: vec![],
        },
        tasks: (0..100)
            .map(|_| {
                let mut task = task.clone();
                task.id = Uuid::new_v4();
                task
            })
            .collect(),
        next_after: Some(Uuid::new_v4()),
    };
    let full = responses::with_actions(
        json!(context),
        context_actions(&context, &query_value).unwrap(),
        Some(0),
    );
    let expected = serde_json::to_vec(&full).unwrap();
    let digest: String = Sha256::digest(&expected)
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    assert!(
        serde_json::to_vec(&responses::success(full.clone()))
            .unwrap()
            .len()
            > 8192
    );
    let mut restored = Vec::new();
    loop {
        let page = query(context.clone(), &query_value, 8 * 1024 * 1024).unwrap();
        bounded(&page);
        let fragment = &page["fragment"];
        let text = fragment["text"].as_str().unwrap();
        assert!(!text.is_empty() && text.len() <= 4096);
        assert_eq!(fragment["byte_length"], text.len());
        assert_eq!(fragment["offset"], restored.len());
        assert_eq!(fragment["snapshot_digest"], digest);
        restored.extend_from_slice(text.as_bytes());
        if restored.len() == expected.len() {
            assert_eq!(page["actions"], json!([]));
            assert!(page.get("recommended_action").is_some_and(Value::is_null));
            break;
        }
        assert_eq!(page["actions"].as_array().unwrap().len(), 1);
        let action = &page["actions"][0];
        assert_eq!(action["arguments"]["route"], "knowledge.maintenance");
        let params = &action["arguments"]["params"];
        assert_eq!(params["limit"], 100);
        assert!(params.get("after").is_none());
        assert!(params["fragment"]["limit"].as_u64().unwrap() <= 4096);
        query_value = serde_json::from_value(params.clone()).unwrap();
        query_value.validate().unwrap();
    }
    assert_eq!(restored, expected);
    let complete: Value = serde_json::from_slice(&restored).unwrap();
    assert_eq!(complete["tasks"].as_array().unwrap().len(), 100);
    assert_eq!(complete["actions"].as_array().unwrap().len(), 101);
    assert_eq!(
        complete["actions"][100]["arguments"]["params"]["after"],
        json!(context.next_after)
    );
}

#[test]
fn empty_maintenance_query_has_no_recommended_index() {
    let context = KnowledgeMaintenanceContext {
        workspace_generation: 7,
        method: PipelineInstructionSnapshot {
            id: "method".into(),
            version: "1".into(),
            digest: "digest".into(),
            body: "method".into(),
            origin_refs: vec![],
        },
        tasks: vec![],
        next_after: None,
    };
    let query_value = KnowledgeMaintenanceQuery {
        unit_id: None,
        states: vec![],
        after: None,
        limit: 100,
        fragment: None,
    };
    query_value.validate().unwrap();
    let rendered = query(context, &query_value, 8 * 1024 * 1024).unwrap();
    bounded(&rendered);
    assert_eq!(rendered["actions"], json!([]));
    assert!(
        rendered
            .get("recommended_action")
            .is_some_and(Value::is_null)
    );
}
