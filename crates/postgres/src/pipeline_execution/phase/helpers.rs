use super::*;
mod engineering_findings;
use std::collections::{BTreeMap, BTreeSet};

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

fn wrap_prior_ledger_error(error: Error) -> Error {
    match error {
        Error::InvalidPipelineArtifact(diagnostic) => {
            let mut violations = LedgerViolations {
                omitted: diagnostic.omitted_violation_count,
                inherited_truncated: diagnostic.truncated,
                ..LedgerViolations::default()
            };
            for mut violation in diagnostic.violations {
                violation.path = format!("phase5:{}", violation.path);
                violations.push(violation);
            }
            ledger_error_with_budget(
                "slice-reconciliation-runner",
                "requirement_ledger_lineage_invalid",
                violations,
                "Submit a valid phase 7 output with completed/continue and revisit_phase_id slice-component-decision-interrogator; then rework phase 5 and rerun phases 6 and 7."
                    .to_owned(),
            )
        }
        other => other,
    }
}

pub(super) async fn validate_reconciliation_ledger_lineage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let prior_artifacts: serde_json::Value = sqlx::query_scalar(
        "SELECT o.artifacts FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id='slice-component-decision-interrogator' AND b.stale=false AND NOT o.payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::Forbidden)?;
    let prior_body = prior_artifacts
        .as_array()
        .and_then(|artifacts| {
            artifacts
                .iter()
                .find(|artifact| artifact["name"] == "requirements-ledger.json")
        })
        .and_then(|artifact| artifact["body"].as_str())
        .ok_or_else(|| {
            ledger_error(
                "slice-reconciliation-runner",
                "requirement_ledger_lineage_unavailable",
                vec![violation(
                    "prior_requirement_ledger_missing",
                    "phase:slice-component-decision-interrogator/requirements-ledger.json",
                    Some("non-stale phase 5 requirements ledger".to_owned()),
                    None,
                )],
            )
        })?;
    let current_body = output
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
    let prior = parse_requirements_ledger(prior_body, "slice-component-decision-interrogator")
        .map_err(wrap_prior_ledger_error)?;
    let current = parse_requirements_ledger(current_body, "slice-reconciliation-runner")?;
    let mut violations = LedgerViolations::default();
    if prior.source.0 != current.source.0 {
        violations.push(violation(
            "source_path_mismatch",
            "$.source.path",
            Some(text_preview(&prior.source.0)),
            Some(text_preview(&current.source.0)),
        ));
    }
    if prior.source.1 != current.source.1 {
        violations.push(violation(
            "source_digest_mismatch",
            "$.source.digest",
            Some(text_preview(&prior.source.1)),
            Some(text_preview(&current.source.1)),
        ));
    }
    for (id, modality) in &prior.rows {
        match current.rows.get(id) {
            None => violations.push(violation(
                "prior_requirement_missing",
                format!("$.requirements[{}]", text_preview(id)),
                Some(format!("preserved {modality} requirement")),
                None,
            )),
            Some(current_modality) if modality == "MUST" && current_modality != "MUST" => {
                violations.push(violation(
                    "must_modality_changed",
                    format!("$.requirements[{}].modality", text_preview(id)),
                    Some("MUST".to_owned()),
                    Some(current_modality.clone()),
                ));
            }
            _ => {}
        }
    }
    if !violations.is_empty() {
        return Err(ledger_error_with_budget(
            "slice-reconciliation-runner",
            "requirement_ledger_lineage_invalid",
            violations,
            "Correct requirements-ledger.json for slice-reconciliation-runner and retry the same phase completion request with a new request_id."
                .to_owned(),
        ));
    }
    Ok(())
}

