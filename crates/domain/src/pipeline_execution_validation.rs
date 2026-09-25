use crate::*;

mod definition;

mod completion;

mod completion_helpers;
#[cfg(test)]
use completion_helpers::missing_test_target;

impl RecordPipelineInput {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || self.phase_id.trim().is_empty()
            || self.input.trim().is_empty()
            || self.input.len() > MAX_PIPELINE_INPUT_BYTES
        {
            return Err(Error::InvalidArguments);
        }
        if let Some(amendment) = &self.source_amendment {
            validate_source_amendment(amendment)?;
        }
        Ok(())
    }
}

fn valid_relative_source_path(value: &str) -> bool {
    !value.is_empty()
        && value.trim() == value
        && value.len() <= MAX_SOURCE_PATH_BYTES
        && !value.starts_with('/')
        && !value.contains(['\\', '\0'])
        && value
            .split('/')
            .all(|part| !matches!(part, "" | "." | ".."))
}

fn valid_media_type(value: &str) -> bool {
    value.trim() == value
        && value.len() <= 255
        && value.split_once('/').is_some_and(|(kind, subtype)| {
            !kind.is_empty()
                && !subtype.is_empty()
                && value.bytes().all(|byte| {
                    byte.is_ascii_alphanumeric()
                        || matches!(
                            byte,
                            b'!' | b'#' | b'$' | b'&' | b'^' | b'_' | b'.' | b'+' | b'-' | b'/'
                        )
                })
        })
}

fn validate_source_amendment(amendment: &PipelineSourceAmendment) -> Result<()> {
    let successor = &amendment.successor;
    let artifact = &successor.artifact;
    if amendment.target_phase_id.trim().is_empty()
        || amendment.predecessor.output_id.is_nil()
        || amendment.predecessor.output_revision < 1
        || amendment.predecessor.output_digest.trim().is_empty()
        || amendment.predecessor.output_digest.len() > 128
        || amendment.predecessor.artifact_name.trim().is_empty()
        || amendment.predecessor.artifact_name.len() > MAX_SOURCE_PATH_BYTES
        || amendment.predecessor.artifact_digest.trim().is_empty()
        || amendment.predecessor.artifact_digest.len() > 128
        || !valid_relative_source_path(&amendment.predecessor.source_path)
        || amendment.predecessor.source_digest.trim().is_empty()
        || amendment.predecessor.source_digest.len() > 128
        || !valid_relative_source_path(&successor.path)
        || !valid_relative_source_path(&artifact.name)
        || successor.path != artifact.name
        || !valid_media_type(&artifact.media_type)
        || artifact.body.trim().is_empty()
        || artifact.body.len() > MAX_PIPELINE_OUTPUT_BYTES
        || artifact.digest.len() != 64
        || !artifact
            .digest
            .bytes()
            .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
        || artifact.reference.as_ref().is_some_and(|reference| {
            reference.trim().is_empty() || reference.len() > MAX_SOURCE_PATH_BYTES
        })
        || amendment.authorization_scope.trim().is_empty()
        || amendment.authorization_scope.len() > MAX_PIPELINE_INPUT_BYTES
        || amendment.authorization_provenance.trim().is_empty()
        || amendment.authorization_provenance.len() > MAX_PIPELINE_INPUT_BYTES
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

impl EscalatePipelineDelivery {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.run_revision < 1
            || self.phase_id.trim().is_empty()
            || self.reason.trim().is_empty()
        {
            Err(Error::InvalidArguments)
        } else {
            Ok(())
        }
    }
}

#[cfg(test)]
mod source_amendment_tests {
    use super::*;
    use std::collections::BTreeMap;
    use uuid::Uuid;

    fn proof_test_definition(version: &str) -> PipelineDefinitionSnapshot {
        let instruction = PipelineInstructionSnapshot {
            id: "instruction".into(),
            version: "1".into(),
            digest: "instruction-digest".into(),
            body: "instruction".into(),
            origin_refs: Vec::new(),
        };
        let phase = PipelinePhaseDefinition {
            id: "phase-1".into(),
            ordinal: 1,
            title: "Phase".into(),
            required: true,
            disposition_required: false,
            instructions: vec![instruction.clone()],
            skills: Vec::new(),
            resources: Vec::new(),
            required_artifacts: Vec::new(),
            validator_contracts: Vec::new(),
            followup_contracts: Vec::new(),
            required_fields: Vec::new(),
            allowed_verdicts: Vec::new(),
            required_dispositions: Vec::new(),
            allowed_dispositions: Vec::new(),
            output_constraints: Vec::new(),
            verdict_routes: Vec::new(),
            allowed_backward_to: Vec::new(),
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "contract".into(),
        };
        PipelineDefinitionSnapshot {
            kind: PipelineKind::LightweightTddDevelopment,
            version: version.into(),
            digest: "definition-digest".into(),
            overview: instruction,
            default_mode: PipelineDeliveryMode::Phasewise,
            allowed_modes: vec![PipelineDeliveryMode::Phasewise],
            phases: vec![phase],
            completion_contract: "completion".into(),
            escalation_contract: "escalation".into(),
            forbidden_claims: Vec::new(),
        }
    }

