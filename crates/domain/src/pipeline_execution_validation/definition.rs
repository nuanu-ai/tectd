use super::*;

impl PipelineDefinitionSnapshot {
    pub fn validate(&self) -> Result<()> {
        if self.version.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-DEFINITION-VERSION",
                "pipeline_definition.version",
                "nonblank version",
                "blank",
            ));
        }
        if self.digest.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-DEFINITION-DIGEST",
                "pipeline_definition.digest",
                "nonblank digest",
                "blank",
            ));
        }
        if self.completion_contract.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-DEFINITION-COMPLETION-CONTRACT",
                "pipeline_definition.completion_contract",
                "nonblank completion_contract",
                "blank",
            ));
        }
        if self.escalation_contract.trim().is_empty() {
            return Err(schema_refusal(
                "WP6-DEFINITION-ESCALATION-CONTRACT",
                "pipeline_definition.escalation_contract",
                "nonblank escalation_contract",
                "blank",
            ));
        }
        if self.phases.is_empty() {
            return Err(schema_refusal(
                "WP6-DEFINITION-PHASES",
                "pipeline_definition.phases",
                "at least one phase",
                "count=0",
            ));
        }
        if !self.allowed_modes.contains(&self.default_mode) {
            return Err(schema_refusal(
                "WP6-DEFINITION-DEFAULT-MODE",
                "pipeline_definition.default_mode",
                "mode included in allowed_modes",
                format!("{:?}", self.default_mode),
            ));
        }
        validate_instruction(&self.overview, "pipeline_definition.overview")?;
        let mut ids = BTreeSet::new();
        for (index, phase) in self.phases.iter().enumerate() {
            let path = format!("pipeline_definition.phases[{index}]");
            let fail = |rule, field: &str, expected: &str, actual: &str| {
                schema_refusal(rule, format!("{path}.{field}"), expected, actual)
            };
            if phase.id.trim().is_empty() {
                return Err(fail("WP6-PHASE-ID", "id", "nonblank id", "blank"));
            }
            if phase.title.trim().is_empty() {
                return Err(fail("WP6-PHASE-TITLE", "title", "nonblank title", "blank"));
            }
            if phase.output_contract.trim().is_empty() {
                return Err(fail(
                    "WP6-PHASE-OUTPUT-CONTRACT",
                    "output_contract",
                    "nonblank output_contract",
                    "blank",
                ));
            }
            let expected_ordinal = phase_ordinal(index)?;
            if phase.ordinal != expected_ordinal {
                return Err(schema_refusal(
                    "WP6-PHASE-ORDINAL",
                    format!("{path}.ordinal"),
                    expected_ordinal.to_string(),
                    phase.ordinal.to_string(),
                ));
            }
            if !ids.insert(phase.id.clone()) {
                return Err(fail(
                    "WP6-PHASE-ID-DUPLICATE",
                    "id",
                    "unique phase ID",
                    "duplicate",
                ));
            }
            if phase.instructions.is_empty() && phase.skills.is_empty() {
                return Err(fail(
                    "WP6-PHASE-INSTRUCTION-OR-SKILL",
                    "instructions",
                    "at least one instruction or skill",
                    "instructions=0; skills=0",
                ));
            }
            if let Some(item_index) = first_duplicate(phase.required_fields.iter()) {
                return Err(fail(
                    "WP6-PHASE-REQUIRED-FIELD-DUPLICATE",
                    &format!("required_fields[{item_index}]"),
                    "unique list entry",
                    "duplicate",
                ));
            }
            if let Some(item_index) = first_duplicate(phase.allowed_verdicts.iter()) {
                return Err(fail(
                    "WP6-PHASE-ALLOWED-VERDICT-DUPLICATE",
                    &format!("allowed_verdicts[{item_index}]"),
                    "unique list entry",
                    "duplicate",
                ));
            }
            if let Some(item_index) = first_duplicate(phase.required_dispositions.iter()) {
                return Err(fail(
                    "WP6-PHASE-REQUIRED-DISPOSITION-DUPLICATE",
                    &format!("required_dispositions[{item_index}]"),
                    "unique list entry",
                    "duplicate",
                ));
            }
            if let Some(item_index) = first_duplicate(phase.allowed_dispositions.iter()) {
                return Err(fail(
                    "WP6-PHASE-ALLOWED-DISPOSITION-DUPLICATE",
                    &format!("allowed_dispositions[{item_index}]"),
                    "unique list entry",
                    "duplicate",
                ));
            }
            if let Some(item_index) = first_duplicate(phase.verdict_routes.iter()) {
                return Err(fail(
                    "WP6-PHASE-VERDICT-ROUTE-DUPLICATE",
                    &format!("verdict_routes[{item_index}]"),
                    "unique list entry",
                    "duplicate",
                ));
            }
            if phase.disposition_required && phase.allowed_dispositions.is_empty() {
                return Err(fail(
                    "WP6-PHASE-DISPOSITION-REQUIRED",
                    "allowed_dispositions",
                    "nonempty when disposition_required",
                    "count=0",
                ));
            }
            if self.version.starts_with("0.7") && phase.required_fields.len() > 8 {
                return Err(schema_refusal(
                    "WP6-PHASE-REQUIRED-FIELD-LIMIT",
                    format!("{path}.required_fields"),
                    "at most 8 fields for 0.7 definitions",
                    format!("count={}", phase.required_fields.len()),
                ));
            }
            for (field, values, rule) in [
                (
                    "required_fields",
                    &phase.required_fields,
                    "WP6-PHASE-REQUIRED-FIELD-BLANK",
                ),
                (
                    "allowed_verdicts",
                    &phase.allowed_verdicts,
                    "WP6-PHASE-ALLOWED-VERDICT-BLANK",
                ),
                (
                    "required_dispositions",
                    &phase.required_dispositions,
                    "WP6-PHASE-REQUIRED-DISPOSITION-BLANK",
                ),
                (
                    "allowed_dispositions",
                    &phase.allowed_dispositions,
                    "WP6-PHASE-ALLOWED-DISPOSITION-BLANK",
                ),
            ] {
                if let Some(item_index) = values.iter().position(|value| value.trim().is_empty()) {
                    return Err(fail(
                        rule,
                        &format!("{field}[{item_index}]"),
                        "nonblank list entry",
                        "blank",
                    ));
                }
            }
            if !phase.allowed_dispositions.is_empty()
                && let Some(item_index) = phase
                    .required_dispositions
                    .iter()
                    .position(|value| !phase.allowed_dispositions.contains(value))
            {
                return Err(fail(
                    "WP6-PHASE-REQUIRED-DISPOSITION-MEMBERSHIP",
                    &format!("required_dispositions[{item_index}]"),
                    "member of allowed_dispositions",
                    "not allowed",
                ));
            }
            if !phase.allowed_verdicts.is_empty() {
                if phase.verdict_routes.is_empty() {
                    return Err(fail(
                        "WP6-PHASE-VERDICT-ROUTES-MISSING",
                        "verdict_routes",
                        "routes for every allowed verdict",
                        "count=0",
                    ));
                }
                if let Some(item_index) = phase.allowed_verdicts.iter().position(|verdict| {
                    !phase
                        .verdict_routes
                        .iter()
                        .any(|route| &route.verdict == verdict)
                }) {
                    return Err(fail(
                        "WP6-PHASE-VERDICT-COVERAGE",
                        &format!("allowed_verdicts[{item_index}]"),
                        "verdict with a route",
                        "route missing",
                    ));
                }
            }
            for (route_index, route) in phase.verdict_routes.iter().enumerate() {
                let route_path = format!("{path}.verdict_routes[{route_index}]");
                let route_fail = |rule, field: &str, expected: &str, actual: &str| {
                    schema_refusal(rule, format!("{route_path}.{field}"), expected, actual)
                };
                if route.verdict.trim().is_empty() {
                    return Err(route_fail(
                        "WP6-PHASE-ROUTE-VERDICT-BLANK",
                        "verdict",
                        "nonblank verdict",
                        "blank",
                    ));
                }
                if !phase.allowed_verdicts.contains(&route.verdict) {
                    return Err(route_fail(
                        "WP6-PHASE-ROUTE-VERDICT-MEMBERSHIP",
                        "verdict",
                        "member of allowed_verdicts",
                        "not allowed",
                    ));
                }
                if let Some(item_index) = first_duplicate(route.dispositions.iter()) {
                    return Err(route_fail(
                        "WP6-PHASE-ROUTE-DISPOSITION-DUPLICATE",
                        &format!("dispositions[{item_index}]"),
                        "unique disposition",
                        "duplicate",
                    ));
                }
                if let Some(item_index) = first_duplicate(route.revisit_to.iter()) {
                    return Err(route_fail(
                        "WP6-PHASE-ROUTE-REVISIT-DUPLICATE",
                        &format!("revisit_to[{item_index}]"),
                        "unique revisit target",
                        "duplicate",
                    ));
                }
                if !route.revisit_to.is_empty() {
                    if route.transition != PipelineTransition::Continue {
                        return Err(route_fail(
                            "WP6-PHASE-ROUTE-REVISIT-TRANSITION",
                            "transition",
                            "continue for nonempty revisit_to",
                            "different transition",
                        ));
                    }
                    if let Some(item_index) = route
                        .revisit_to
                        .iter()
                        .position(|id| !phase.allowed_backward_to.contains(id))
                    {
                        return Err(route_fail(
                            "WP6-PHASE-ROUTE-REVISIT-MEMBERSHIP",
                            &format!("revisit_to[{item_index}]"),
                            "member of allowed_backward_to",
                            "not allowed",
                        ));
                    }
                }
                if phase.disposition_required && route.dispositions.is_empty() {
                    return Err(route_fail(
                        "WP6-PHASE-ROUTE-DISPOSITION-REQUIRED",
                        "dispositions",
                        "at least one disposition",
                        "count=0",
                    ));
                }
                for (item_index, value) in route.dispositions.iter().enumerate() {
                    if value.trim().is_empty() {
                        return Err(route_fail(
                            "WP6-PHASE-ROUTE-DISPOSITION-BLANK",
                            &format!("dispositions[{item_index}]"),
                            "nonblank disposition",
                            "blank",
                        ));
                    }
                    if !phase.allowed_dispositions.is_empty()
                        && !phase.allowed_dispositions.contains(value)
                    {
                        return Err(route_fail(
                            "WP6-PHASE-ROUTE-DISPOSITION-MEMBERSHIP",
                            &format!("dispositions[{item_index}]"),
                            "member of allowed_dispositions",
                            "not allowed",
                        ));
                    }
                }
                if self.version.starts_with("0.7")
                    && route.outcome == PipelinePhaseOutcome::Completed
                    && let Some(item_index) = phase
                        .required_dispositions
                        .iter()
                        .position(|required| !route.dispositions.contains(required))
                {
                    return Err(route_fail(
                        "WP6-PHASE-ROUTE-COMPLETED-DISPOSITION",
                        "dispositions",
                        &format!(
                            "include required_dispositions[{item_index}] for completed 0.7 route"
                        ),
                        "required disposition absent",
                    ));
                }
                match route.transition {
                    PipelineTransition::Complete => {
                        if route.outcome != PipelinePhaseOutcome::Completed {
                            return Err(route_fail(
                                "WP6-PHASE-ROUTE-COMPLETE-OUTCOME",
                                "outcome",
                                "completed for complete transition",
                                "different outcome",
                            ));
                        }
                        if phase.ordinal as usize != self.phases.len() {
                            return Err(schema_refusal(
                                "WP6-PHASE-ROUTE-COMPLETE-ORDINAL",
                                format!("{path}.ordinal"),
                                "terminal phase for complete transition",
                                format!("ordinal={}; phases={}", phase.ordinal, self.phases.len()),
                            ));
                        }
                    }
                    PipelineTransition::Block if route.outcome != PipelinePhaseOutcome::Blocked => {
                        return Err(route_fail(
                            "WP6-PHASE-ROUTE-BLOCK-OUTCOME",
                            "outcome",
                            "blocked for block transition",
                            "different outcome",
                        ));
                    }
                    PipelineTransition::Escalate
                        if route.outcome == PipelinePhaseOutcome::WaitingInput =>
                    {
                        return Err(route_fail(
                            "WP6-PHASE-ROUTE-ESCALATE-OUTCOME",
                            "outcome",
                            "completed or blocked for escalate transition",
                            "waiting_input",
                        ));
                    }
                    _ => {}
                }
            }
            for (collection, values) in [
                ("instructions", &phase.instructions),
                ("skills", &phase.skills),
                ("resources", &phase.resources),
            ] {
                for (instruction_index, instruction) in values.iter().enumerate() {
                    let instruction_path = format!("{path}.{collection}[{instruction_index}]");
                    if self.version.starts_with("0.7") && instruction.body.len() > 4 * 1024 {
                        return Err(Error::Refused(Box::new(
                            Refusal::new(RefusalCode::PayloadTooLarge)
                                .with_message(RefusalCode::PayloadTooLarge.message())
                                .with_rule("WP6-INSTRUCTION-SIZE-01")
                                .with_path(format!("{instruction_path}.body"))
                                .with_expected("at most 4096 UTF-8 bytes")
                                .with_actual(instruction.body.len().to_string())
                                .with_next_action("reduce_instruction_body")
                                .with_required("instruction_body"),
                        )));
                    }
                    validate_instruction(instruction, &instruction_path)?;
                }
            }
            validate_artifact_definition(phase).map_err(|error| {
                crate::pipeline_artifacts::prefix_definition_refusal(error, index)
            })?;
            validate_followup_definitions(self, phase)?;
            for constraint in &phase.output_constraints {
                validate_output_constraint(phase, constraint)?;
            }
        }
        for (index, phase) in self.phases.iter().enumerate() {
            if let Some(target_index) = phase.allowed_backward_to.iter().position(|id| {
                self.phases
                    .iter()
                    .find(|candidate| &candidate.id == id)
                    .is_none_or(|target| target.ordinal >= phase.ordinal)
            }) {
                return Err(schema_refusal(
                    "WP6-PHASE-BACKWARD-TARGET",
                    format!(
                        "pipeline_definition.phases[{index}].allowed_backward_to[{target_index}]"
                    ),
                    "existing strictly earlier phase",
                    "missing or not earlier",
                ));
            }
        }
        validate_definition_constraints(self)?;
        Ok(())
    }
}

