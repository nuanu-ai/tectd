#[cfg(test)]
mod tests {
    use super::*;

    fn navigation_phase(
        id: &str,
        ordinal: u32,
        allowed_backward_to: &[&str],
    ) -> PipelinePhaseDefinition {
        PipelinePhaseDefinition {
            id: id.into(),
            ordinal,
            title: id.into(),
            required: true,
            disposition_required: false,
            instructions: vec![],
            skills: vec![],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            required_fields: vec![],
            allowed_verdicts: vec![],
            required_dispositions: vec![],
            allowed_dispositions: vec![],
            output_constraints: vec![],
            verdict_routes: vec![],
            followup_contracts: vec![],
            allowed_backward_to: allowed_backward_to
                .iter()
                .map(|value| (*value).into())
                .collect(),
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: String::new(),
        }
    }

    fn navigation_definition(k3_allowed_backward_to: &[&str]) -> PipelineDefinitionSnapshot {
        PipelineDefinitionSnapshot {
            kind: PipelineKind::LightweightTddDevelopment,
            version: "test".into(),
            digest: "test".into(),
            overview: PipelineInstructionSnapshot {
                id: "test".into(),
                version: "test".into(),
                digest: "test".into(),
                body: String::new(),
                origin_refs: vec![],
            },
            default_mode: PipelineDeliveryMode::Phasewise,
            allowed_modes: vec![PipelineDeliveryMode::Phasewise],
            phases: vec![
                navigation_phase("K1", 1, &[]),
                navigation_phase("K2", 2, &["K1"]),
                navigation_phase("K3", 3, k3_allowed_backward_to),
            ],
            completion_contract: String::new(),
            escalation_contract: String::new(),
            forbidden_claims: vec![],
        }
    }

