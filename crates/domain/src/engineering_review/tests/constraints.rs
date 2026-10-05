use super::*;

fn phase() -> PipelinePhaseDefinition {
    serde_json::from_value(json!({"id":"review", "ordinal":2, "title":"review", "required":true,
        "disposition_required":false, "instructions":[], "skills":[], "required_fields":[],
        "allowed_verdicts":["PASS"], "required_dispositions":[], "allowed_backward_to":[],
        "fresh_reviewer_input":false, "retry_policy":"repeatable", "output_contract":"review",
        "resources":[{"id":"standards", "version":"1.0.0", "digest":"rules", "body":"rules", "origin_refs":[]},
            {"id":"tect:engineering-review", "version":"1.0.0", "digest":"skill", "body":"review", "origin_refs":[]}],
        "required_artifacts":[{"name_pattern":"engineering-review.json","media_type":"application/json","required":true,
            "minimum_matches":1,"schema_resource_id":"tect:engineering-review-schema"}],
        "output_constraints":[{"kind":"engineering_review","stage":"specification","standards_resource_id":"standards",
            "standards_resource_digest":"rules","artifact_name":"engineering-review.json","success_verdicts":["PASS"]}]
    })).unwrap()
}
fn definition(phases: Vec<PipelinePhaseDefinition>) -> PipelineDefinitionSnapshot {
    PipelineDefinitionSnapshot {
        kind: PipelineKind::FullDesignToExecution,
        version: "1".into(),
        digest: "def".into(),
        overview: PipelineInstructionSnapshot {
            id: "overview".into(),
            version: "1".into(),
            digest: "digest".into(),
            body: "body".into(),
            origin_refs: vec![],
        },
        default_mode: PipelineDeliveryMode::Phasewise,
        allowed_modes: vec![PipelineDeliveryMode::Phasewise],
        phases,
        completion_contract: "complete".into(),
        escalation_contract: "escalate".into(),
        forbidden_claims: vec![],
    }
}
fn report_artifact(body: String) -> PipelinePhaseArtifactDraft {
    PipelinePhaseArtifactDraft {
        name: "engineering-review.json".into(),
        media_type: "application/json".into(),
        body,
        digest: "digest".into(),
        reference: None,
    }
}
#[test]
fn engineering_completion_missing_artifact_json_and_verdict_are_distinct() {
    let phase = phase();
    let definition = definition(vec![phase.clone()]);
    let mut req = request();
    let missing = validate_completion_constraints(&req, &definition, &phase)
        .unwrap_err()
        .refusal()
        .unwrap();
    assert_eq!(
        missing.rule.as_deref(),
        Some("WP6-ENGINEERING-REPORT-ARTIFACT-MISSING")
    );
    assert_eq!(missing.path.as_deref(), Some("output.artifacts"));
    req.output.artifacts = vec![
        PipelinePhaseArtifactDraft {
            name: "other.md".into(),
            ..report_artifact("ignored".into())
        },
        report_artifact("秘密 malformed".into()),
    ];
    let json_error = validate_completion_constraints(&req, &definition, &phase).unwrap_err();
    let invalid = json_error.refusal().unwrap();
    assert_eq!(invalid.rule.as_deref(), Some("WP6-ENGINEERING-REPORT-JSON"));
    assert_eq!(invalid.path.as_deref(), Some("output.artifacts[1].body"));
    assert!(!serde_json::to_string(&json_error).unwrap().contains("秘密"));
    req.output.artifacts[1] = report_artifact(report().to_string());
    assert!(validate_completion_constraints(&req, &definition, &phase).is_ok());
    req.output.verdict = None;
    let refusal = validate_completion_constraints(&req, &definition, &phase)
        .unwrap_err()
        .refusal()
        .unwrap();
    assert_eq!(
        refusal.rule.as_deref(),
        Some("WP6-ENGINEERING-REPORT-OUTPUT-VERDICT")
    );
    assert_eq!(refusal.path.as_deref(), Some("output.verdict"));
}
#[test]
fn engineering_completion_prior_review_and_code_authority_are_required() {
    let mut phase = phase();
    let definition = definition(vec![phase.clone()]);
    let mut req = request();
    req.output.artifacts = vec![report_artifact(report().to_string())];
    if let PipelineOutputConstraint::EngineeringReview {
        required_prior_review_phase_ids,
        ..
    } = &mut phase.output_constraints[0]
    {
        required_prior_review_phase_ids.push("prior".into());
    }
    let refusal = validate_completion_constraints(&req, &definition, &phase)
        .unwrap_err()
        .refusal()
        .unwrap();
    assert_eq!(
        refusal.rule.as_deref(),
        Some("WP6-ENGINEERING-REPORT-PRIOR-CONSUMED")
    );
    assert_eq!(refusal.code, RefusalCode::InvalidOutput);
    req.consumed_outputs.push(PipelineConsumedOutput {
        phase_id: "prior".into(),
        output_revision: 1,
        digest: "digest".into(),
    });
    let mut value = report();
    value["reviewed_outputs"] = serde_json::to_value(&req.consumed_outputs).unwrap();
    req.output.artifacts[0].body = value.to_string();
    assert!(validate_completion_constraints(&req, &definition, &phase).is_ok());
    phase.output_constraints = vec![PipelineOutputConstraint::CodeAuthorization {
        required_plan_review_phase_id: "plan".into(),
    }];
    let refusal = validate_completion_constraints(&req, &definition, &phase)
        .unwrap_err()
        .refusal()
        .unwrap();
    assert_eq!(
        refusal.rule.as_deref(),
        Some("WP6-ENGINEERING-REPORT-CODE-AUTHORIZATION")
    );
    req.consumed_outputs[0].phase_id = "plan".into();
    assert!(validate_completion_constraints(&req, &definition, &phase).is_ok());
}
#[test]
fn engineering_definition_resources_and_prior_phase_authority_are_checked() {
    let valid = phase();
    assert!(validate_definition_constraints(&definition(vec![valid.clone()])).is_ok());
    for (remove, suffix) in [(0, "STANDARDS"), (1, "SKILL")] {
        let mut changed = valid.clone();
        changed.resources.remove(remove);
        let refusal = validate_definition_constraints(&definition(vec![changed]))
            .unwrap_err()
            .refusal()
            .unwrap();
        assert_eq!(refusal.code, RefusalCode::InputSchemaInvalid);
        assert_eq!(
            refusal.rule.as_deref(),
            Some(format!("WP6-ENGINEERING-REPORT-DEFINITION-{suffix}").as_str())
        );
        assert_eq!(
            refusal.path.as_deref(),
            Some("pipeline_definition.phases[0].output_constraints[0]")
        );
    }
    let mut changed = valid.clone();
    changed.required_artifacts.clear();
    assert_eq!(
        validate_definition_constraints(&definition(vec![changed]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-ARTIFACT")
    );
    let mut changed = valid.clone();
    if let PipelineOutputConstraint::EngineeringReview {
        required_prior_review_phase_ids,
        ..
    } = &mut changed.output_constraints[0]
    {
        required_prior_review_phase_ids.push("prior".into());
    }
    assert_eq!(
        validate_definition_constraints(&definition(vec![changed.clone()]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-PRIOR-PHASE")
    );
    let mut prior = valid.clone();
    prior.id = "prior".into();
    prior.ordinal = 1;
    prior.output_constraints.clear();
    assert_eq!(
        validate_definition_constraints(&definition(vec![prior.clone(), changed.clone()]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-PRIOR-CONSTRAINT")
    );
    prior.output_constraints = valid.output_constraints.clone();
    assert!(validate_definition_constraints(&definition(vec![prior.clone(), changed])).is_ok());
    let mut changed = valid.clone();
    if let PipelineOutputConstraint::EngineeringReview {
        required_reconciliation_phase_id,
        ..
    } = &mut changed.output_constraints[0]
    {
        *required_reconciliation_phase_id = Some("prior".into());
    }
    assert_eq!(
        validate_definition_constraints(&definition(vec![changed.clone()]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-RECONCILIATION-PHASE")
    );
    assert_eq!(
        validate_definition_constraints(&definition(vec![prior.clone(), changed.clone()]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-RECONCILIATION-FIELDS")
    );
    prior.required_fields = vec![
        "engineering_finding_ids".into(),
        "resolved_engineering_finding_ids".into(),
        "deferred_engineering_finding_ids".into(),
        "unresolved_engineering_finding_count".into(),
    ];
    assert!(validate_definition_constraints(&definition(vec![prior.clone(), changed])).is_ok());
    let mut changed = valid;
    changed.output_constraints = vec![PipelineOutputConstraint::CodeAuthorization {
        required_plan_review_phase_id: "prior".into(),
    }];
    assert_eq!(
        validate_definition_constraints(&definition(vec![changed.clone()]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-PLAN-PHASE")
    );
    assert_eq!(
        validate_definition_constraints(&definition(vec![prior.clone(), changed.clone()]))
            .unwrap_err()
            .refusal()
            .unwrap()
            .rule
            .as_deref(),
        Some("WP6-ENGINEERING-REPORT-DEFINITION-PLAN-CONSTRAINT")
    );
    if let PipelineOutputConstraint::EngineeringReview { stage, .. } =
        &mut prior.output_constraints[0]
    {
        *stage = "plan".into();
    }
    assert!(validate_definition_constraints(&definition(vec![prior, changed])).is_ok());
}
