use super::*;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use std::collections::BTreeMap;
use tect_domain::{
    CompletePipelinePhase, PipelineConsumedOutput, PipelinePhaseArtifactDraft,
    PipelinePhaseOutcome, PipelinePhaseOutputDraft, PipelineReviewerAttestation,
    PipelineSkillReadReceipt, PipelineTransition,
};
use uuid::Uuid;

fn digest(body: &str) -> String {
    Sha256::digest(body.as_bytes())
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect()
}

fn completion() -> (
    tect_domain::PipelineDefinitionSnapshot,
    CompletePipelinePhase,
) {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::LightweightTddDevelopment)
        .unwrap();
    let phase = definition
        .phases
        .iter()
        .find(|phase| phase.id == "slice-lightweight-pre-implementation-review")
        .unwrap();
    let consumed_outputs = vec![PipelineConsumedOutput {
        phase_id: "slice-test-target-selector".into(),
        output_revision: 2,
        digest: "target-digest".into(),
    }];
    let standards_digest = phase
        .resources
        .iter()
        .find(|resource| resource.id == "tect:engineering-standards")
        .unwrap()
        .digest
        .clone();
    let assessments = (1..=10).map(|number| json!({
        "rule_id":format!("ENG-{number:02}"),"status":"satisfied","rationale":"Concrete evidence supports this rule.","evidence_refs":["implementation-plan.md#task-1"]
    })).collect::<Vec<_>>();
    let report = json!({
        "stage":"plan","rules_digest":standards_digest,"verdict":"pass",
        "reviewed_outputs":consumed_outputs,"source_basis":"Current compact contract, plan, and test target.",
        "assessments":assessments,"findings":[],
        "files":[{"path":"src/owner.rs","content_kind":"behavioral","line_count":200,"count_basis":"estimate","responsibility":"Own the requested behavior."}],
        "summary":"The compact plan conforms to the pinned engineering standards."
    });
    let body = serde_json::to_string(&report).unwrap();
    let fields = phase
        .required_fields
        .iter()
        .map(|field| (field.clone(), "recorded".into()))
        .collect::<BTreeMap<_, _>>();
    let reads = |values: &[tect_domain::PipelineInstructionSnapshot]| {
        values
            .iter()
            .map(|value| PipelineSkillReadReceipt {
                instruction_id: value.id.clone(),
                version: value.version.clone(),
                digest: value.digest.clone(),
            })
            .collect()
    };
    let request = CompletePipelinePhase {
        request_id: Uuid::new_v4(),
        run_id: Uuid::new_v4(),
        run_revision: 1,
        phase_id: phase.id.clone(),
        outcome: PipelinePhaseOutcome::Completed,
        transition: PipelineTransition::Continue,
        output: PipelinePhaseOutputDraft {
            body: "Complete engineering plan review evidence.".into(),
            producer_context_id: "reviewer".into(),
            fields,
            verdict: Some("pass".into()),
            dispositions: vec!["engineering_review_pass".into()],
            skill_reads: reads(&phase.skills),
            resource_reads: reads(&phase.resources),
            artifacts: vec![PipelinePhaseArtifactDraft {
                name: "engineering-review.json".into(),
                media_type: "application/json".into(),
                digest: digest(&body),
                body,
                reference: None,
            }],
            validator_receipts: vec![],
            followup_proposal: None,
            reviewer_context: Some(PipelineReviewerAttestation {
                reviewer_identity: "reviewer".into(),
                reviewer_context_id: "reviewer".into(),
                producer_context_ids: vec!["producer".into()],
                fresh_input: true,
            }),
            reference: None,
            knowledge_publication: None,
        },
        consumed_outputs,
        consumed_inputs: vec![],
        revisit_phase_id: None,
        escalation_target: None,
        terminal_result: None,
        publish_blocked_result: false,
        consumed_knowledge: None,
        research_checkpoint: None,
    };
    (definition, request)
}

