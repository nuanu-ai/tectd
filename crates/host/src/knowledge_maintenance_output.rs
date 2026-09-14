use crate::{knowledge_lifecycle_encoding, responses};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use tect_application::KnowledgeMaintenanceOutputGuard;
use tect_domain::*;

pub(crate) struct KnowledgeMaintenanceEncoding {
    capacity: usize,
}

impl KnowledgeMaintenanceEncoding {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self { capacity }
    }
}

impl KnowledgeMaintenanceOutputGuard for KnowledgeMaintenanceEncoding {
    fn observe(&self, value: &ObserveKnowledgeMaintenanceOutcome) -> Result<()> {
        observe(value.clone(), self.capacity).map(drop)
    }

    fn begin(&self, value: &BeginKnowledgeMaintenanceChangeOutcome) -> Result<()> {
        begin(value.clone(), self.capacity).map(drop)
    }
}

pub(crate) fn query(
    value: KnowledgeMaintenanceContext,
    query: &KnowledgeMaintenanceQuery,
    capacity: usize,
) -> Result<Value> {
    let actions = context_actions(&value, query)?;
    let full = responses::with_actions(json!(value), actions, Some(0));
    if query.fragment.is_none() && responses::encoded_len(&full)? <= capacity {
        Ok(full)
    } else {
        knowledge_lifecycle_encoding::maintenance_fragment(full, query, capacity)
    }
}

pub(crate) fn observe(value: ObserveKnowledgeMaintenanceOutcome, capacity: usize) -> Result<Value> {
    let (outcome, task, changed) = match &value {
        ObserveKnowledgeMaintenanceOutcome::Created(task) => ("created", task, true),
        ObserveKnowledgeMaintenanceOutcome::Existing(task) => ("existing", task, false),
        ObserveKnowledgeMaintenanceOutcome::Replay(task) => ("replay", task, false),
    };
    let actions = task_actions(task)?;
    let full = responses::with_actions(json!(value), actions.clone(), Some(0));
    if responses::encoded_len(&full)? <= capacity {
        return Ok(full);
    }
    within(
        responses::with_actions(
            json!({
                "outcome":outcome,"changed":changed,"task_id":task.id,
                "task_revision":task.revision,"state":task.state,
                "unit_id":task.signal.unit_id,"unit_revision":task.signal.unit_revision,
                "change_id":task.change_id,"run_id":task.run_id
            }),
            vec![retrieval_action(task)?],
            Some(0),
        ),
        capacity,
    )
}

pub(crate) fn begin(
    value: BeginKnowledgeMaintenanceChangeOutcome,
    capacity: usize,
) -> Result<Value> {
    let (outcome, task, change, changed) = match &value {
        BeginKnowledgeMaintenanceChangeOutcome::Created { task, change } => {
            ("created", task, change, true)
        }
        BeginKnowledgeMaintenanceChangeOutcome::Replay { task, change } => {
            ("replay", task, change, false)
        }
    };
    let context = match change {
        BeginKnowledgeChangeOutcome::Created(context)
        | BeginKnowledgeChangeOutcome::Replay(context) => context,
    };
    let actions = vec![responses::action(
        "knowledge_lifecycle",
        json!({"change_id":context.change_id,"view":"current"}),
    )?];
    let full = responses::with_actions(json!(value), actions.clone(), Some(0));
    if responses::encoded_len(&full)? <= capacity {
        return Ok(full);
    }
    within(
        responses::with_actions(
            json!({
                "outcome":outcome,"changed":changed,"task_id":task.id,
                "task_revision":task.revision,"task_state":task.state,
                "change_id":context.change_id,"run_id":context.run.id,
                "run_revision":context.run.revision,"run_status":context.run.status,
                "current_phase_id":context.run.current_phase_id
            }),
            actions,
            Some(0),
        ),
        capacity,
    )
}