    fn proof_test_completion() -> CompletePipelinePhase {
        CompletePipelinePhase {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 1,
            phase_id: "phase-1".into(),
            outcome: PipelinePhaseOutcome::Completed,
            transition: PipelineTransition::Continue,
            output: PipelinePhaseOutputDraft {
                body: "body".into(),
                producer_context_id: "ctx".into(),
                fields: BTreeMap::new(),
                verdict: None,
                dispositions: Vec::new(),
                skill_reads: Vec::new(),
                resource_reads: Vec::new(),
                artifacts: Vec::new(),
                evidence_artifacts: Vec::new(),
                validator_receipts: Vec::new(),
                followup_proposal: None,
                knowledge_publication: None,
                reviewer_context: None,
                reference: None,
            },
            consumed_outputs: vec![PipelineConsumedOutput {
                phase_id: "previous".into(),
                output_revision: 1,
                digest: "output-digest".into(),
            }],
            consumed_inputs: vec![PipelineConsumedInput {
                input_id: Uuid::new_v4(),
                sequence: 1,
                digest: "input-digest".into(),
            }],
            revisit_phase_id: None,
            escalation_target: None,
            terminal_result: None,
            publish_blocked_result: false,
            consumed_knowledge: None,
            research_checkpoint: None,
        }
    }

    fn request(body: String) -> RecordPipelineInput {
        RecordPipelineInput {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 2,
            phase_id: "slice-implementation-spec-synthesizer".to_owned(),
            input: "Direct source amendment authority.".to_owned(),
            source_amendment: Some(PipelineSourceAmendment {
                target_phase_id: "slice-component-decision-interrogator".to_owned(),
                predecessor: PipelineSourcePredecessor {
                    output_id: Uuid::new_v4(),
                    output_revision: 1,
                    output_digest: "a".repeat(64),
                    artifact_name: "requirements-ledger.json".to_owned(),
                    artifact_digest: "b".repeat(64),
                    source_path: "source.md".to_owned(),
                    source_digest: "c".repeat(64),
                },
                successor: PipelineSourceSuccessor {
                    path: "source.md".to_owned(),
                    artifact: PipelineSourceArtifactDraft {
                        name: "source.md".to_owned(),
                        media_type: "text/markdown".to_owned(),
                        body,
                        digest: "d67e2e944994496c8d8ec76eed0cf9f09679448d584b532bebf941852a37f5ed"
                            .to_owned(),
                        reference: None,
                    },
                },
                authorization_scope: "Amend the current Full Design source.".to_owned(),
                authorization_provenance: "Exact direct operator input.".to_owned(),
            }),
        }
    }

    #[test]
    fn source_amendment_accepts_one_bounded_hash_valid_artifact() {
        assert_eq!(request("changed".to_owned()).validate(), Ok(()));
    }

