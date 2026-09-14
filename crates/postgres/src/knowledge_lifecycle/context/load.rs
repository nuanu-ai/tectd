use super::*;
use sqlx::Row;
use std::collections::BTreeSet;

type OperationRow = (
    Uuid,
    String,
    String,
    Uuid,
    Option<i64>,
    Option<String>,
    Vec<Uuid>,
);

pub(crate) async fn load_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change_id: Uuid,
) -> Result<Option<KnowledgeChangeContext>> {
    let row = sqlx::query(
        "SELECT r.id,r.revision,r.definition,r.definition_version,r.definition_digest,\
                r.delivery_mode,r.status,r.current_phase_id,c.owner,r.baseline,r.branch_plan,\
                r.ready_to_commit,r.publisher_receipt,r.erased_publisher_receipt,r.effects_report,r.erased_effects_report,r.result,r.erased_result,\
                c.intent,c.desired_outcome,c.completion,c.sources,c.source_pins,c.operation_hints,\
                r.payload_erased,c.payload_erased,c.source_revision,r.erased_no_change_proof \
         FROM knowledge_change_runs r JOIN knowledge_lifecycle_changes c \
           ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.id=r.change_id \
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.change_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    let run_id: Uuid = row.get(0);
    let definition: KnowledgeChangeDefinition = decode(row.get(2))?;
    let current_phase_id = row
        .try_get::<Option<String>, _>(7)
        .map_err(storage_error)?
        .map(|value| phase(&value))
        .transpose()?;
    let mode: PipelineDeliveryMode = decode(serde_json::Value::String(row.get(5)))?;
    let plan: Option<KnowledgeBranchPlan> = row
        .try_get::<Option<serde_json::Value>, _>(10)
        .map_err(storage_error)?
        .map(decode)
        .transpose()?;
    let allowed_methods = plan.as_ref().map(|value| {
        value
            .method_refs
            .iter()
            .map(|method| (&method.id, &method.version, &method.digest))
            .collect::<BTreeSet<_>>()
    });
    let filter_phase = |mut value: KnowledgeChangePhaseDefinition| {
        value.methods.retain(|method| {
            value.id.method_id() == Some(method.id.as_str())
                || allowed_methods.as_ref().is_some_and(|allowed| {
                    allowed.contains(&(&method.id, &method.version, &method.digest))
                })
        });
        if let Some(plan) = &plan {
            value.required_obligation_ids = plan
                .obligations
                .iter()
                .filter(|obligation| obligation.phase_id == value.id)
                .map(|obligation| obligation.obligation_id.clone())
                .collect();
        }
        value
    };
    let delivered_phases = match mode {
        PipelineDeliveryMode::Whole => definition
            .phases
            .clone()
            .into_iter()
            .map(filter_phase)
            .collect(),
        PipelineDeliveryMode::Phasewise => current_phase_id
            .and_then(|id| {
                definition
                    .phases
                    .iter()
                    .find(|value| value.id == id)
                    .cloned()
                    .map(filter_phase)
            })
            .into_iter()
            .collect(),
    };
    let run = KnowledgeChangeRun {
        id: run_id,
        change_id,
        workspace_id: workspace,
        revision: row.get(1),
        definition_version: row.get(3),
        definition_digest: row.get(4),
        delivery_mode: mode,
        status: run_status(row.get(6))?,
        current_phase_id,
        owner: decode(row.get(8))?,
    };

    let attempt_rows = sqlx::query(
        "SELECT id,phase_id,attempt,outcome,transition,output_id,output_digest,actor_session_id \
         FROM knowledge_change_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 \
         ORDER BY created_at,id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let attempts = attempt_rows
        .into_iter()
        .map(|value| {
            Ok(KnowledgePhaseAttempt {
                id: value.get(0),
                run_id,
                phase_id: phase(value.get(1))?,
                attempt: value.get(2),
                outcome: phase_outcome(value.get(3))?,
                transition: transition(value.get(4))?,
                output_id: value.try_get(5).map_err(storage_error)?,
                output_digest: value.try_get(6).map_err(storage_error)?,
                actor_session_id: value.get(7),
            })
        })
        .collect::<Result<Vec<_>>>()?;

    let output_rows = sqlx::query(
        "SELECT o.id,o.revision,o.digest,o.output,o.payload_erased,b.stale,b.stale_reason \
         FROM knowledge_change_outputs o JOIN knowledge_change_output_bindings b \
           ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND b.stale=false \
         ORDER BY o.created_at,o.id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut outputs = Vec::new();
    let mut erased_payloads = Vec::new();
    for value in output_rows {
        let id: Uuid = value.get(0);
        if value.get::<bool, _>(4) {
            erased_payloads.push(KnowledgePayloadTombstone {
                change_id,
                run_id,
                payload_id: id,
                kind: KnowledgePayloadKind::PhaseOutput,
            });
        } else {
            outputs.push(KnowledgePhaseOutput {
                id,
                run_id,
                revision: value.get(1),
                digest: value.get(2),
                output: decode(value.get(3))?,
                stale: value
                    .try_get::<Option<bool>, _>(5)
                    .map_err(storage_error)?
                    .unwrap_or(true),
                stale_reason: value.try_get(6).map_err(storage_error)?,
            });
        }
    }

    let input_rows = sqlx::query(
        "SELECT id,sequence,revisit_phase_id,reason,input,digest,payload_erased,actor_session_id,applied_basis_amendment \
         FROM knowledge_change_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 \
         ORDER BY sequence",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut inputs = Vec::new();
    for value in input_rows {
        let id: Uuid = value.get(0);
        if value.get::<bool, _>(6) {
            erased_payloads.push(KnowledgePayloadTombstone {
                change_id,
                run_id,
                payload_id: id,
                kind: KnowledgePayloadKind::Input,
            });
        } else {
            inputs.push(KnowledgeChangeInput {
                id,
                sequence: value.get(1),
                revisit_phase_id: phase(value.get(2))?,
                reason: value.get(3),
                input: value.get(4),
                digest: value.get(5),
                actor_session_id: value.get(7),
                applied_basis_amendment: value
                    .try_get::<Option<serde_json::Value>, _>(8)
                    .map_err(storage_error)?
                    .map(decode)
                    .transpose()?,
            });
        }
    }

    let payload_erased = row.get::<bool, _>(24) || row.get::<bool, _>(25);
    let stored_baseline: Option<KnowledgeBaselineManifest> = row
        .try_get::<Option<serde_json::Value>, _>(9)
        .map_err(storage_error)?
        .map(decode)
        .transpose()?;
    let (origin, candidate_baseline, candidate_source_pin_digest, candidate_impact) =
        if payload_erased {
            (None, None, None, None)
        } else {
            let source_refs: Vec<KnowledgeSourceRef> = decode(row.get(21))?;
            let source_pins: Vec<KnowledgeResolvedSourcePin> = decode(row.get(22))?;
            let operation_rows:Vec<OperationRow>=sqlx::query_as(
                "SELECT id,client_label,operation,unit_id,expected_revision,expected_lifecycle,dependency_operation_ids FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 ORDER BY id"
            ).bind(tenant).bind(workspace).bind(change_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
            let operations = operation_rows
                .into_iter()
                .map(|value| {
                    Ok(KnowledgeOperationAssignment {
                        operation_id: value.0,
                        client_label: value.1,
                        operation: decode(serde_json::Value::String(value.2))?,
                        unit_id: value.3,
                        expected_revision: value.4,
                        expected_lifecycle: value
                            .5
                            .map(|value| decode(serde_json::Value::String(value)))
                            .transpose()?,
                        dependency_operation_ids: value.6,
                    })
                })
                .collect::<Result<Vec<_>>>()?;
            let assessment = stored_baseline
                .clone()
                .unwrap_or(KnowledgeBaselineManifest {
                    workspace_generation: 0,
                    registry_generation: 0,
                    policy_generation: 0,
                    targets: Vec::new(),
                    dependencies: Vec::new(),
                    identity_matches: Vec::new(),
                    source_availability: Vec::new(),
                    assessment_conflicts: Vec::new(),
                    assessment_gaps: Vec::new(),
                    conflicts: Vec::new(),
                    missing_context: Vec::new(),
                    digest: String::new(),
                });
            let candidate_baseline = super::phase_data::current_baseline(
                tx,
                tenant,
                workspace,
                change_id,
                &assessment,
                false,
            )
            .await?;
            let candidate_source_pin_digest =
                super::phase_data::current_source_pin_digest(tx, tenant, workspace, change_id)
                    .await?;
            let candidate_impact =
                super::phase_data::current_impact(tx, tenant, workspace, change_id).await?;
            (
                Some(KnowledgeChangeOrigin {
                    intent: row.get(18),
                    desired_outcome: row.get(19),
                    owner: run.owner.clone(),
                    completion: decode(row.get(20))?,
                    sources: source_refs,
                    source_pins,
                    source_revision: row.get(26),
                    operation_hints: decode(row.get(23))?,
                    operations,
                }),
                Some(candidate_baseline),
                Some(candidate_source_pin_digest),
                Some(candidate_impact),
            )
        };
    let semantic_result = row
        .try_get::<Option<serde_json::Value>, _>(16)
        .map_err(storage_error)?;
    let erased_result = row
        .try_get::<Option<serde_json::Value>, _>(17)
        .map_err(storage_error)?;
    let effects_report = super::settle::load_effects_report(tx, tenant, workspace, run_id).await?;
    let result = match (payload_erased, semantic_result, erased_result) {
        (false, Some(value), None) => Some(decode(value)?),
        (false, None, None) => None,
        (true, None, Some(value)) => Some(decode::<KnowledgeErasedResult>(value)?.to_public()),
        (true, None, None) => None,
        _ => return Err(Error::InternalInvariant),
    };
    Ok(Some(KnowledgeChangeContext {
        change_id,
        origin,
        run,
        definition,
        delivered_phases,
        attempts,
        outputs,
        inputs,
        erased_payloads,
        baseline: stored_baseline,
        candidate_baseline,
        candidate_source_pin_digest,
        candidate_impact,
        plan,
        ready_to_commit: row
            .try_get::<Option<serde_json::Value>, _>(11)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        publisher_receipt: row
            .try_get::<Option<serde_json::Value>, _>(12)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        erased_publisher_receipt: row
            .try_get::<Option<serde_json::Value>, _>(13)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        erased_no_change_proof: row
            .try_get::<Option<serde_json::Value>, _>(27)
            .map_err(storage_error)?
            .map(decode)
            .transpose()?,
        effects_report,
        result,
    }))
}
