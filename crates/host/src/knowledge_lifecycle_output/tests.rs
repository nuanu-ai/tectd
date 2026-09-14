use super::*;
use crate::knowledge_lifecycle_definitions::StaticKnowledgeLifecycleDefinitions;
use tect_application::KnowledgeLifecycleDefinitionProvider;

fn context(revision: i64) -> KnowledgeChangeContext {
    let definition = StaticKnowledgeLifecycleDefinitions.definition().unwrap();
    let change_id = Uuid::new_v4();
    KnowledgeChangeContext {
        change_id,
        origin: Some(KnowledgeChangeOrigin {
            intent: "bounded change".into(),
            desired_outcome: "exact result".into(),
            owner: KnowledgeChangeOwner::Workspace,
            completion: KnowledgeCompletionRequirement {
                canonical_result: true,
                exact_delivery: true,
                impact_recorded: true,
                search: KnowledgeSearchRequirement::NotRequired,
                erasure: KnowledgeErasureRequirement::NotRequired,
            },
            sources: Vec::new(),
            source_pins: Vec::new(),
            source_revision: 0,
            operation_hints: Vec::new(),
            operations: Vec::new(),
        }),
        run: KnowledgeChangeRun {
            id: Uuid::new_v4(),
            change_id,
            workspace_id: Uuid::new_v4(),
            revision,
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
        candidate_baseline: Some(KnowledgeBaselineManifest {
            workspace_generation: 1,
            registry_generation: 1,
            policy_generation: 1,
            targets: Vec::new(),
            dependencies: Vec::new(),
            identity_matches: Vec::new(),
            source_availability: Vec::new(),
            assessment_conflicts: Vec::new(),
            assessment_gaps: Vec::new(),
            conflicts: Vec::new(),
            missing_context: Vec::new(),
            digest: "candidate".into(),
        }),
        candidate_source_pin_digest: Some("source-pins".into()),
        candidate_impact: Some(KnowledgeImpactPlan {
            synchronous_changes: Vec::new(),
            affected_contexts: Vec::new(),
            derivations: Vec::new(),
            owned_copies: Vec::new(),
            followups: Vec::new(),
            blocking_conflicts: Vec::new(),
            digest: "impact".into(),
        }),
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
fn repeated_phase_context_is_stable_but_new_revision_gets_new_request() {
    let mut current = context(1);
    let first = context_actions(&current).unwrap();
    let replay = context_actions(&current).unwrap();
    assert_eq!(
        first[0]["arguments"]["params"]["request_id"],
        replay[0]["arguments"]["params"]["request_id"]
    );
    assert_eq!(
        first[0]["arguments"]["params"]["output"]["expected_run_revision"],
        1
    );
    assert!(
        first[0]["context_input"]["required_method_reads"]
            .as_array()
            .is_some_and(|items| items.len() == 1)
    );
    current.run.revision = 2;
    let revisited = context_actions(&current).unwrap();
    assert_ne!(
        first[0]["arguments"]["params"]["request_id"],
        revisited[0]["arguments"]["params"]["request_id"]
    );
    assert_eq!(
        revisited[0]["arguments"]["params"]["output"]["expected_run_revision"],
        2
    );
}

#[test]
fn machine_pins_ignore_future_plan_and_same_phase_output() {
    let mut current = context(8);
    current.plan = Some(KnowledgeBranchPlan {
        revision: 4,
        digest: "plan-4".into(),
        definition_version: current.definition.version.clone(),
        definition_digest: current.definition.digest.clone(),
        registry_version: current.definition.registry_version.clone(),
        registry_digest: current.definition.registry_digest.clone(),
        delivery_mode: PipelineDeliveryMode::Phasewise,
        operation_ids: Vec::new(),
        profiles: Vec::new(),
        obligations: Vec::new(),
        policy_refs: Vec::new(),
        shape_refs: Vec::new(),
        method_refs: Vec::new(),
    });
    let same_phase = KnowledgeChangePhaseId::KcQualifyEvidence;
    current.outputs.push(KnowledgePhaseOutput {
        id: Uuid::new_v4(),
        run_id: current.run.id,
        revision: 3,
        digest: "same-phase".into(),
        output: KnowledgeAgentPhaseOutputDraft {
            phase_id: same_phase,
            expected_run_revision: 7,
            plan_revision: 4,
            plan_digest: "plan-4".into(),
            consumed_outputs: Vec::new(),
            consumed_inputs: Vec::new(),
            baseline_guards: Vec::new(),
            source_digests: Vec::new(),
            method_reads: Vec::new(),
            phasewise_reason: None,
            body: "old attempt".into(),
            data: KnowledgeAgentPhaseData::KcQualifyEvidence(KnowledgeEvidenceManifest {
                claims: Vec::new(),
                source_pins: Vec::new(),
                source_pin_digest: "pins".into(),
                unresolved_gaps: Vec::new(),
            }),
            verdict: "retry".into(),
            outcome: PipelinePhaseOutcome::Completed,
            transition: PipelineTransition::Continue,
            findings: Vec::new(),
            dispositions: Vec::new(),
        },
        stale: false,
        stale_reason: None,
    });
    for phase in [
        KnowledgeChangePhaseId::KcIntake,
        KnowledgeChangePhaseId::KcResolveBaseline,
        KnowledgeChangePhaseId::KcQualifyPlan,
    ] {
        let output = machine_output(&current, phase);
        assert_eq!(output["plan_revision"], 0);
        assert_eq!(output["plan_digest"], "");
    }
    let output = machine_output(&current, same_phase);
    assert_eq!(output["plan_revision"], 4);
    assert_eq!(output["plan_digest"], "plan-4");
    assert_eq!(output["consumed_outputs"], json!([]));
}

#[test]
fn erased_no_change_handoff_delivers_opaque_proof_and_exact_result_shape() {
    let mut current = context(1);
    current.origin = None;
    current.run.current_phase_id = Some(KnowledgeChangePhaseId::KcResultHandoff);
    let operation_id = Uuid::new_v4();
    let unit_id = Uuid::new_v4();
    current.erased_no_change_proof = Some(KnowledgeErasedNoChangeProof {
        completion: KnowledgeCompletionRequirement {
            canonical_result: true,
            exact_delivery: true,
            impact_recorded: true,
            search: KnowledgeSearchRequirement::NotRequired,
            erasure: KnowledgeErasureRequirement::OwnedLiveCopies,
        },
        operations: vec![KnowledgeErasedNoChangeOperationProof {
            operation_id,
            unit_id,
            expected_revision: 1,
            expected_lifecycle: KnowledgeLifecycleState::Erased,
            erasure_sequence: 7,
        }],
    });
    let action = context_actions(&current).unwrap().remove(0);
    assert_eq!(action["kind"], "needs_context");
    assert_eq!(
        action["context_input"]["erased_no_change_proof"]["operations"][0]["unit_id"],
        json!(unit_id)
    );
    assert_eq!(
        action["context_input"]["required_result"],
        json!({"canonical":"no_change","user_outcome":"achieved","remaining_work":[],
            "publisher_receipt_id":"omit","effects":[]})
    );
    assert_eq!(action["arguments"]["params"]["output"]["plan_revision"], 0);
    assert_eq!(action["arguments"]["params"]["output"]["plan_digest"], "");
}

#[test]
fn settle_followup_preserves_exact_change() {
    let change_id = Uuid::new_v4();
    let report = KnowledgeEffectsReport {
        publisher_receipt_id: Uuid::new_v4(),
        effects: Vec::new(),
        required_complete: true,
        remaining_work: Vec::new(),
    };
    let value = settle(
        change_id,
        SettleKnowledgeChangeEffectsOutcome::Settled(report),
        1_000_000,
    )
    .unwrap();
    assert_eq!(
        value["actions"][0]["arguments"]["params"]["change_id"],
        json!(change_id)
    );
}

#[test]
fn erased_receipt_still_advertises_initial_settle() {
    let mut current = context(11);
    current.run.current_phase_id = Some(KnowledgeChangePhaseId::KcSettleEffects);
    let receipt_id = Uuid::new_v4();
    let effect_id = Uuid::new_v4();
    current.origin = None;
    current.erased_publisher_receipt = Some(KnowledgeErasedPublisherReceipt {
        id: receipt_id,
        request_id: Uuid::new_v4(),
        change_id: current.change_id,
        run_id: current.run.id,
        completion: KnowledgeCompletionRequirement {
            canonical_result: true,
            exact_delivery: true,
            impact_recorded: true,
            search: KnowledgeSearchRequirement::NotRequired,
            erasure: KnowledgeErasureRequirement::RestoreSafe,
        },
        operations: Vec::new(),
        effects: vec![KnowledgeOpaqueEffectReceipt {
            effect_id,
            kind: KnowledgeEffectKind::BackupDisposition,
            status: KnowledgeEffectStatus::Pending,
            generation: 7,
        }],
    });
    let action = context_actions(&current).unwrap().remove(0);
    assert_eq!(action["kind"], "ready_call");
    assert_eq!(
        action["arguments"]["params"]["publisher_receipt_id"],
        json!(receipt_id)
    );
    assert_eq!(
        action["arguments"]["params"]["effect_ids"],
        json!([effect_id])
    );
}

#[test]
fn pending_backup_checkpoint_is_context_instead_of_ready_loop() {
    let mut current = context(12);
    current.run.current_phase_id = Some(KnowledgeChangePhaseId::KcSettleEffects);
    let receipt_id = Uuid::new_v4();
    let effect_id = Uuid::new_v4();
    current.erased_publisher_receipt = Some(KnowledgeErasedPublisherReceipt {
        id: receipt_id,
        request_id: Uuid::new_v4(),
        change_id: current.change_id,
        run_id: current.run.id,
        completion: KnowledgeCompletionRequirement {
            canonical_result: true,
            exact_delivery: true,
            impact_recorded: true,
            search: KnowledgeSearchRequirement::NotRequired,
            erasure: KnowledgeErasureRequirement::RestoreSafe,
        },
        operations: Vec::new(),
        effects: vec![KnowledgeOpaqueEffectReceipt {
            effect_id,
            kind: KnowledgeEffectKind::BackupDisposition,
            status: KnowledgeEffectStatus::Pending,
            generation: 7,
        }],
    });
    current.effects_report = Some(KnowledgeEffectsReport {
        publisher_receipt_id: receipt_id,
        effects: vec![KnowledgeEffectReceipt {
            effect_id,
            kind: KnowledgeEffectKind::BackupDisposition,
            status: KnowledgeEffectStatus::Pending,
            generation: 7,
            owner_ref: "tect-backend".into(),
            detail: "operator checkpoint required through erasure sequence 9".into(),
        }],
        required_complete: false,
        remaining_work: vec!["operator checkpoint required".into()],
    });
    let action = context_actions(&current).unwrap().remove(0);
    assert_eq!(action["kind"], "needs_context");
    assert_eq!(
        action["context"]["required_operator_checkpoint"]["detail"],
        "operator checkpoint required through erasure sequence 9"
    );
    assert_eq!(
        action["arguments"]["params"]["publisher_receipt_id"],
        json!(receipt_id)
    );
}

#[test]
fn oversized_mutation_has_a_compact_exact_read_action() {
    let mut current = context(3);
    current.origin.as_mut().unwrap().intent = "é".repeat(30_000);
    let change_id = current.change_id;
    let run_id = current.run.id;
    let value = mutation(
        KnowledgeChangeMutationOutcome::Advanced(Box::new(current)),
        2_000,
    )
    .unwrap();
    assert_eq!(value["change_id"], json!(change_id));
    assert_eq!(value["run_id"], json!(run_id));
    assert_eq!(value["run_revision"], 3);
    assert_eq!(value["outcome"], "advanced");
    assert_eq!(value["changed"], true);
    assert_eq!(
        value["actions"][0]["arguments"]["params"]["change_id"],
        json!(change_id)
    );
    assert_eq!(
        value["actions"][0]["arguments"]["params"]["fragment"]["offset"],
        0
    );
    assert!(responses::encoded_len(&value).unwrap() <= 2_000);
}

#[test]
fn oversized_commit_and_effects_preserve_replay_identity_and_exact_read() {
    let change_id = Uuid::new_v4();
    let run_id = Uuid::new_v4();
    let receipt_id = Uuid::new_v4();
    let effect = KnowledgeEffectReceipt {
        effect_id: Uuid::new_v4(),
        kind: KnowledgeEffectKind::Impact,
        status: KnowledgeEffectStatus::Ready,
        generation: 8,
        owner_ref: "owner".into(),
        detail: "large".repeat(20_000),
    };
    let committed = commit(
        CommitKnowledgeChangeOutcome::Replay(KnowledgePublisherReceipt {
            id: receipt_id,
            request_id: Uuid::new_v4(),
            change_id,
            run_id,
            sealed_command_digest: "seal".into(),
            workspace_generation: 8,
            applied_operations: Vec::new(),
            effects: vec![effect.clone()],
            digest: "receipt".into(),
        }),
        2_000,
    )
    .unwrap();
    assert_eq!(committed["outcome"], "replay");
    assert_eq!(committed["changed"], false);
    assert_eq!(committed["publisher_receipt_id"], json!(receipt_id));
    assert_eq!(
        committed["actions"][0]["arguments"]["params"]["change_id"],
        json!(change_id)
    );

    let settled = settle(
        change_id,
        SettleKnowledgeChangeEffectsOutcome::Replay(KnowledgeEffectsReport {
            publisher_receipt_id: receipt_id,
            effects: vec![effect],
            required_complete: false,
            remaining_work: vec!["remaining".repeat(20_000)],
        }),
        2_000,
    )
    .unwrap();
    assert_eq!(settled["outcome"], "replay");
    assert_eq!(settled["changed"], false);
    assert_eq!(settled["publisher_receipt_id"], json!(receipt_id));
    assert_eq!(
        settled["actions"][0]["arguments"]["params"]["change_id"],
        json!(change_id)
    );
}