fn context_actions(
    context: &KnowledgeMaintenanceContext,
    query: &KnowledgeMaintenanceQuery,
) -> Result<Vec<Value>> {
    let mut actions = Vec::new();
    for task in &context.tasks {
        actions.extend(task_actions(task)?);
    }
    if let Some(after) = context.next_after {
        let mut params = serde_json::to_value(query).map_err(|_| Error::TransportUnavailable)?;
        params["after"] = json!(after);
        params
            .as_object_mut()
            .ok_or(Error::InternalInvariant)?
            .remove("fragment");
        actions.push(responses::action("knowledge_maintenance", params)?);
    }
    Ok(actions)
}

fn task_actions(task: &KnowledgeMaintenanceTask) -> Result<Vec<Value>> {
    if let Some(change_id) = task.change_id {
        return Ok(vec![responses::action(
            "knowledge_lifecycle",
            json!({"change_id":change_id,"view":"current"}),
        )?]);
    }
    if matches!(
        task.state,
        KnowledgeMaintenanceTaskState::NeedsReview | KnowledgeMaintenanceTaskState::Exhausted
    ) {
        let request_id = begin_request_id(task);
        return Ok(vec![crate::api::needs_action(
            "needs_context",
            "knowledge_maintenance_begin",
            json!({
                "request_id":request_id,"task_id":task.id,"task_revision":task.revision,
                "change":{"request_id":request_id,"owner":{"kind":"workspace"}}
            }),
            "context_input",
            json!({
                "task":{"task_id":task.id,"task_revision":task.revision,
                    "unit_id":task.signal.unit_id,"unit_revision":task.signal.unit_revision,
                    "reason":task.signal.reason,"basis_digest":task.signal.basis_digest,
                    "state":task.state,"attempts":task.attempts,"failure_code":task.failure_code},
                "recovery":"Link this exact task; do not retry it or create a replacement task.",
                "fields":[
                    {"path":"arguments.params.change.intent","format":"State a substantive maintenance intent grounded in this exact task and the current authorized objective. Reuse existing authority; request input only for a genuinely unspecified decision."},
                    {"path":"arguments.params.change.desired_outcome","format":"Explicit desired reviewed outcome."},
                    {"path":"arguments.params.change.sources","format":"Exact current source references, possibly empty only when valid for the chosen operation."},
                    {"path":"arguments.params.change.operation_hints","format":"Exactly one revalidate, revise, or supersede hint for this task unit and revision."},
                    {"path":"arguments.params.change.completion","format":"Explicit completion requirements."}
                ]
            }),
        )?]);
    }
    Ok(vec![retrieval_action(task)?])
}

fn retrieval_action(task: &KnowledgeMaintenanceTask) -> Result<Value> {
    responses::action(
        "knowledge_maintenance",
        json!({"unit_id":task.signal.unit_id,"states":[task.state],"limit":100}),
    )
}

fn begin_request_id(task: &KnowledgeMaintenanceTask) -> uuid::Uuid {
    let digest = Sha256::digest(format!(
        "tectd-dk4:knowledge-maintenance-begin:{}:{}:{}",
        task.id, task.revision, task.signal.basis_digest
    ));
    let mut bytes = [0; 16];
    bytes.copy_from_slice(&digest[..16]);
    bytes[6] = (bytes[6] & 0x0f) | 0x50;
    bytes[8] = (bytes[8] & 0x3f) | 0x80;
    uuid::Uuid::from_bytes(bytes)
}

