use super::*;

/// Read one immutable method/skill/resource body from the definition pinned to
/// a run.  The version and digest are caller supplied pins; `refresh` is an
/// explicit request for the body and is never treated as delivery proof.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineInstructionQuery {
    pub run_id: Uuid,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub instruction_id: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub version: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub digest: Option<String>,
    #[serde(default)]
    pub refresh: Option<bool>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub offset_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit_bytes: Option<u64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub representation_digest: Option<String>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum PipelineInstructionSection {
    Method,
    Instruction,
    Skill,
    Resource,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct PipelineInstructionResponse {
    pub run_id: Uuid,
    #[serde(skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
    pub section: PipelineInstructionSection,
    pub instruction: PipelineInstructionSnapshot,
}

impl PipelineInstructionQuery {
    pub fn validate(&self) -> Result<()> {
        validate_pipeline_fragment(
            self.offset_bytes,
            self.limit_bytes,
            self.representation_digest.as_deref(),
        )?;
        if self.run_id.is_nil() {
            return Err(selector_refusal(
                "WP6-INSTRUCTION-RUN-ID",
                "arguments.params.run_id",
                "non-nil run UUID",
                "nil UUID",
            ));
        }
        for (value, path) in [
            (self.phase_id.as_deref(), "arguments.params.phase_id"),
            (
                self.instruction_id.as_deref(),
                "arguments.params.instruction_id",
            ),
            (self.version.as_deref(), "arguments.params.version"),
            (self.digest.as_deref(), "arguments.params.digest"),
        ] {
            if value.is_some_and(|value| value.trim().is_empty()) {
                return Err(selector_refusal(
                    "WP6-INSTRUCTION-SELECTOR-BLANK",
                    path,
                    "nonblank selector",
                    "blank",
                ));
            }
        }
        if self.phase_id.is_some() {
            if self.instruction_id.is_some() || self.version.is_some() || self.digest.is_some() {
                return Err(selector_refusal(
                    "WP6-INSTRUCTION-SELECTOR-MIXED",
                    "arguments.params.phase_id",
                    "phase_id alone, or complete instruction_id/version/digest pins",
                    "mixed selector modes",
                ));
            }
        } else {
            for (value, path) in [
                (
                    self.instruction_id.as_ref(),
                    "arguments.params.instruction_id",
                ),
                (self.version.as_ref(), "arguments.params.version"),
                (self.digest.as_ref(), "arguments.params.digest"),
            ] {
                if value.is_none() {
                    return Err(selector_refusal(
                        "WP6-INSTRUCTION-SELECTOR-MISSING",
                        path,
                        "complete instruction_id/version/digest pins, or phase_id",
                        "missing",
                    ));
                }
            }
        }
        Ok(())
    }

    pub fn resolve(&self, context: &PipelineRunContext) -> Result<PipelineInstructionResponse> {
        self.validate()?;
        if self.refresh == Some(false) || (self.phase_id.is_none() && self.refresh != Some(true)) {
            return Err(Error::refused_at(
                RefusalCode::DeliveryRefreshRequired,
                "WP6-INSTRUCTION-REFRESH-01",
                "arguments.params.refresh",
                "true",
                "false",
                "set_refresh_true",
                "refresh",
            ));
        }

        if let Some(phase_id) = &self.phase_id {
            let phase = context
                .definition
                .phases
                .iter()
                .find(|phase| &phase.id == phase_id)
                .ok_or_else(|| {
                    selector_refusal(
                        "WP6-INSTRUCTION-PHASE-UNKNOWN",
                        "arguments.params.phase_id",
                        "phase in the run's immutable definition snapshot",
                        "unknown phase",
                    )
                })?;
            let instruction = match phase.instructions.as_slice() {
                [instruction] => instruction,
                [] => {
                    return Err(selector_refusal(
                        "WP6-INSTRUCTION-PHASE-EMPTY",
                        "arguments.params.phase_id",
                        "phase with exactly one primary instruction",
                        "primary instruction count=0",
                    ));
                }
                instructions => {
                    return Err(selector_refusal(
                        "WP6-INSTRUCTION-PHASE-AMBIGUOUS",
                        "arguments.params.phase_id",
                        "explicit instruction_id/version/digest pins for a phase with multiple primary instructions; read Current then pinned phase_contract",
                        format!("primary instruction count={}", instructions.len()),
                    ));
                }
            };
            return Ok(PipelineInstructionResponse {
                run_id: context.run.id,
                phase_id: Some(phase.id.clone()),
                section: PipelineInstructionSection::Instruction,
                instruction: instruction.clone(),
            });
        }
        let instruction_id = self
            .instruction_id
            .as_deref()
            .expect("validated pinned selector");
        let version = self.version.as_deref().expect("validated pinned selector");
        let digest = self.digest.as_deref().expect("validated pinned selector");
        let mut matches = Vec::new();
        if context.definition.overview.id == instruction_id {
            matches.push((
                None,
                PipelineInstructionSection::Method,
                &context.definition.overview,
            ));
        }
        for phase in &context.definition.phases {
            for instruction in &phase.instructions {
                if instruction.id == instruction_id {
                    matches.push((
                        Some(phase.id.clone()),
                        PipelineInstructionSection::Instruction,
                        instruction,
                    ));
                }
            }
            for skill in &phase.skills {
                if skill.id == instruction_id {
                    matches.push((
                        Some(phase.id.clone()),
                        PipelineInstructionSection::Skill,
                        skill,
                    ));
                }
            }
            for resource in &phase.resources {
                if resource.id == instruction_id {
                    matches.push((
                        Some(phase.id.clone()),
                        PipelineInstructionSection::Resource,
                        resource,
                    ));
                }
            }
        }
        let Some(first) = matches.first() else {
            return Err(method_version_unavailable(
                instruction_id,
                version,
                digest,
                None,
            ));
        };
        let (_, _, instruction) = first;
        if matches
            .iter()
            .any(|(_, _, candidate)| candidate != instruction)
            || instruction.version != version
            || instruction.digest != digest
        {
            return Err(method_version_unavailable(
                instruction_id,
                version,
                digest,
                Some(instruction),
            ));
        }
        let (phase_id, section, instruction) = matches
            .iter()
            .find(|(phase_id, _, _)| {
                phase_id.is_some() && *phase_id == context.run.current_phase_id
            })
            .unwrap_or(first);
        Ok(PipelineInstructionResponse {
            run_id: context.run.id,
            phase_id: phase_id.clone(),
            section: *section,
            instruction: (*instruction).clone(),
        })
    }
}

/// Pagination is a byte window over the complete serialized authorized read.
pub fn validate_pipeline_fragment(
    offset: Option<u64>,
    limit: Option<u64>,
    digest: Option<&str>,
) -> Result<()> {
    let invalid = if limit.is_some_and(|n| n == 0 || n > 4096) {
        Some(("limit_bytes", "integer in 1..=4096"))
    } else if offset.is_some_and(|n| usize::try_from(n).is_err()) {
        Some(("offset_bytes", "representable byte offset"))
    } else if digest.is_some_and(|v| v.len() != 64 || !v.bytes().all(|b| b.is_ascii_hexdigit()))
        || (offset.unwrap_or(0) > 0 && digest.is_none())
    {
        Some((
            "representation_digest",
            "SHA256 digest required for nonzero offset",
        ))
    } else {
        None
    };
    if let Some((field, expected)) = invalid {
        return Err(Error::refused_at(
            RefusalCode::InputSchemaInvalid,
            "PIPELINE-JSON-FRAGMENT-QUERY",
            match field {
                "limit_bytes" => "arguments.params.limit_bytes",
                "offset_bytes" => "arguments.params.offset_bytes",
                _ => "arguments.params.representation_digest",
            },
            expected,
            "invalid fragment selector",
            "supply_valid_fragment_selector",
            field,
        ));
    }
    Ok(())
}

fn method_version_unavailable(
    instruction_id: &str,
    version: &str,
    digest: &str,
    actual: Option<&PipelineInstructionSnapshot>,
) -> Error {
    let actual = actual
        .map(|instruction| format!("{}@{}", instruction.version, instruction.digest))
        .unwrap_or_else(|| "unavailable".to_owned());
    Error::Refused(Box::new(
        Refusal::new(RefusalCode::MethodVersionUnavailable)
            .with_next_action("select_pinned_instruction_version")
            .with_required("instruction_id_version_digest")
            .with_path(format!("pipeline.instruction.{instruction_id}"))
            .with_expected(format!("{version}@{digest}"))
            .with_actual(actual),
    ))
}

fn selector_refusal(
    rule: &'static str,
    path: &'static str,
    expected: &'static str,
    actual: impl Into<String>,
) -> Error {
    Error::Refused(Box::new(
        Refusal::new(RefusalCode::InputSchemaInvalid)
            .with_rule(rule)
            .with_path(path)
            .with_expected(expected)
            .with_actual(actual)
            .with_next_action("read_current_pipeline_context_and_select_pinned_instruction")
            .with_required("valid_instruction_selector"),
    ))
}
