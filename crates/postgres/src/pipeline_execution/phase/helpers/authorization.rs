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