fn within(value: Value, capacity: usize) -> Result<Value> {
    if responses::encoded_len(&value)? > capacity {
        Err(Error::RequestTooLarge)
    } else {
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::knowledge_lifecycle_definitions::StaticKnowledgeLifecycleDefinitions;
    use tect_application::KnowledgeLifecycleDefinitionProvider;
    use uuid::Uuid;

    fn task(state: KnowledgeMaintenanceTaskState) -> KnowledgeMaintenanceTask {
        let id = Uuid::new_v4();
        KnowledgeMaintenanceTask {
            id,
            revision: 2,
            signal: KnowledgeMaintenanceSignal {
                id: Uuid::new_v4(),
                unit_id: Uuid::new_v4(),
                unit_revision: 3,
                reason: KnowledgeMaintenanceSignalReason::OperatorRequested,
                basis: KnowledgeMaintenanceBasis::OperatorRequested {
                    subject_ref: "urn:operator:review".into(),
                    observation_digest: "digest".into(),
                },
                basis_digest: "basis".into(),
                observed_at: "2026-09-15T00:00:00Z".into(),
            },
            state,
            attempts: 0,
            failure_code: None,
            next_retry_at: None,
            change_id: None,
            run_id: None,
            current_review: None,
            affected_consumers: Vec::new(),
            terminal_evidence: None,
        }
    }

    fn change_context() -> KnowledgeChangeContext {
        let definition = StaticKnowledgeLifecycleDefinitions.definition().unwrap();
        let change_id = Uuid::new_v4();
        KnowledgeChangeContext {
            change_id,
            origin: None,
            maintenance_tasks: Vec::new(),
            run: KnowledgeChangeRun {
                id: Uuid::new_v4(),
                change_id,
                workspace_id: Uuid::new_v4(),
                revision: 1,
                definition_version: definition.version.clone(),
                definition_digest: definition.digest.clone(),
                delivery_mode: PipelineDeliveryMode::Phasewise,
                status: PipelineRunStatus::Active,
                current_phase_id: Some(KnowledgeChangePhaseId::KcIntake),
                owner: KnowledgeChangeOwner::Workspace,
            },
            delivered_phases: vec![definition.phases[0].clone()],
            definition,
            attempts: Vec::new(),
            outputs: Vec::new(),
            inputs: Vec::new(),
            erased_payloads: Vec::new(),
            baseline: None,
            candidate_baseline: None,
            candidate_source_pin_digest: None,
            candidate_impact: None,
            plan: None,
            ready_to_commit: None,
            publisher_receipt: None,
            erased_publisher_receipt: None,
            erased_no_change_proof: None,
            effects_report: None,
            result: None,
        }
    }

    #[test]
    fn generated_begin_and_pagination_actions_roundtrip_through_public_decoder() {
        let after = Uuid::new_v4();
        let review_task = task(KnowledgeMaintenanceTaskState::NeedsReview);
        let first_request =
            task_actions(&review_task).unwrap()[0]["arguments"]["params"]["request_id"].clone();
        let replay_request =
            task_actions(&review_task).unwrap()[0]["arguments"]["params"]["request_id"].clone();
        assert_eq!(first_request, replay_request);
        let value = KnowledgeMaintenanceContext {
            workspace_generation: 7,
            method: PipelineInstructionSnapshot {
                id: "tect:knowledge-maintenance:method".into(),
                version: "1".into(),
                digest: "digest".into(),
                body: "method".into(),
                origin_refs: Vec::new(),
            },
            tasks: vec![review_task],
            next_after: Some(after),
        };
        let query_value = KnowledgeMaintenanceQuery {
            unit_id: None,
            states: Vec::new(),
            after: None,
            limit: 25,
            fragment: None,
        };
        let encoded = query(value, &query_value, usize::MAX).unwrap();
        let actions = encoded["actions"].as_array().unwrap();
        assert_eq!(actions[0]["kind"], "needs_context");
        assert_eq!(
            actions[0]["arguments"]["route"],
            "knowledge.maintenance_begin"
        );
        assert_eq!(
            actions[0]["arguments"]["params"]["request_id"],
            actions[0]["arguments"]["params"]["change"]["request_id"]
        );
        assert_eq!(
            actions[0]["arguments"]["params"]["change"]["owner"]["kind"],
            "workspace"
        );
        assert_eq!(
            actions[1]["arguments"]["params"]["after"],
            after.to_string()
        );
        for action in actions {
            if action["kind"] == "ready_call" {
                crate::api::decode_public_call(
                    action["tool"].as_str().unwrap(),
                    action["arguments"].clone(),
                )
                .unwrap();
            }
        }
        let exhausted = task_actions(&task(KnowledgeMaintenanceTaskState::Exhausted)).unwrap();
        assert_eq!(exhausted[0]["kind"], "needs_context");
        assert_eq!(
            exhausted[0]["context_input"]["fields"][0]["path"],
            "arguments.params.change.intent"
        );
    }

    #[test]
    fn maintenance_query_uses_exact_utf8_fragment_continuations() {
        let value = KnowledgeMaintenanceContext {
            workspace_generation: 7,
            method: PipelineInstructionSnapshot {
                id: "tect:knowledge-maintenance:method".into(),
                version: "1".into(),
                digest: "digest".into(),
                body: "метод".repeat(100),
                origin_refs: Vec::new(),
            },
            tasks: vec![task(KnowledgeMaintenanceTaskState::Pending)],
            next_after: None,
        };
        let query_value = KnowledgeMaintenanceQuery {
            unit_id: None,
            states: Vec::new(),
            after: None,
            limit: 25,
            fragment: Some(KnowledgeLifecycleFragmentQuery {
                snapshot_digest: None,
                offset: 0,
                limit: 32,
            }),
        };
        let encoded = query(value, &query_value, 2_048).unwrap();
        assert_eq!(encoded["fragment"]["encoding"], "utf8");
        assert!(
            encoded["fragment"]["text"]
                .as_str()
                .unwrap()
                .is_char_boundary(32)
        );
        let action = &encoded["actions"][0];
        assert_eq!(action["arguments"]["route"], "knowledge.maintenance");
        crate::api::decode_public_call(
            action["tool"].as_str().unwrap(),
            action["arguments"].clone(),
        )
        .unwrap();
    }

    #[test]
    fn observe_guard_uses_the_final_compact_serializer_and_fails_below_it() {
        let value = ObserveKnowledgeMaintenanceOutcome::Created(task(
            KnowledgeMaintenanceTaskState::NeedsReview,
        ));
        let full = observe(value.clone(), usize::MAX).unwrap();
        assert_eq!(full["actions"][0]["kind"], "needs_context");
        let full_size = responses::encoded_len(&full).unwrap();
        let compact = observe(value.clone(), full_size - 1).unwrap();
        assert_eq!(compact["outcome"], "created");
        assert_eq!(
            compact["actions"][0]["arguments"]["route"],
            "knowledge.maintenance"
        );
        let compact_size = responses::encoded_len(&compact).unwrap();
        assert_eq!(
            observe(value.clone(), compact_size - 1),
            Err(Error::RequestTooLarge)
        );
        let guard = KnowledgeMaintenanceEncoding::new(compact_size - 1);
        assert_eq!(guard.observe(&value), Err(Error::RequestTooLarge));
    }

    #[test]
    fn begin_guard_preserves_exact_linkage_in_compact_output() {
        let context = change_context();
        let mut linked_task = task(KnowledgeMaintenanceTaskState::Linked);
        linked_task.change_id = Some(context.change_id);
        linked_task.run_id = Some(context.run.id);
        let value = BeginKnowledgeMaintenanceChangeOutcome::Created {
            task: linked_task.clone(),
            change: BeginKnowledgeChangeOutcome::Created(Box::new(context.clone())),
        };
        let full = begin(value.clone(), usize::MAX).unwrap();
        let compact = begin(value.clone(), responses::encoded_len(&full).unwrap() - 1).unwrap();
        assert_eq!(compact["task_id"], linked_task.id.to_string());
        assert_eq!(compact["change_id"], context.change_id.to_string());
        assert_eq!(compact["run_id"], context.run.id.to_string());
        assert_eq!(
            compact["actions"][0]["arguments"]["route"],
            "knowledge.lifecycle"
        );
        let compact_size = responses::encoded_len(&compact).unwrap();
        let guard = KnowledgeMaintenanceEncoding::new(compact_size - 1);
        assert_eq!(guard.begin(&value), Err(Error::RequestTooLarge));
    }
}