    #[test]
    fn source_amendment_rejects_empty_even_with_the_empty_sha256() {
        assert_eq!(
            request(String::new()).validate(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn source_amendment_rejects_unsafe_and_untrimmed_paths() {
        for path in ["../source.md", "/source.md", " source.md", "source.md "] {
            let mut value = request("changed".to_owned());
            let amendment = value.source_amendment.as_mut().unwrap();
            amendment.successor.path = path.to_owned();
            amendment.successor.artifact.name = path.to_owned();
            assert_eq!(value.validate(), Err(Error::InvalidArguments), "{path}");
        }
    }

    #[test]
    fn source_amendment_rejects_oversized_body() {
        assert_eq!(
            request("x".repeat(MAX_PIPELINE_OUTPUT_BYTES + 1)).validate(),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn source_amendment_rejects_path_name_mismatch() {
        let mut path = request("changed".to_owned());
        path.source_amendment
            .as_mut()
            .unwrap()
            .successor
            .artifact
            .name = "other.md".to_owned();
        assert_eq!(path.validate(), Err(Error::InvalidArguments));
    }

    #[test]
    fn source_amendment_leaves_digest_integrity_to_application() {
        let mut digest = request("changed".to_owned());
        digest
            .source_amendment
            .as_mut()
            .unwrap()
            .successor
            .artifact
            .digest = "0".repeat(64);
        assert_eq!(digest.validate(), Ok(()));
    }

    #[test]
    fn missing_test_target_has_typed_refusal_predicate() {
        let phase = PipelinePhaseDefinition {
            id: "tdd".into(),
            ordinal: 1,
            title: "TDD".into(),
            required: true,
            disposition_required: false,
            instructions: vec![],
            skills: vec![],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            followup_contracts: vec![],
            required_fields: vec!["selected_test_target".into()],
            allowed_verdicts: vec![],
            required_dispositions: vec![],
            allowed_dispositions: vec![],
            output_constraints: vec![],
            verdict_routes: vec![],
            allowed_backward_to: vec![],
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "target".into(),
        };
        let output = PipelinePhaseOutputDraft {
            body: "body".into(),
            producer_context_id: "ctx".into(),
            fields: BTreeMap::new(),
            verdict: None,
            dispositions: vec![],
            skill_reads: vec![],
            resource_reads: vec![],
            artifacts: vec![],
            evidence_artifacts: vec![],
            validator_receipts: vec![],
            followup_proposal: None,
            knowledge_publication: None,
            reviewer_context: None,
            reference: None,
        };
        assert!(missing_test_target(&phase, &output));
    }

    #[test]
    fn backend_proof_is_rejected_only_for_current_definitions_with_exact_field_paths() {
        let legacy = proof_test_completion();
        assert_eq!(legacy.validate(&proof_test_definition("0.6.0")), Ok(()));

        let definition = proof_test_definition("0.7.0-native.k1k5");
        for (field, path) in [
            ("consumed_outputs", "arguments.params.consumed_outputs"),
            ("consumed_inputs", "arguments.params.consumed_inputs"),
        ] {
            let mut request = proof_test_completion();
            if field == "consumed_outputs" {
                request.consumed_inputs.clear();
            } else {
                request.consumed_outputs.clear();
            }
            let error = request.validate(&definition).unwrap_err();
            let refusal = error.refusal().expect("typed backend proof refusal");
            assert_eq!(error.code(), "BACKEND_DERIVED_PROOF_REQUIRED");
            assert_eq!(refusal.code, RefusalCode::BackendDerivedProofRequired);
            assert_eq!(refusal.rule.as_deref(), Some("WP3-PROOF-01"));
            assert_eq!(refusal.path.as_deref(), Some(path));
            assert_eq!(
                refusal.next_action.as_deref(),
                Some("omit_agent_supplied_proof")
            );
            assert_eq!(
                refusal.expected.as_deref(),
                Some("omitted; backend derives the proof")
            );
        }
    }

    #[test]
    fn legacy_resource_read_digest_mismatch_is_a_precise_invalid_output_refusal() {
        let mut definition = proof_test_definition("0.6.0");
        definition.phases[0]
            .resources
            .push(PipelineInstructionSnapshot {
                id: "test-resource".into(),
                version: "1".into(),
                digest: "expected-digest".into(),
                body: "resource".into(),
                origin_refs: Vec::new(),
            });
        let mut request = proof_test_completion();
        request
            .output
            .resource_reads
            .push(PipelineSkillReadReceipt {
                instruction_id: "test-resource".into(),
                version: "1".into(),
                digest: "expected-digest".into(),
            });
        assert_eq!(request.validate(&definition), Ok(()));

        request.output.resource_reads[0].digest = "substituted-digest".into();
        let error = request.validate(&definition).unwrap_err();
        let refusal = error.refusal().expect("typed resource-read refusal");
        assert_eq!(error.code(), "INVALID_OUTPUT");
        assert_eq!(refusal.code, RefusalCode::InvalidOutput);
        assert_eq!(refusal.rule.as_deref(), Some("WP6-RESOURCE-READ-01"));
        assert_eq!(
            refusal.path.as_deref(),
            Some("arguments.params.output.resource_reads")
        );
        assert_eq!(
            refusal.expected.as_deref(),
            Some(
                "exact pinned phase resource read receipts: [test-resource@1 digest=expected-digest]"
            )
        );
        assert_eq!(
            refusal.actual.as_deref(),
            Some(
                "submitted 1 receipt(s) (1 unique, 0 duplicate): [test-resource@1 digest=substituted-digest]"
            )
        );
        assert_eq!(
            refusal.next_action.as_deref(),
            Some("supply_exact_phase_resource_reads")
        );
        assert_eq!(
            refusal.required.as_deref(),
            Some("exact_phase_resource_reads")
        );
    }
}