pub(super) async fn validate_consumed_outputs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    current_ordinal: u32,
    supplied: &[PipelineConsumedOutput],
) -> Result<()> {
    let rows:Vec<(String,i64,String)>=sqlx::query_as("SELECT b.phase_id,b.output_revision,o.body_digest FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal<$4 AND b.stale=false ORDER BY b.phase_ordinal")
        .bind(tenant).bind(workspace).bind(run).bind(current_ordinal as i32).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = rows
        .into_iter()
        .map(|row| PipelineConsumedOutput {
            phase_id: row.0,
            output_revision: row.1,
            digest: row.2,
        })
        .collect::<Vec<_>>();
    if expected == supplied {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(super) async fn validate_consumed_inputs(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase_id: &str,
    supplied: &[PipelineConsumedInput],
) -> Result<()> {
    let rows:Vec<(Uuid,i64,String)>=sqlx::query_as("SELECT id,sequence,input_digest FROM slice_pipeline_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4 ORDER BY sequence")
        .bind(tenant).bind(workspace).bind(run).bind(phase_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = rows
        .into_iter()
        .map(|row| PipelineConsumedInput {
            input_id: row.0,
            sequence: row.1,
            digest: row.2,
        })
        .collect::<Vec<_>>();
    if expected == supplied {
        Ok(())
    } else {
        Err(Error::StaleContext)
    }
}

pub(super) async fn validate_review_authorization(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let mut required = Vec::new();
    let mut specification_lineage: Option<(Vec<String>, String)> = None;
    for constraint in &phase.output_constraints {
        match constraint {
            PipelineOutputConstraint::EngineeringReview {
                stage,
                success_verdicts,
                required_prior_review_phase_ids,
                required_reconciliation_phase_id,
                ..
            } if output
                .verdict
                .as_deref()
                .is_some_and(|value| success_verdicts.iter().any(|success| success == value)) =>
            {
                required.extend(required_prior_review_phase_ids.iter().cloned());
                if stage == "specification"
                    && let Some(reconciliation_id) = required_reconciliation_phase_id
                {
                    specification_lineage = Some((
                        required_prior_review_phase_ids.clone(),
                        reconciliation_id.clone(),
                    ));
                }
            }
            PipelineOutputConstraint::CodeAuthorization {
                required_plan_review_phase_id,
            } => {
                required.push(required_plan_review_phase_id.clone());
            }
            _ => {}
        }
    }
    required.sort_unstable();
    required.dedup();
    for required_id in required {
        let prior = definition
            .phases
            .iter()
            .find(|candidate| candidate.id == required_id)
            .ok_or(Error::InternalInvariant)?;
        let accepted = prior
            .output_constraints
            .iter()
            .find_map(|constraint| match constraint {
                PipelineOutputConstraint::EngineeringReview {
                    success_verdicts, ..
                } => Some(success_verdicts),
                _ => None,
            })
            .ok_or(Error::InternalInvariant)?;
        let prior_verdict: Option<String> = sqlx::query_scalar(
            "SELECT o.verdict FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND b.stale=false AND NOT o.payload_erased",
        )
        .bind(tenant).bind(workspace).bind(run).bind(&required_id)
        .fetch_optional(&mut **tx).await.map_err(storage_error)?
        .flatten();
        if prior_verdict
            .as_ref()
            .is_none_or(|value| !accepted.contains(value))
        {
            return Err(Error::Forbidden);
        }
    }
    if let Some((prior_phase_ids, reconciliation_phase_id)) = specification_lineage {
        engineering_findings::validate_specification_finding_lineage(
            tx,
            tenant,
            workspace,
            run,
            output,
            &prior_phase_ids,
            &reconciliation_phase_id,
        )
        .await?;
    }
    Ok(())
}

pub(super) async fn validate_reviewer_boundary(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase: &PipelinePhaseDefinition,
    request: &CompletePipelinePhase,
) -> Result<()> {
    if !phase.fresh_reviewer_input {
        return Ok(());
    }
    let attestation = request
        .output
        .reviewer_context
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let expected: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT o.producer_context_id FROM slice_pipeline_output_bindings b JOIN slice_pipeline_phase_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_ordinal<$4 AND b.stale=false ORDER BY o.producer_context_id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .bind(phase.ordinal as i32)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut supplied = attestation.producer_context_ids.clone();
    supplied.sort_unstable();
    supplied.dedup();
    if attestation.reviewer_context_id != request.output.producer_context_id
        || expected != supplied
        || expected.contains(&request.output.producer_context_id)
        || supplied.len() != attestation.producer_context_ids.len()
    {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}

pub(super) async fn enforce_retry_policy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase: &PipelinePhaseDefinition,
) -> Result<()> {
    let attempts:i64=sqlx::query_scalar("SELECT COUNT(*) FROM slice_pipeline_phase_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_id=$4")
        .bind(tenant).bind(workspace).bind(run).bind(&phase.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if attempts == 0 {
        return Ok(());
    }
    match phase.retry_policy {
        PipelinePhaseRetryPolicy::Repeatable => Ok(()),
        PipelinePhaseRetryPolicy::ExactReplayOnly => Err(Error::Forbidden),
        PipelinePhaseRetryPolicy::ReconciliationRequired => {
            let reconciled:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM slice_pipeline_inputs i WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND i.run_id=$3 AND i.phase_id=$4 AND i.created_at>(SELECT MAX(a.created_at) FROM slice_pipeline_phase_attempts a WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.run_id=$3 AND a.phase_id=$4))")
                .bind(tenant).bind(workspace).bind(run).bind(&phase.id).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if reconciled {
                Ok(())
            } else {
                Err(Error::InputPending)
            }
        }
    }
}

pub(super) struct PlannedNextState {
    pub(super) status: &'static str,
    pub(super) next_id: Option<String>,
    pub(super) next_ordinal: Option<u32>,
    pub(super) revisit_ordinal: Option<u32>,
}

fn resolve_revisit<'a>(
    request: &CompletePipelinePhase,
    definition: &'a PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<Option<&'a PipelinePhaseDefinition>> {
    let Some(revisit) = &request.revisit_phase_id else {
        return Ok(None);
    };
    if !phase.allowed_backward_to.contains(revisit) {
        return Err(Error::Forbidden);
    }
    let target = definition
        .phases
        .iter()
        .find(|candidate| &candidate.id == revisit)
        .ok_or(Error::InvalidArguments)?;
    if target.ordinal >= phase.ordinal {
        return Err(Error::Forbidden);
    }
    Ok(Some(target))
}

pub(super) fn plan_next_state(
    request: &CompletePipelinePhase,
    definition: &PipelineDefinitionSnapshot,
    phase: &PipelinePhaseDefinition,
) -> Result<PlannedNextState> {
    if request.outcome == PipelinePhaseOutcome::WaitingInput {
        if request.transition != PipelineTransition::Continue {
            return Err(Error::InvalidArguments);
        }
        if let Some(target) = resolve_revisit(request, definition, phase)? {
            return Ok(PlannedNextState {
                status: "active",
                next_id: Some(target.id.clone()),
                next_ordinal: Some(target.ordinal),
                revisit_ordinal: Some(target.ordinal),
            });
        }
        return Ok(PlannedNextState {
            status: "waiting_input",
            next_id: Some(phase.id.clone()),
            next_ordinal: Some(phase.ordinal),
            revisit_ordinal: None,
        });
    }
    if request.outcome == PipelinePhaseOutcome::Blocked
        && request.transition == PipelineTransition::Continue
    {
        return Ok(PlannedNextState {
            status: "blocked",
            next_id: Some(phase.id.clone()),
            next_ordinal: Some(phase.ordinal),
            revisit_ordinal: None,
        });
    }
    match request.transition {
        PipelineTransition::Continue => {
            if request.outcome != PipelinePhaseOutcome::Completed {
                return Err(Error::InvalidArguments);
            }
            let revisit = resolve_revisit(request, definition, phase)?;
            let next = if let Some(revisit) = revisit {
                revisit
            } else {
                definition
                    .phases
                    .iter()
                    .find(|value| value.ordinal == phase.ordinal + 1)
                    .ok_or(Error::InvalidArguments)?
            };
            Ok(PlannedNextState {
                status: "active",
                next_id: Some(next.id.clone()),
                next_ordinal: Some(next.ordinal),
                revisit_ordinal: revisit.map(|_| next.ordinal),
            })
        }
        PipelineTransition::Complete => {
            if request.outcome != PipelinePhaseOutcome::Completed
                || phase.ordinal as usize != definition.phases.len()
            {
                return Err(Error::Forbidden);
            }
            Ok(PlannedNextState {
                status: "completed",
                next_id: None,
                next_ordinal: None,
                revisit_ordinal: None,
            })
        }
        PipelineTransition::Block => {
            if request.outcome != PipelinePhaseOutcome::Blocked {
                return Err(Error::InvalidArguments);
            }
            Ok(PlannedNextState {
                status: "blocked",
                next_id: Some(phase.id.clone()),
                next_ordinal: Some(phase.ordinal),
                revisit_ordinal: None,
            })
        }
        PipelineTransition::Escalate => {
            if request.outcome == PipelinePhaseOutcome::WaitingInput {
                return Err(Error::InvalidArguments);
            }
            Ok(PlannedNextState {
                status: "escalated",
                next_id: None,
                next_ordinal: None,
                revisit_ordinal: None,
            })
        }
    }
}

pub(super) async fn apply_rework(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    run: Uuid,
    plan: &PlannedNextState,
) -> Result<()> {
    let Some(target_ordinal) = plan.revisit_ordinal else {
        return Ok(());
    };
    let target_id = plan.next_id.as_deref().ok_or(Error::InternalInvariant)?;
    stale_from_ordinal(
        tx,
        tenant,
        workspace,
        session,
        run,
        target_ordinal,
        &format!("rework_from:{target_id}"),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn stale_from_ordinal(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    run: Uuid,
    target_ordinal: u32,
    reason: &str,
) -> Result<()> {
    sqlx::query("UPDATE slice_pipeline_output_bindings SET stale=true,stale_reason=$4,updated_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND phase_ordinal>=$5")
        .bind(tenant).bind(workspace).bind(run).bind(reason).bind(target_ordinal as i32)
        .execute(&mut **tx).await.map_err(storage_error)?;
    checkpoint::mark_superseded_after_rework(tx, tenant, workspace, run, target_ordinal, session)
        .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn publish_result(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    request: &CompletePipelinePhase,
    attempt_id: Uuid,
    run: &LockedRun,
    origin: &str,
    outcome: SliceResultOutcome,
) -> Result<SliceResult> {
    let draft = request
        .terminal_result
        .as_ref()
        .ok_or(Error::InvalidArguments)?;
    let slice_revision:i64=sqlx::query_scalar("SELECT revision FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(run.1).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let revision:i64=sqlx::query_scalar("SELECT COALESCE(MAX(revision),0)+1 FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND slice_id=$3")
        .bind(tenant).bind(workspace).bind(run.1).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let id = Uuid::new_v4();
    let state = if outcome == SliceResultOutcome::Completed {
        "completed"
    } else {
        "blocked"
    };
    sqlx::query("INSERT INTO slice_results(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,outcome,summary,evidence,scope_impact,remaining_work,provenance,request_id,request_payload,pipeline_run_id,pipeline_definition_version,pipeline_definition_digest,pipeline_final_attempt_id,pipeline_result_origin) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,'externally_reported',$13,$14,$15,$16,$17,$18,$19)")
        .bind(id).bind(tenant).bind(workspace).bind(run.0).bind(run.1).bind(slice_revision).bind(revision).bind(state)
        .bind(&draft.summary).bind(json(&draft.evidence)?).bind(&draft.scope_impact).bind(&draft.remaining_work).bind(request.request_id).bind(json(request)?)
        .bind(request.run_id).bind(&run.5).bind(&run.6).bind(attempt_id).bind(origin).execute(&mut **tx).await.map_err(storage_error)?;
    let next_slice_revision = slice_revision
        .checked_add(1)
        .ok_or(Error::StorageUnavailable)?;
    sqlx::query("UPDATE native_slices SET revision=$4,state=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run.1).bind(next_slice_revision).bind(state).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_pipeline_runs SET slice_revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(request.run_id).bind(next_slice_revision).execute(&mut **tx).await.map_err(storage_error)?;
    let set_id:Uuid=sqlx::query_scalar("SELECT slice_candidate_set_id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(run.0).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let set:(i64,i64)=sqlx::query_as("SELECT revision,latest_input FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(set_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let next_input = set.1.checked_add(1).ok_or(Error::StorageUnavailable)?;
    sqlx::query("INSERT INTO slice_planning_inputs(tenant_id,workspace_id,candidate_set_id,sequence,session_id,source_result_id,input) VALUES($1,$2,$3,$4,$5,$6,$7)")
        .bind(tenant).bind(workspace).bind(set_id).bind(next_input).bind(session).bind(id).bind(format!("Pipeline-managed Slice Result {id}: {}",draft.summary))
        .execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE slice_candidate_sets SET revision=revision+1,status='review_required',latest_input=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(set_id).bind(next_input).execute(&mut **tx).await.map_err(storage_error)?;
    sqlx::query("UPDATE native_scopes SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(run.0).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(SliceResult {
        id,
        slice_id: run.1,
        slice_revision,
        revision,
        outcome,
        summary: draft.summary.clone(),
        evidence: draft.evidence.clone(),
        scope_impact: draft.scope_impact.clone(),
        remaining_work: draft.remaining_work.clone(),
        provenance: "externally_reported".into(),
        pipeline_run_id: Some(request.run_id),
        pipeline_definition_version: Some(run.5.clone()),
        pipeline_definition_digest: Some(run.6.clone()),
        pipeline_final_attempt_id: Some(attempt_id),
        pipeline_result_origin: Some(origin.into()),
        knowledge_provenance: None,
    })
}

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
