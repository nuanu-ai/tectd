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
            evidence_artifacts: vec![],
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
        assert_eq!(
            definition.version,
            if kind == PipelineKind::FullDesignToExecution {
                "0.6.0-native.engineering.4"
            } else {
                "0.6.0-native.engineering.2"
            }
        );
        let phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == phase_id)
            .unwrap();
        assert_eq!(
            phase.instructions.len(),
            if kind == PipelineKind::FullDesignToExecution {
                2
            } else {
                1
            }
        );
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

#[test]
fn every_engineering_gate_pins_exact_standards_schema_and_reviewer_resources() {
    let expected = [
        (
            "tect:engineering-standards",
            "f4374bc9f68fc9bcc7c844765d0ea4ccc581042f169ed7756a5d6e106fef9f50",
        ),
        (
            "tect:engineering-review",
            "0d21e199f1e0e33570c898f4266253d45621c6b4b5d7e4f326c7bdc0834a423f",
        ),
        (
            "tect:engineering-review-schema",
            "6af26dca3d825e8f31e9f7ad0e9156458595d51b76df477607424673f7048141",
        ),
    ];
    for kind in [
        PipelineKind::LightweightTddDevelopment,
        PipelineKind::FullDesignToExecution,
    ] {
        let definition = StaticPipelineDefinitions.definition(kind).unwrap();
        for phase in definition.phases.iter().filter(|phase| {
            phase.output_constraints.iter().any(|constraint| {
                matches!(
                    constraint,
                    tect_domain::PipelineOutputConstraint::EngineeringReview { .. }
                )
            })
        }) {
            for (id, digest) in expected {
                let resource = phase
                    .resources
                    .iter()
                    .find(|resource| resource.id == id)
                    .unwrap_or_else(|| panic!("{} missing {id}", phase.id));
                assert_eq!(resource.version, "1.0.0", "{} {id}", phase.id);
                assert_eq!(resource.digest, digest, "{} {id}", phase.id);
                if id == "tect:engineering-standards" {
                    let body = &resource.body;
                    for normative_clause in [
                        "Up to 500 lines is the target for every file",
                        "501–1000 lines requires one cohesive responsibility and an explicit",
                        "1001–1500 lines is allowed only for genuinely declarative definitions",
                        "fields, types, schemas and enumerations without substantial execution",
                        "More than 1500 lines is prohibited. There is no exception above this ceiling.",
                        "a generated label, or arbitrary file fragments",
                        "Creating an unsolicited standalone audit/test harness",
                        "then diverting the task into developing or debugging it, is strictly prohibited.",
                    ] {
                        assert!(
                            body.contains(normative_clause),
                            "{} standards resource omitted {normative_clause:?}",
                            phase.id
                        );
                    }
                }
            }
        }
    }
}

#[test]
fn engineering_definition_rejects_missing_substituted_or_wrong_version_resources() {
    for mutation in 0..4 {
        let mut definition = StaticPipelineDefinitions
            .definition(PipelineKind::LightweightTddDevelopment)
            .unwrap();
        let phase = definition
            .phases
            .iter_mut()
            .find(|phase| phase.id == "slice-lightweight-pre-implementation-review")
            .unwrap();
        match mutation {
            0 => phase
                .resources
                .retain(|resource| resource.id != "tect:engineering-standards"),
            1 => {
                phase
                    .resources
                    .iter_mut()
                    .find(|resource| resource.id == "tect:engineering-review")
                    .unwrap()
                    .id = "tect:substituted-reviewer".into()
            }
            2 => {
                phase
                    .resources
                    .iter_mut()
                    .find(|resource| resource.id == "tect:engineering-standards")
                    .unwrap()
                    .version = "wrong".into()
            }
            3 => {
                phase
                    .resources
                    .iter_mut()
                    .find(|resource| resource.id == "tect:engineering-standards")
                    .unwrap()
                    .digest = "wrong".into()
            }
            _ => unreachable!(),
        }
        assert!(definition.validate().is_err(), "mutation {mutation}");
    }
}

#[test]
fn engineering_review_requires_typed_independent_reviewer_authority() {
    for mutation in 0..4 {
        let (definition, mut request) = completion();
        let reviewer = request.output.reviewer_context.as_mut().unwrap();
        match mutation {
            0 => reviewer.reviewer_identity.clear(),
            1 => reviewer.fresh_input = false,
            2 => reviewer.producer_context_ids.clear(),
            3 => reviewer
                .producer_context_ids
                .push(reviewer.reviewer_context_id.clone()),
            _ => unreachable!(),
        }
        assert!(
            request.validate(&definition).is_err(),
            "mutation {mutation}"
        );
    }
}

#[test]
fn lightweight_review_precedes_tdd_and_current_review_binding_is_mandatory() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::LightweightTddDevelopment)
        .unwrap();
    let review = definition
        .phases
        .iter()
        .find(|phase| phase.id == "slice-lightweight-pre-implementation-review")
        .unwrap();
    let tdd = definition
        .phases
        .iter()
        .find(|phase| phase.id == "slice-tdd-cycle-runner")
        .unwrap();
    assert_eq!(review.ordinal + 1, tdd.ordinal);
    assert!(tdd.output_constraints.iter().any(|constraint| matches!(
        constraint,
        tect_domain::PipelineOutputConstraint::CodeAuthorization { required_plan_review_phase_id }
            if required_plan_review_phase_id == &review.id
    )));

    let (definition, mut request) = completion();
    request.consumed_outputs[0].digest = "changed-reviewed-input".into();
    assert!(request.validate(&definition).is_err());
}

#[path = "engineering_review_tests/validation.rs"]
mod validation;

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