    fn waiting_request(revisit_phase_id: Option<&str>) -> CompletePipelinePhase {
        CompletePipelinePhase {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            run_revision: 3,
            phase_id: "K3".into(),
            outcome: PipelinePhaseOutcome::WaitingInput,
            transition: PipelineTransition::Continue,
            output: PipelinePhaseOutputDraft {
                body: String::new(),
                producer_context_id: "test".into(),
                fields: BTreeMap::new(),
                verdict: None,
                dispositions: vec![],
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
            revisit_phase_id: revisit_phase_id.map(Into::into),
            escalation_target: None,
            terminal_result: None,
            publish_blocked_result: false,
            consumed_knowledge: None,
            research_checkpoint: None,
        }
    }

    #[test]
    fn waiting_rework_moves_to_the_exact_backward_phase() {
        let definition = navigation_definition(&["K2"]);
        let phase = &definition.phases[2];
        let plan = plan_next_state(&waiting_request(Some("K2")), &definition, phase).unwrap();

        assert_eq!(plan.status, "active");
        assert_eq!(plan.next_id.as_deref(), Some("K2"));
        assert_eq!(plan.next_ordinal, Some(2));
        assert_eq!(plan.revisit_ordinal, Some(2));
    }

    #[test]
    fn waiting_without_rework_stays_on_the_current_phase() {
        let definition = navigation_definition(&["K2"]);
        let phase = &definition.phases[2];
        let plan = plan_next_state(&waiting_request(None), &definition, phase).unwrap();

        assert_eq!(plan.status, "waiting_input");
        assert_eq!(plan.next_id.as_deref(), Some("K3"));
        assert_eq!(plan.next_ordinal, Some(3));
        assert_eq!(plan.revisit_ordinal, None);
    }

    #[test]
    fn waiting_rework_rejects_missing_disallowed_and_forward_targets() {
        let missing_definition = navigation_definition(&["missing"]);
        assert!(matches!(
            plan_next_state(
                &waiting_request(Some("missing")),
                &missing_definition,
                &missing_definition.phases[2],
            ),
            Err(Error::InvalidArguments)
        ));

        let disallowed_definition = navigation_definition(&["K2"]);
        assert!(matches!(
            plan_next_state(
                &waiting_request(Some("K1")),
                &disallowed_definition,
                &disallowed_definition.phases[2],
            ),
            Err(Error::Forbidden)
        ));

        let mut forward_definition = navigation_definition(&["K3"]);
        forward_definition
            .phases
            .push(navigation_phase("K4", 4, &[]));
        forward_definition.phases[2].allowed_backward_to = vec!["K4".into()];
        assert!(matches!(
            plan_next_state(
                &waiting_request(Some("K4")),
                &forward_definition,
                &forward_definition.phases[2],
            ),
            Err(Error::Forbidden)
        ));
    }

    #[test]
    fn requirement_ledger_reports_duplicate_identity_inventory_and_modality_violations_together() {
        let body = serde_json::json!({
            "source":{"path":"spec.md","digest":"abc"},
            "sourceRequirementIds":["REQ-001","REQ-001","REQ-002"],
            "requirements":[
                {"id":"REQ-001","modality":"MUST"},
                {"id":"REQ-001","modality":"SHOULD"},
                {"id":"REQ-003","modality":"INVALID"}
            ]
        })
        .to_string();
        let error =
            parse_requirements_ledger(&body, "slice-component-decision-interrogator").unwrap_err();
        let diagnostic = error.pipeline_artifact_diagnostic().unwrap();
        assert_eq!(diagnostic.code, "requirement_ledger_invalid");
        assert_eq!(diagnostic.phase, "slice-component-decision-interrogator");
        assert_eq!(diagnostic.artifact, "requirements-ledger.json");
        assert!(diagnostic.retryable);
        assert_eq!(
            diagnostic
                .violations
                .iter()
                .map(|violation| violation.code.as_str())
                .collect::<Vec<_>>(),
            vec![
                "source_requirement_id_duplicate",
                "requirement_id_duplicate",
                "requirement_modality_invalid",
                "source_requirement_missing_row",
            ]
        );
        assert_eq!(diagnostic.violations[2].path, "$.requirements[2].modality");
        assert_eq!(
            diagnostic.violations[2].actual.as_deref(),
            Some("string:INVALID")
        );
        assert!(!diagnostic.truncated);
        assert_eq!(diagnostic.omitted_violation_count, 0);
    }

    #[test]
    fn malformed_and_large_raw_values_produce_bounded_stable_diagnostics() {
        let malformed = format!("{{\"payload\":\"{}\"", "x".repeat(500_000));
        let error = parse_requirements_ledger(&malformed, "phase-seven").unwrap_err();
        let diagnostic = error.pipeline_artifact_diagnostic().unwrap();
        assert_eq!(diagnostic.code, "requirement_ledger_invalid");
        assert_eq!(diagnostic.violations[0].code, "invalid_json");
        assert_eq!(
            diagnostic.violations[0].actual.as_deref(),
            Some("malformed_json")
        );
        assert!(serde_json::to_vec(diagnostic).unwrap().len() < 2_048);

        let raw = serde_json::json!({
            "source":{"path":"spec.md","digest":"abc"},
            "sourceRequirementIds":[{"raw":"x".repeat(500_000)}],
            "requirements":[]
        })
        .to_string();
        let error = parse_requirements_ledger(&raw, "phase-seven").unwrap_err();
        let diagnostic = error.pipeline_artifact_diagnostic().unwrap();
        assert_eq!(
            diagnostic.violations[0].code,
            "source_requirement_id_invalid"
        );
        assert_eq!(
            diagnostic.violations[0].actual.as_deref(),
            Some("object(keys=1)")
        );
        assert!(!diagnostic.truncated);
        assert_eq!(diagnostic.omitted_violation_count, 0);
        assert!(serde_json::to_vec(diagnostic).unwrap().len() < 2_048);
    }

    #[test]
    fn high_cardinality_diffs_are_capped_with_an_exact_omitted_count() {
        let ids = (0..10_000)
            .map(|index| format!("REQ-{index:03}"))
            .collect::<Vec<_>>();
        let body = serde_json::json!({
            "source":{"path":"spec.md","digest":"abc"},
            "sourceRequirementIds":ids,
            "requirements":[]
        })
        .to_string();
        let first = parse_requirements_ledger(&body, "phase-five").unwrap_err();
        let second = parse_requirements_ledger(&body, "phase-five").unwrap_err();
        let first = first.pipeline_artifact_diagnostic().unwrap();
        let second = second.pipeline_artifact_diagnostic().unwrap();
        assert_eq!(first.code, "requirement_ledger_invalid");
        assert_eq!(
            first.violations.len(),
            MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS
        );
        assert_eq!(first.omitted_violation_count, 9_976);
        assert!(first.truncated);
        assert_eq!(first, second);
        assert!(serde_json::to_vec(first).unwrap().len() < 16_384);
    }

    #[test]
    fn legacy_wrapper_preserves_nested_truncation_and_omitted_count() {
        let violations = (0..40)
            .map(|index| {
                violation(
                    "source_requirement_missing_row",
                    format!("$.requirements[{index}]"),
                    Some(format!("row for REQ-{index:03}")),
                    None,
                )
            })
            .collect::<Vec<_>>();
        let nested = Error::InvalidPipelineArtifact(Box::new(PipelineArtifactDiagnostic::bounded(
            "requirement_ledger_invalid".to_owned(),
            "slice-component-decision-interrogator".to_owned(),
            "requirements-ledger.json".to_owned(),
            violations,
            true,
            "Correct phase 5.".to_owned(),
        )));
        let wrapped = wrap_prior_ledger_error(nested);
        let diagnostic = wrapped.pipeline_artifact_diagnostic().unwrap();
        assert_eq!(diagnostic.code, "requirement_ledger_lineage_invalid");
        assert_eq!(diagnostic.violations.len(), 24);
        assert_eq!(diagnostic.omitted_violation_count, 16);
        assert!(diagnostic.truncated);
        assert!(
            diagnostic
                .violations
                .iter()
                .all(|violation| violation.path.starts_with("phase5:"))
        );
        assert!(serde_json::to_vec(diagnostic).unwrap().len() < 16_384);
    }
}
