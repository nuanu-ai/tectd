use super::*;

mod phase_instruction;
mod receipt_diff;
mod repeated;

fn snapshot(id: &str, version: &str, digest: &str) -> PipelineInstructionSnapshot {
    PipelineInstructionSnapshot {
        id: id.into(),
        version: version.into(),
        digest: digest.into(),
        body: format!("body:{id}"),
        origin_refs: vec![format!("origin:{id}")],
    }
}

fn context() -> PipelineRunContext {
    let run_id = Uuid::new_v4();
    let method = snapshot("method", "0.6.0", "method-digest");
    let skill = snapshot("skill", "0.6.0", "skill-digest");
    let phase = PipelinePhaseDefinition {
        id: "phase-1".into(),
        ordinal: 1,
        title: "Phase 1".into(),
        required: true,
        disposition_required: false,
        instructions: vec![snapshot("instruction", "0.6.0", "instruction-digest")],
        skills: vec![skill],
        resources: Vec::new(),
        required_artifacts: Vec::new(),
        validator_contracts: Vec::new(),
        required_fields: Vec::new(),
        allowed_verdicts: Vec::new(),
        required_dispositions: Vec::new(),
        allowed_dispositions: Vec::new(),
        output_constraints: Vec::new(),
        verdict_routes: Vec::new(),
        followup_contracts: Vec::new(),
        allowed_backward_to: Vec::new(),
        fresh_reviewer_input: false,
        retry_policy: PipelinePhaseRetryPolicy::Repeatable,
        output_contract: "contract".into(),
    };
    PipelineRunContext {
        run: PipelineRun {
            id: run_id,
            scope_id: Uuid::new_v4(),
            slice_id: Uuid::new_v4(),
            slice_revision: 1,
            revision: 1,
            definition_kind: PipelineKind::LightweightTddDevelopment,
            definition_version: "0.6.0".into(),
            definition_digest: "definition-digest".into(),
            selected_option_id: None,
            verification_plan_id: None,
            verification_plan_version: None,
            verification_plan_digest: None,
            delivery_mode: PipelineDeliveryMode::Phasewise,
            qualification_reason: "fixture".into(),
            status: PipelineRunStatus::Active,
            current_phase_id: Some("phase-1".into()),
            current_phase_ordinal: Some(1),
        },
        definition: PipelineDefinitionSnapshot {
            kind: PipelineKind::LightweightTddDevelopment,
            version: "0.6.0".into(),
            digest: "definition-digest".into(),
            overview: method,
            default_mode: PipelineDeliveryMode::Phasewise,
            allowed_modes: vec![PipelineDeliveryMode::Phasewise],
            phases: vec![phase.clone()],
            completion_contract: "completion".into(),
            escalation_contract: "escalation".into(),
            forbidden_claims: Vec::new(),
        },
        inquiry: None,
        source_checkpoint: None,
        checkpoints: Vec::new(),
        delivered_phases: vec![phase],
        attempts: Vec::new(),
        bindings: Vec::new(),
        outputs: Vec::new(),
        outputs_complete: true,
        inputs: Vec::new(),
        result: None,
        knowledge: None,
        knowledge_status: None,
        knowledge_resources: None,
        knowledge_resource_status: None,
        delivery_receipt: None,
        delivery_fresh: false,
    }
}

fn query(
    context: &PipelineRunContext,
    id: &str,
    version: &str,
    digest: &str,
) -> PipelineInstructionQuery {
    PipelineInstructionQuery {
        run_id: context.run.id,
        phase_id: None,
        instruction_id: Some(id.into()),
        version: Some(version.into()),
        digest: Some(digest.into()),
        refresh: Some(true),
        offset_bytes: None,
        limit_bytes: None,
        representation_digest: None,
    }
}

