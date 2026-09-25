#[derive(Debug)]
struct RequirementsLedger {
    source: (String, String),
    rows: BTreeMap<String, String>,
}

#[derive(Debug, Default)]
struct LedgerViolations {
    retained: Vec<PipelineArtifactViolation>,
    omitted: usize,
    inherited_truncated: bool,
}

impl LedgerViolations {
    fn push(&mut self, violation: PipelineArtifactViolation) {
        if self.retained.len() < MAX_PIPELINE_ARTIFACT_DIAGNOSTIC_VIOLATIONS {
            self.retained.push(violation);
        } else {
            self.omitted = self.omitted.saturating_add(1);
        }
    }

    fn is_empty(&self) -> bool {
        self.retained.is_empty() && self.omitted == 0
    }
}

fn text_preview(value: &str) -> String {
    const MAXIMUM: usize = 96;
    if value.len() <= MAXIMUM {
        return value.to_owned();
    }
    let mut end = MAXIMUM;
    while !value.is_char_boundary(end) {
        end -= 1;
    }
    format!("{}...[{} bytes]", &value[..end], value.len())
}

fn value_summary(value: &serde_json::Value) -> String {
    match value {
        serde_json::Value::Null => "null".to_owned(),
        serde_json::Value::Bool(value) => format!("boolean:{value}"),
        serde_json::Value::Number(value) => format!("number:{value}"),
        serde_json::Value::String(value) => {
            format!("string:{}", text_preview(value))
        }
        serde_json::Value::Array(values) => format!("array(items={})", values.len()),
        serde_json::Value::Object(values) => format!("object(keys={})", values.len()),
    }
}

fn violation(
    code: &str,
    path: impl Into<String>,
    expected: Option<String>,
    actual: Option<String>,
) -> PipelineArtifactViolation {
    PipelineArtifactViolation {
        code: code.to_owned(),
        path: path.into(),
        expected,
        actual,
    }
}

fn ledger_error(phase: &str, code: &str, violations: Vec<PipelineArtifactViolation>) -> Error {
    ledger_error_with_recovery(
        phase,
        code,
        violations,
        format!(
            "Correct requirements-ledger.json for {phase} and retry the same phase completion request with a new request_id."
        ),
    )
}

fn ledger_error_with_recovery(
    phase: &str,
    code: &str,
    violations: Vec<PipelineArtifactViolation>,
    recovery_action: String,
) -> Error {
    ledger_error_with_budget(
        phase,
        code,
        LedgerViolations {
            retained: violations,
            ..LedgerViolations::default()
        },
        recovery_action,
    )
}

fn ledger_error_with_budget(
    phase: &str,
    code: &str,
    violations: LedgerViolations,
    recovery_action: String,
) -> Error {
    Error::InvalidPipelineArtifact(Box::new(PipelineArtifactDiagnostic::bounded_with_omitted(
        code.to_owned(),
        phase.to_owned(),
        "requirements-ledger.json".to_owned(),
        violations.retained,
        violations.omitted,
        violations.inherited_truncated,
        true,
        recovery_action,
    )))
}

