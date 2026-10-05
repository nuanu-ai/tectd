use super::*;

pub(crate) fn validate_definition_constraints(
    definition: &PipelineDefinitionSnapshot,
) -> Result<()> {
    for (phase_index, phase) in definition.phases.iter().enumerate() {
        for (constraint_index, constraint) in phase.output_constraints.iter().enumerate() {
            match constraint {
                PipelineOutputConstraint::EngineeringReview {
                    standards_resource_id,
                    standards_resource_digest,
                    artifact_name,
                    required_prior_review_phase_ids,
                    required_reconciliation_phase_id,
                    ..
                } => {
                    let standards = phase.resources.iter().any(|resource| {
                        resource.id == *standards_resource_id
                            && resource.version == "1.0.0"
                            && resource.digest == *standards_resource_digest
                    });
                    let review_skill = phase.resources.iter().any(|resource| {
                        resource.id == "tect:engineering-review" && resource.version == "1.0.0"
                    });
                    let report = phase.required_artifacts.iter().any(|artifact| {
                        artifact.name_pattern == *artifact_name
                            && artifact.media_type == "application/json"
                            && artifact.required
                            && artifact.minimum_matches == 1
                            && artifact.schema_resource_id.as_deref()
                                == Some("tect:engineering-review-schema")
                    });
                    let path = format!(
                        "pipeline_definition.phases[{phase_index}].output_constraints[{constraint_index}]"
                    );
                    for (invalid, rule, expected) in [
                        (
                            !standards,
                            "WP6-ENGINEERING-REPORT-DEFINITION-STANDARDS",
                            "matching version 1.0.0 standards resource",
                        ),
                        (
                            !review_skill,
                            "WP6-ENGINEERING-REPORT-DEFINITION-SKILL",
                            "engineering review skill version 1.0.0",
                        ),
                        (
                            !report,
                            "WP6-ENGINEERING-REPORT-DEFINITION-ARTIFACT",
                            "required JSON report artifact with schema and minimum_matches=1",
                        ),
                    ] {
                        if invalid {
                            return Err(engineering_refusal(
                                RefusalCode::InputSchemaInvalid,
                                rule,
                                &path,
                                expected,
                                "missing or mismatched",
                            ));
                        }
                    }
                    for (prior_index, prior_id) in
                        required_prior_review_phase_ids.iter().enumerate()
                    {
                        let prior = definition
                            .phases
                            .iter()
                            .find(|candidate| {
                                candidate.id == *prior_id && candidate.ordinal < phase.ordinal
                            })
                            .ok_or_else(|| {
                                engineering_refusal(
                                    RefusalCode::InputSchemaInvalid,
                                    "WP6-ENGINEERING-REPORT-DEFINITION-PRIOR-PHASE",
                                    &format!(
                                        "{path}.required_prior_review_phase_ids[{prior_index}]"
                                    ),
                                    "existing earlier phase",
                                    "missing or not earlier",
                                )
                            })?;
                        if !prior.output_constraints.iter().any(|candidate| {
                            matches!(
                                candidate,
                                PipelineOutputConstraint::EngineeringReview { .. }
                            )
                        }) {
                            return Err(engineering_refusal(
                                RefusalCode::InputSchemaInvalid,
                                "WP6-ENGINEERING-REPORT-DEFINITION-PRIOR-CONSTRAINT",
                                &format!("{path}.required_prior_review_phase_ids[{prior_index}]"),
                                "earlier engineering review phase",
                                "constraint absent",
                            ));
                        }
                    }
                    if let Some(reconciliation_id) = required_reconciliation_phase_id {
                        let reconciliation = definition
                            .phases
                            .iter()
                            .find(|candidate| {
                                candidate.id == *reconciliation_id
                                    && candidate.ordinal < phase.ordinal
                            })
                            .ok_or_else(|| {
                                engineering_refusal(
                                    RefusalCode::InputSchemaInvalid,
                                    "WP6-ENGINEERING-REPORT-DEFINITION-RECONCILIATION-PHASE",
                                    &format!("{path}.required_reconciliation_phase_id"),
                                    "existing earlier phase",
                                    "missing or not earlier",
                                )
                            })?;
                        let required = [
                            "engineering_finding_ids",
                            "resolved_engineering_finding_ids",
                            "deferred_engineering_finding_ids",
                            "unresolved_engineering_finding_count",
                        ];
                        if required.iter().any(|field| {
                            !reconciliation
                                .required_fields
                                .iter()
                                .any(|value| value == field)
                        }) {
                            return Err(engineering_refusal(
                                RefusalCode::InputSchemaInvalid,
                                "WP6-ENGINEERING-REPORT-DEFINITION-RECONCILIATION-FIELDS",
                                &format!("{path}.required_reconciliation_phase_id"),
                                "reconciliation phase with all four engineering finding fields",
                                "required field absent",
                            ));
                        }
                    }
                }
                PipelineOutputConstraint::CodeAuthorization {
                    required_plan_review_phase_id,
                } => {
                    let path = format!(
                        "pipeline_definition.phases[{phase_index}].output_constraints[{constraint_index}].required_plan_review_phase_id"
                    );
                    let prior = definition
                        .phases
                        .iter()
                        .find(|candidate| {
                            candidate.id == *required_plan_review_phase_id
                                && candidate.ordinal < phase.ordinal
                        })
                        .ok_or_else(|| {
                            engineering_refusal(
                                RefusalCode::InputSchemaInvalid,
                                "WP6-ENGINEERING-REPORT-DEFINITION-PLAN-PHASE",
                                &path,
                                "existing earlier phase",
                                "missing or not earlier",
                            )
                        })?;
                    if !prior.output_constraints.iter().any(|candidate| {
                        matches!(candidate, PipelineOutputConstraint::EngineeringReview { stage, .. } if stage == "plan")
                    }) {
                        return Err(engineering_refusal(RefusalCode::InputSchemaInvalid, "WP6-ENGINEERING-REPORT-DEFINITION-PLAN-CONSTRAINT", &path, "earlier plan engineering review phase", "constraint absent"));
                    }
                }
                _ => {}
            }
        }
    }
    Ok(())
}