#[test]
fn run_plan_identity_is_optional_for_historical_contexts_and_visible_when_pinned() {
    let mut run = context().run;
    let mut legacy = serde_json::to_value(&run).unwrap();
    assert!(legacy.get("verification_plan_id").is_none());
    assert_eq!(
        serde_json::from_value::<PipelineRun>(legacy.clone()).unwrap(),
        run
    );

    let digest = "a".repeat(64);
    run.selected_option_id = Some(format!(
        "{}+verification-plan:{digest}",
        run.definition_kind.as_str()
    ));
    run.verification_plan_id = Some(format!("verification-plan:{digest}"));
    run.verification_plan_version = Some(run.definition_version.clone());
    run.verification_plan_digest = Some(digest.clone());
    let pinned = serde_json::to_value(&run).unwrap();
    assert_eq!(pinned["verification_plan_digest"], digest);
    assert_eq!(serde_json::from_value::<PipelineRun>(pinned).unwrap(), run);
    legacy["selected_option_id"] = serde_json::Value::Null;
    assert!(serde_json::from_value::<PipelineRun>(legacy).is_ok());
}

#[test]
fn instruction_query_requires_explicit_refresh() {
    let context = context();
    let mut query = query(&context, "skill", "0.6.0", "skill-digest");
    query.refresh = Some(false);
    let error = query.resolve(&context).unwrap_err();
    assert_eq!(
        error.refusal().unwrap().code,
        RefusalCode::DeliveryRefreshRequired
    );
}

#[test]
fn instruction_query_returns_only_the_exact_pinned_snapshot() {
    let context = context();
    let response = query(&context, "skill", "0.6.0", "skill-digest")
        .resolve(&context)
        .unwrap();
    assert_eq!(response.run_id, context.run.id);
    assert_eq!(response.phase_id.as_deref(), Some("phase-1"));
    assert_eq!(response.section, PipelineInstructionSection::Skill);
    assert_eq!(response.instruction.body, "body:skill");
}

#[test]
fn instruction_query_mismatch_is_typed_and_legacy_versions_still_work() {
    let context = context();
    let error = query(&context, "skill", "0.7.0", "skill-digest")
        .resolve(&context)
        .unwrap_err();
    assert_eq!(
        error.refusal().unwrap().code,
        RefusalCode::MethodVersionUnavailable
    );

    let response = query(&context, "method", "0.6.0", "method-digest")
        .resolve(&context)
        .unwrap();
    assert_eq!(response.section, PipelineInstructionSection::Method);
    assert_eq!(response.phase_id, None);
}
#[test]
fn fragment_queries_reject_other_views_and_invalid_windows() {
    let context = context();
    let mut output = PipelineRunContextQuery {
        receipt_kind: None,
        submitted_receipts: None,
        submitted_digest: None,
        run_id: context.run.id,
        view: PipelineRunContextView::Output,
        definition_digest: None,
        phase_id: None,
        run_revision: None,
        section: None,
        output_id: Some(Uuid::new_v4()),
        digest: Some("body-digest".into()),
        refresh: false,
        offset_bytes: None,
        limit_bytes: None,
        representation_digest: None,
    };
    assert!(output.validate().is_ok());
    for limit in [0, 4097, u64::MAX] {
        output.limit_bytes = Some(limit);
        let error = output.validate().unwrap_err();
        assert_eq!(
            error.refusal().unwrap().path.as_deref(),
            Some("arguments.params.limit_bytes")
        );
    }
    output.limit_bytes = Some(4096);
    output.offset_bytes = Some(1);
    assert!(output.validate().is_err());
    output.representation_digest = Some("a".repeat(64));
    assert!(output.validate().is_ok());
    output.view = PipelineRunContextView::Current;
    output.output_id = None;
    output.digest = None;
    let error = output.validate().unwrap_err();
    assert_eq!(
        error.refusal().unwrap().path.as_deref(),
        Some("arguments.params.view")
    );
    output.view = PipelineRunContextView::DeliveryReceipt;
    assert!(output.validate().is_err());
    let mut instruction = query(&context, "skill", "0.6.0", "skill-digest");
    instruction.limit_bytes = Some(1);
    assert!(instruction.validate().is_ok());
    instruction.refresh = Some(false);
    assert_eq!(
        instruction
            .resolve(&context)
            .unwrap_err()
            .refusal()
            .unwrap()
            .code,
        RefusalCode::DeliveryRefreshRequired
    );
    for value in [
        serde_json::json!(-1),
        serde_json::json!(18446744073709551616_f64),
    ] {
        let mut encoded = serde_json::to_value(&instruction).unwrap();
        encoded["offset_bytes"] = value;
        assert!(serde_json::from_value::<PipelineInstructionQuery>(encoded).is_err());
    }
}