fn parse_requirements_ledger(
    body: &str,
    phase: &str,
) -> std::result::Result<RequirementsLedger, Error> {
    let ledger: serde_json::Value = serde_json::from_str(body).map_err(|_| {
        ledger_error(
            phase,
            "requirement_ledger_invalid",
            vec![violation(
                "invalid_json",
                "$",
                Some("valid JSON object".to_owned()),
                Some("malformed_json".to_owned()),
            )],
        )
    })?;
    let mut violations = LedgerViolations::default();
    let source = ledger.get("source").and_then(serde_json::Value::as_object);
    let source_value = |field: &str| {
        source
            .and_then(|value| value.get(field))
            .and_then(serde_json::Value::as_str)
            .filter(|value| !value.trim().is_empty() && *value == value.trim())
            .map(str::to_owned)
    };
    let source_path = source_value("path");
    let source_digest = source_value("digest");
    if source_path.is_none() {
        violations.push(violation(
            "source_path_required",
            "$.source.path",
            Some("non-empty trimmed string".to_owned()),
            ledger.pointer("/source/path").map(value_summary),
        ));
    }
    if source_digest.is_none() {
        violations.push(violation(
            "source_digest_required",
            "$.source.digest",
            Some("non-empty trimmed string".to_owned()),
            ledger.pointer("/source/digest").map(value_summary),
        ));
    }

    let mut inventory = BTreeSet::new();
    match ledger
        .get("sourceRequirementIds")
        .and_then(serde_json::Value::as_array)
    {
        Some(ids) => {
            for (index, value) in ids.iter().enumerate() {
                let id = value
                    .as_str()
                    .filter(|id| !id.trim().is_empty() && *id == id.trim());
                match id {
                    Some(id) if !inventory.insert(id.to_owned()) => violations.push(violation(
                        "source_requirement_id_duplicate",
                        format!("$.sourceRequirementIds[{index}]"),
                        Some("unique requirement ID".to_owned()),
                        Some(text_preview(id)),
                    )),
                    Some(_) => {}
                    None => violations.push(violation(
                        "source_requirement_id_invalid",
                        format!("$.sourceRequirementIds[{index}]"),
                        Some("non-empty trimmed string".to_owned()),
                        Some(value_summary(value)),
                    )),
                }
            }
        }
        None => violations.push(violation(
            "source_requirement_ids_required",
            "$.sourceRequirementIds",
            Some("array".to_owned()),
            ledger.get("sourceRequirementIds").map(value_summary),
        )),
    }

    let mut rows = BTreeMap::new();
    match ledger
        .get("requirements")
        .and_then(serde_json::Value::as_array)
    {
        Some(requirements) => {
            for (index, row) in requirements.iter().enumerate() {
                let id = row
                    .get("id")
                    .and_then(serde_json::Value::as_str)
                    .filter(|id| !id.trim().is_empty() && *id == id.trim());
                let modality = row.get("modality").and_then(serde_json::Value::as_str);
                if id.is_none() {
                    violations.push(violation(
                        "requirement_id_required",
                        format!("$.requirements[{index}].id"),
                        Some("non-empty trimmed string".to_owned()),
                        row.get("id").map(value_summary),
                    ));
                }
                if !matches!(modality, Some("MUST" | "SHOULD" | "MAY")) {
                    violations.push(violation(
                        "requirement_modality_invalid",
                        format!("$.requirements[{index}].modality"),
                        Some("MUST, SHOULD, or MAY".to_owned()),
                        row.get("modality").map(value_summary),
                    ));
                }
                if let (Some(id), Some(modality)) = (id, modality)
                    && matches!(modality, "MUST" | "SHOULD" | "MAY")
                    && rows.insert(id.to_owned(), modality.to_owned()).is_some()
                {
                    violations.push(violation(
                        "requirement_id_duplicate",
                        format!("$.requirements[{index}].id"),
                        Some("unique requirement ID".to_owned()),
                        Some(text_preview(id)),
                    ));
                }
            }
        }
        None => violations.push(violation(
            "requirements_required",
            "$.requirements",
            Some("array".to_owned()),
            ledger.get("requirements").map(value_summary),
        )),
    }

    for id in &inventory {
        if !rows.contains_key(id) {
            violations.push(violation(
                "source_requirement_missing_row",
                "$.requirements",
                Some(format!("row for {}", text_preview(id))),
                None,
            ));
        }
    }
    for id in rows.keys() {
        if !inventory.contains(id) {
            violations.push(violation(
                "requirement_id_not_in_source_inventory",
                "$.sourceRequirementIds",
                Some(format!("source inventory entry for {}", text_preview(id))),
                None,
            ));
        }
    }
    if !violations.is_empty() {
        return Err(ledger_error_with_budget(
            phase,
            "requirement_ledger_invalid",
            violations,
            format!(
                "Correct requirements-ledger.json for {phase} and retry the same phase completion request with a new request_id."
            ),
        ));
    }
    Ok(RequirementsLedger {
        source: (source_path.unwrap(), source_digest.unwrap()),
        rows,
    })
}

pub(super) fn validate_decision_requirements_ledger(
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let body = output
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "requirements-ledger.json")
        .map(|artifact| artifact.body.as_str())
        .ok_or_else(|| {
            ledger_error(
                "slice-component-decision-interrogator",
                "requirement_ledger_missing",
                vec![violation(
                    "required_artifact_missing",
                    "$.output.artifacts",
                    Some("requirements-ledger.json".to_owned()),
                    None,
                )],
            )
        })?;
    parse_requirements_ledger(body, "slice-component-decision-interrogator")?;
    Ok(())
}

pub(super) fn validate_reconciliation_requirements_ledger(
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let body = output
        .artifacts
        .iter()
        .find(|artifact| artifact.name == "requirements-ledger.json")
        .map(|artifact| artifact.body.as_str())
        .ok_or_else(|| {
            ledger_error(
                "slice-reconciliation-runner",
                "requirement_ledger_missing",
                vec![violation(
                    "required_artifact_missing",
                    "$.output.artifacts",
                    Some("requirements-ledger.json".to_owned()),
                    None,
                )],
            )
        })?;
    parse_requirements_ledger(body, "slice-reconciliation-runner")?;
    Ok(())
}
