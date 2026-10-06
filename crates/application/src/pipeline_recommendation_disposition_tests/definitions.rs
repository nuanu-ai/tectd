fn definition(kind: PipelineKind) -> PipelineDefinitionSnapshot {
    let instruction = PipelineInstructionSnapshot {
        id: "instruction".into(),
        version: "1".into(),
        digest: "instruction-digest".into(),
        body: "instruction".into(),
        origin_refs: vec!["source".into()],
    };
    PipelineDefinitionSnapshot {
        kind,
        version: "1".into(),
        digest: "definition".into(),
        overview: instruction.clone(),
        default_mode: PipelineDeliveryMode::Phasewise,
        allowed_modes: vec![PipelineDeliveryMode::Phasewise],
        phases: vec![PipelinePhaseDefinition {
            id: "proof".into(),
            ordinal: 1,
            title: "Proof".into(),
            required: true,
            disposition_required: false,
            instructions: vec![instruction],
            skills: vec![],
            resources: vec![],
            required_artifacts: vec![],
            validator_contracts: vec![],
            required_fields: vec!["proof".into()],
            allowed_verdicts: vec![],
            required_dispositions: vec![],
            allowed_dispositions: vec![],
            output_constraints: vec![],
            verdict_routes: vec![],
            followup_contracts: vec![],
            allowed_backward_to: vec![],
            fresh_reviewer_input: false,
            retry_policy: PipelinePhaseRetryPolicy::Repeatable,
            output_contract: "proof".into(),
        }],
        completion_contract: "proof".into(),
        escalation_contract: "escalation".into(),
        forbidden_claims: vec![],
    }
}

struct OpenDefinitions {
    drift: bool,
}

impl PipelineRecommendationDefinitionProvider for OpenDefinitions {
    fn definition(
        &self,
        catalogue_revision: &str,
        kind: PipelineKind,
    ) -> Result<Option<PipelineDefinitionSnapshot>> {
        if catalogue_revision != "4" {
            return Ok(None);
        }
        let mut value = definition(kind);
        if self.drift && kind == PipelineKind::LightweightTddDevelopment {
            value.version = "2".into();
            value.digest = "changed-definition".into();
        }
        Ok(Some(value))
    }
}

#[test]
fn open_guard_reloads_pinned_definitions_after_disposition() {
    let (store, _, _, _, _) = fixture(PipelineDispositionAdvice::NoCall);
    let saved = &store.basis.prepared.manifest;
    assert_eq!(
        crate::pipeline_recommendation::validate_pinned_definitions(
            &OpenDefinitions { drift: false },
            saved
        ),
        Ok(())
    );
    assert_eq!(
        crate::pipeline_recommendation::validate_pinned_definitions(
            &OpenDefinitions { drift: true },
            saved
        ),
        Err(Error::StaleContext)
    );
}