#[test]
fn engineering_gate_instructions_and_active_ordinals_are_exact() {
    for (kind, phase_id) in [
        (
            PipelineKind::LightweightTddDevelopment,
            "slice-lightweight-pre-implementation-review",
        ),
        (
            PipelineKind::FullDesignToExecution,
            "slice-engineering-plan-review",
        ),
    ] {
        let definition = StaticPipelineDefinitions.definition(kind).unwrap();
        assert_eq!(definition.version, "0.6.0-native.engineering.1");
        let phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == phase_id)
            .unwrap();
        assert_eq!(phase.instructions.len(), 1);
        assert_eq!(
            phase.instructions[0].id,
            "internal-instruction.engineering-review-gate"
        );
        assert!(phase.instructions[0].body.len() > 100);
        assert_eq!(
            phase.instructions[0].digest,
            digest(&phase.instructions[0].body)
        );
    }
    let lightweight = StaticPipelineDefinitions
        .definition(PipelineKind::LightweightTddDevelopment)
        .unwrap();
    assert!(
        lightweight
            .completion_contract
            .contains("phases 12, 14, and 15")
    );
    assert!(
        lightweight
            .completion_contract
            .contains("phase-13 result body")
    );
    let full = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    assert!(
        full.completion_contract
            .contains("all 21 phase obligations")
    );
    let code_review = full
        .phases
        .iter()
        .flat_map(|phase| &phase.resources)
        .find(|resource| resource.id == "tect:superpowers-v6-code-review")
        .unwrap();
    assert!(code_review.body.contains("phase-13 contract"));
}

fn report(request: &CompletePipelinePhase) -> Value {
    serde_json::from_str(&request.output.artifacts[0].body).unwrap()
}

fn replace_report(request: &mut CompletePipelinePhase, report: Value) {
    let body = serde_json::to_string(&report).unwrap();
    request.output.artifacts[0].digest = digest(&body);
    request.output.artifacts[0].body = body;
}

#[test]
fn engineering_review_accepts_complete_plan_and_size_boundaries() {
    let (definition, request) = completion();
    assert!(request.validate(&definition).is_ok());
    for (lines, kind) in [(501, "behavioral"), (1500, "declarative")] {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["files"][0]["line_count"] = json!(lines);
        value["files"][0]["content_kind"] = json!(kind);
        value["files"][0]["justification"] =
            json!("One cohesive responsibility requires this unit.");
        replace_report(&mut request, value);
        assert!(request.validate(&definition).is_ok(), "{lines}");
    }
}

#[test]
fn engineering_review_rejects_missing_receipt_report_or_exact_binding() {
    let (definition, mut request) = completion();
    request.output.resource_reads.pop();
    assert!(request.validate(&definition).is_err());
    let (definition, mut request) = completion();
    request.output.artifacts.clear();
    assert!(request.validate(&definition).is_err());
    let (definition, mut request) = completion();
    request.consumed_outputs[0].digest = "changed".into();
    assert!(request.validate(&definition).is_err());
}