pub(crate) fn validate_completion_constraints(
    request: &CompletePipelinePhase,
    _definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    for constraint in &phase.output_constraints {
        match constraint {
            PipelineOutputConstraint::EngineeringReview {
                stage,
                standards_resource_digest,
                artifact_name,
                success_verdicts,
                required_prior_review_phase_ids,
                ..
            } => {
                let (artifact_index, artifact) = request
                    .output
                    .artifacts
                    .iter()
                    .enumerate()
                    .find(|(_, artifact)| artifact.name == *artifact_name)
                    .ok_or_else(|| {
                        engineering_refusal(
                            RefusalCode::InvalidOutput,
                            "WP6-ENGINEERING-REPORT-ARTIFACT-MISSING",
                            "output.artifacts",
                            "engineering review artifact required by phase",
                            "missing",
                        )
                    })?;
                let report: EngineeringReviewReport = serde_json::from_str(&artifact.body)
                    .map_err(|_| {
                        report_refusal(
                            artifact_index,
                            "WP6-ENGINEERING-REPORT-JSON",
                            "/",
                            "JSON matching engineering review report schema",
                            "invalid JSON or report schema",
                        )
                    })?;
                validate_report(
                    &report,
                    request,
                    artifact_index,
                    stage,
                    standards_resource_digest,
                    success_verdicts,
                    !required_prior_review_phase_ids.is_empty(),
                )?;
                if report.verdict == ReviewVerdict::Pass
                    && required_prior_review_phase_ids.iter().any(|required| {
                        !request
                            .consumed_outputs
                            .iter()
                            .any(|output| output.phase_id == *required)
                    })
                {
                    return Err(engineering_refusal(
                        RefusalCode::InvalidOutput,
                        "WP6-ENGINEERING-REPORT-PRIOR-CONSUMED",
                        "consumed_outputs",
                        "all required prior review phases consumed for pass",
                        "required phase absent",
                    ));
                }
            }
            PipelineOutputConstraint::CodeAuthorization {
                required_plan_review_phase_id,
            } => {
                if !request
                    .consumed_outputs
                    .iter()
                    .any(|output| output.phase_id == *required_plan_review_phase_id)
                {
                    return Err(engineering_refusal(
                        RefusalCode::InvalidOutput,
                        "WP6-ENGINEERING-REPORT-CODE-AUTHORIZATION",
                        "consumed_outputs",
                        "required plan review phase consumed",
                        "required phase absent",
                    ));
                }
            }
            _ => {}
        }
    }
    Ok(())
}
