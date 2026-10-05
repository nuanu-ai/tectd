use super::*;

#[test]
fn embedded_definition_inventory_is_exact() {
    let provider = StaticPipelineDefinitions;
    for (kind, version, digest) in [
        (
            PipelineKind::LightweightTddDevelopment,
            "0.7.1-native.k1k5",
            "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89",
        ),
        (
            PipelineKind::FullDesignToExecution,
            "0.6.0-native.engineering.4",
            "85ec63bae1903fedb0c86ecd5326380ea8d524fe0ee29c5dce6e90b9a30cdd3d",
        ),
        (
            PipelineKind::DebugRootCause,
            "0.4.0-native.skills.2",
            "afb0f21932a11eceb8e3aba01d3d07ec9f74160203085391f7de1758118a6574",
        ),
        (
            PipelineKind::OperationalPreparation,
            "0.4.0-native.skills.2",
            "6dcf48ec7712fcc3a9dc1f40c83c2337313bfadb33cbfdbe5d76b71da455d2b4",
        ),
        (
            PipelineKind::OperationalExecution,
            "0.4.0-native.skills.2",
            "47046a703413f6e3048c6923b87dae6ceb0bbecb3c9e0f9d60ca614562267e79",
        ),
        (
            PipelineKind::ResearchToDurableKnowledge,
            "0.4.0-native.skills.2",
            "2bd0c1c0d9403066936d7c73f44dd139a3829a31efe16fe0e653cd1dcf6c4631",
        ),
        (
            PipelineKind::Research,
            "0.5.1-native.inquiry.2",
            "7d9a817dbbd4560aca33f46522027cf2aefad483bf5837bb98b494d533f115af",
        ),
        (
            PipelineKind::DeepBrainstorming,
            "0.5.0-native.inquiry.1",
            "2b7071f75bd5c9d61815c443b550ab452f7eeff3490e9fe7fe3dd71fc90d3227",
        ),
        (
            PipelineKind::CustomProcedureCapture,
            "0.4.0-native.skills.2",
            "1e439fa7521bcd607949ae7856672f2e718320b383c7af2bfb61cc9a39481a2f",
        ),
    ] {
        let definition = provider.definition(kind).unwrap();
        assert_eq!(definition.version, version, "{}", kind.as_str());
        assert_eq!(definition.digest, digest, "{}", kind.as_str());
    }

    for (version, digest) in [
        (
            "0.7.0-native.k1k5",
            "7f5dd6a4503078538d45d0c90c83fdcd896ff1216167556ff9bd0424f826aab0",
        ),
        (
            "0.7.1-native.k1k5",
            "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89",
        ),
    ] {
        let definition = provider
            .definition_for(PipelineKind::LightweightTddDevelopment, Some(version))
            .unwrap();
        assert_eq!(definition.version, version);
        assert_eq!(definition.digest, digest);
    }
}

#[test]
fn lightweight_v07_is_immutable_compact_and_traceable() {
    let definition = lightweight_v07().expect("v0.7 definition loads");
    definition.validate().expect("v0.7 definition validates");
    assert_eq!(definition.version, "0.7.1-native.k1k5");
    assert_eq!(
        definition.digest,
        "93df97f4cb4458a18411b76005b29025a56234dc47650e4147ac5fdab3d30d89"
    );
    assert_eq!(definition.phases.len(), 5);
    assert_eq!(
        definition
            .phases
            .iter()
            .map(|p| p.required_fields.len())
            .max(),
        Some(8)
    );
    assert!(definition.phases.iter().all(|phase| {
        phase
            .instructions
            .iter()
            .all(|body| body.body.len() <= 4096)
    }));
    assert!(definition.overview.origin_refs.len() >= 15);
}

#[test]
fn lightweight_v07_routes_cover_pass_rework_block_and_escalation() {
    let definition = lightweight_v07().unwrap();
    for phase in &definition.phases {
        assert_eq!(phase.verdict_routes.len(), 4);
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "pass")
        );
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "rework")
        );
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "blocked")
        );
        assert!(
            phase
                .verdict_routes
                .iter()
                .any(|route| route.verdict == "escalate")
        );
    }
}