#[test]
fn engineering_review_rejects_rule_verdict_finding_and_size_failures() {
    for mutation in 0..7 {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        match mutation {
            0 => value["rules_digest"] = json!("wrong"),
            1 => {
                value["assessments"].as_array_mut().unwrap().pop();
            }
            2 => {
                value["findings"] = json!([{"id":"F-1","rule_id":"ENG-02","status":"open","evidence":"Responsibility is split."}])
            }
            3 => value["verdict"] = json!("rework"),
            4 => value["files"][0]["line_count"] = json!(501),
            5 => {
                value["files"][0]["line_count"] = json!(1001);
                value["files"][0]["justification"] = json!("Cohesive.");
            }
            6 => {
                value["files"][0]["line_count"] = json!(1501);
                value["files"][0]["content_kind"] = json!("declarative");
                value["files"][0]["justification"] = json!("Cohesive.");
            }
            _ => unreachable!(),
        }
        replace_report(&mut request, value);
        assert!(
            request.validate(&definition).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn engineering_review_records_honest_rework_without_pass_shape() {
    let (definition, mut request) = completion();
    let mut value = report(&request);
    value["verdict"] = json!("rework");
    value.as_object_mut().unwrap().remove("source_basis");
    value["assessments"] = json!([{"rule_id":"ENG-02","status":"violation","rationale":"Responsibility is split.","evidence_refs":["implementation-plan.md#task-1"]}]);
    value["findings"] = json!([{"id":"F-1","rule_id":"ENG-02","status":"open","evidence":"Responsibility is split."}]);
    value["files"] = json!([]);
    replace_report(&mut request, value);
    request.output.verdict = Some("rework".into());
    request.output.dispositions = vec!["engineering_review_rework".into()];
    request.revisit_phase_id = Some("slice-lightweight-contract-writer".into());
    assert!(request.validate(&definition).is_ok());
}

#[test]
fn engineering_review_rejects_empty_or_dishonest_nonpass() {
    for mutation in 0..2 {
        let (definition, mut request) = completion();
        let mut value = report(&request);
        value["verdict"] = json!("rework");
        value.as_object_mut().unwrap().remove("source_basis");
        value["files"] = json!([]);
        if mutation == 0 {
            value["assessments"] = json!([]);
            value["findings"] = json!([]);
        } else {
            value["assessments"] = json!([{"rule_id":"ENG-02","status":"satisfied","rationale":"The boundary is cohesive.","evidence_refs":["implementation-plan.md#task-1"]}]);
            value["findings"] = json!([{"id":"F-1","rule_id":"ENG-02","status":"open","evidence":"A missing basis must be resolved."}]);
        }
        replace_report(&mut request, value);
        request.output.verdict = Some("rework".into());
        request.output.dispositions = vec!["engineering_review_rework".into()];
        request.revisit_phase_id = Some("slice-lightweight-contract-writer".into());
        assert!(
            request.validate(&definition).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn engineering_review_records_honest_partial_blocked_report() {
    let (definition, mut request) = completion();
    let mut value = report(&request);
    value["verdict"] = json!("blocked");
    value.as_object_mut().unwrap().remove("source_basis");
    value["assessments"] = json!([{"rule_id":"ENG-03","status":"unassessed","rationale":"The dependency source is unavailable.","evidence_refs":["missing:dependency-source"]}]);
    value["findings"] = json!([]);
    value["files"] = json!([]);
    replace_report(&mut request, value);
    request.output.verdict = Some("blocked".into());
    request.output.dispositions = vec!["engineering_review_blocked".into()];
    request.outcome = PipelinePhaseOutcome::Blocked;
    request.transition = PipelineTransition::Block;
    assert!(request.validate(&definition).is_ok());
}

#[test]
fn implementation_review_requires_observed_counts_and_sha256() {
    let (mut definition, mut request) = completion();
    let phase = definition
        .phases
        .iter_mut()
        .find(|phase| phase.id == request.phase_id)
        .unwrap();
    if let tect_domain::PipelineOutputConstraint::EngineeringReview { stage, .. } = phase
        .output_constraints
        .iter_mut()
        .find(|constraint| {
            matches!(
                constraint,
                tect_domain::PipelineOutputConstraint::EngineeringReview { .. }
            )
        })
        .unwrap()
    {
        *stage = "implementation".into();
    }
    let mut value = report(&request);
    value["stage"] = json!("implementation");
    value["files"][0]["count_basis"] = json!("observed");
    value["files"][0]["content_digest"] = json!("a".repeat(64));
    replace_report(&mut request, value);
    assert!(request.validate(&definition).is_ok());
    let mut value = report(&request);
    value["files"][0]
        .as_object_mut()
        .unwrap()
        .remove("content_digest");
    replace_report(&mut request, value);
    assert!(request.validate(&definition).is_err());
}

#[test]
fn archived_active_snapshots_remain_valid_and_byte_exact() {
    let lightweight =
        include_str!("../../pipeline-definitions/lightweight-tdd-0.4.0-native.skills.1.json");
    let full = include_str!(
        "../../pipeline-definitions/full-design-to-execution-0.4.0-native.skills.1.json"
    );
    assert_eq!(
        digest(lightweight),
        "66983d90c2fc8f17f91cec02a29dcd3bc382c2967f683a7392dc1378561fb921"
    );
    assert_eq!(
        digest(full),
        "eb36e20697b38204a5a10261f5454e854538b6c719213f9d9b6be0303663bab1"
    );
    assert!(load(lightweight, PipelineKind::LightweightTddDevelopment).is_ok());
    assert!(load(full, PipelineKind::FullDesignToExecution).is_ok());
}