fn validate_instruction(value: &PipelineInstructionSnapshot, path: &str) -> Result<()> {
    if value.id.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-INSTRUCTION-ID",
            format!("{path}.id"),
            "nonblank id",
            "blank",
        ));
    }
    if value.version.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-INSTRUCTION-VERSION",
            format!("{path}.version"),
            "nonblank version",
            "blank",
        ));
    }
    if value.digest.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-INSTRUCTION-DIGEST",
            format!("{path}.digest"),
            "nonblank digest",
            "blank",
        ));
    }
    if value.body.trim().is_empty() {
        return Err(schema_refusal(
            "WP6-INSTRUCTION-BODY",
            format!("{path}.body"),
            "nonblank body",
            "blank",
        ));
    }
    if value.origin_refs.is_empty() {
        return Err(schema_refusal(
            "WP6-INSTRUCTION-ORIGINS",
            format!("{path}.origin_refs"),
            "at least one origin reference",
            "count=0",
        ));
    }
    if let Some(index) = value
        .origin_refs
        .iter()
        .position(|value| value.trim().is_empty())
    {
        return Err(schema_refusal(
            "WP6-INSTRUCTION-ORIGIN-BLANK",
            format!("{path}.origin_refs[{index}]"),
            "nonblank origin reference",
            "blank",
        ));
    }
    Ok(())
}