fn v07_completion(
    phase_id: &str,
) -> (
    PipelineDefinitionSnapshot,
    tect_domain::CompletePipelinePhase,
) {
    use std::collections::BTreeMap;
    use tect_domain::{
        CompletePipelinePhase, PipelinePhaseOutcome, PipelinePhaseOutputDraft,
        PipelineTerminalResultDraft, PipelineTransition, SliceResultEvidence,
    };
    use uuid::Uuid;

    let definition = lightweight_v07().unwrap();
    let phase = definition
        .phases
        .iter()
        .find(|phase| phase.id == phase_id)
        .unwrap();
    let fields = phase
        .required_fields
        .iter()
        .map(|field| {
            let receipt = |status: &str, exit_code: i64, scopes: &[&str], target: &str| {
                serde_json::json!({
                    "command":"cargo test focused",
                    "target":target,
                    "status":status,
                    "exit_code":exit_code,
                    "fresh":true,
                    "skipped":false,
                    "scopes":scopes
                })
                .to_string()
            };
            (
                field.clone(),
                match field.as_str() {
                    "fit" => "bounded_understood".into(),
                    "parent" => "current_confirmed".into(),
                    "preflight" => "current_clear".into(),
                    "authority" | "authority_boundary" => "authorized".into(),
                    "route" => "none".into(),
                    "isolation" | "ownership" => "confirmed".into(),
                    "overlap" => "clear".into(),
                    "review_mode" => "self".into(),
                    "anti_pattern_review" => "reviewed_clear".into(),
                    "missing_proof" => "none".into(),
                    "target_binding" => "focused-target".into(),
                    "red_receipt" => {
                        receipt("failed_as_expected", 1, &["focused"], "focused-target")
                    }
                    "green_receipt" => receipt("passed", 0, &["focused"], "focused-target"),
                    "focused_proof" | "affected_proof" => {
                        receipt("passed", 0, &["focused", "affected"], "final-target")
                    }
                    "truth_level" => "local_verified".into(),
                    "promotion" => "no_promotion".into(),
                    "handoff" => "none".into(),
                    _ => "recorded".into(),
                },
            )
        })
        .collect::<BTreeMap<_, _>>();
    let request = CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: phase.id.clone(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: if phase_id == "K5" {
            PipelineTransition::Complete
        } else {
            PipelineTransition::Continue
        },
        output: PipelinePhaseOutputDraft {
            body: "bounded v0.7 phase evidence".into(),
            producer_context_id: "current-context".into(),
            fields,
            verdict: Some("pass".into()),
            dispositions: vec!["satisfied".into()],
            skill_reads: vec![],
            resource_reads: vec![],
            artifacts: vec![],
            evidence_artifacts: vec![],
            validator_receipts: vec![],
            followup_proposal: None,
            reviewer_context: None,
            reference: None,
            knowledge_publication: None,
        },
        consumed_outputs: vec![],
        consumed_inputs: vec![],
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: (phase_id == "K5").then(|| PipelineTerminalResultDraft {
            summary: "bounded implementation locally verified".into(),
            evidence: vec![SliceResultEvidence {
                kind: "test".into(),
                reference: "final-target".into(),
                observation: "focused and affected proof passed".into(),
            }],
            scope_impact: "bounded target only".into(),
            remaining_work: "none".into(),
        }),
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    };
    (definition, request)
}

#[path = "tests/definition_shapes.rs"]
mod definition_shapes;
#[path = "tests/inquiry_contracts.rs"]
mod inquiry_contracts;
#[path = "tests/lightweight_contract.rs"]
mod lightweight_contract;
#[path = "tests/snapshot_compatibility.rs"]
mod snapshot_compatibility;

#[path = "tests/frozen_contracts.rs"]
mod frozen_contracts;

#[path = "tests/native_contract.rs"]
mod native_contract;

#[path = "tests/retirement.rs"]
mod retirement;
use retirement::historical_lightweight;
