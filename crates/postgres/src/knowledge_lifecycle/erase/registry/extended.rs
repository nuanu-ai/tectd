use super::*;

#[allow(clippy::too_many_arguments)]
pub(crate) async fn register_knowledge_change_output_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
    attempt: Uuid,
    output: Uuid,
    request: Uuid,
) -> Result<()> {
    let units: Vec<Uuid> = sqlx::query_scalar("SELECT unit_id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 ORDER BY unit_id")
        .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for unit in units {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleOutput,
            output,
            0,
        )
        .await?;
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleAttempt,
            attempt,
            0,
        )
        .await?;
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleRun,
            run,
            0,
        )
        .await?;
        register_receipt(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleReceipt,
            change,
            "phase_complete",
            request,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn register_knowledge_change_input_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
    input: Uuid,
    request: Uuid,
) -> Result<()> {
    let units: Vec<Uuid> = sqlx::query_scalar("SELECT unit_id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 ORDER BY unit_id")
        .bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for unit in units {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleInput,
            input,
            0,
        )
        .await?;
        register(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleRun,
            run,
            0,
        )
        .await?;
        register_receipt(
            tx,
            tenant,
            workspace,
            unit,
            "change_control",
            CopyRelation::LifecycleReceipt,
            change,
            "record_input",
            request,
        )
        .await?;
    }
    Ok(())
}

async fn inherited_pipeline_units(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    attempt: Uuid,
    manifest: Option<Uuid>,
) -> Result<Vec<(Uuid, i64)>> {
    let mut units = if let Some(manifest) = manifest {
        units_for_manifest(tx, tenant, workspace, manifest).await?
    } else {
        Vec::new()
    };
    let transitive: Vec<(Uuid, i64)> = sqlx::query_as("SELECT DISTINCT c.unit_id,COALESCE(c.source_revision,0) FROM slice_pipeline_phase_attempts a CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(a.request_payload->'consumed_outputs','[]'::jsonb)) x JOIN slice_pipeline_phase_outputs o ON o.tenant_id=a.tenant_id AND o.workspace_id=a.workspace_id AND o.run_id=a.run_id AND o.phase_id=x->>'phase_id' AND o.revision=(x->>'output_revision')::bigint AND o.body_digest=x->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.relation_name='slice_pipeline_phase_outputs' AND c.row_id=o.id AND NOT c.redacted WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.id=$3 ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(attempt).fetch_all(&mut **tx).await.map_err(storage_error)?;
    units.extend(transitive);
    let input_units: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT DISTINCT c.unit_id,COALESCE(c.source_revision,0) FROM slice_pipeline_phase_attempts a CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(a.request_payload->'consumed_inputs','[]'::jsonb)) x JOIN slice_pipeline_inputs i ON i.tenant_id=a.tenant_id AND i.workspace_id=a.workspace_id AND i.run_id=a.run_id AND i.id=(x->>'input_id')::uuid AND i.sequence=(x->>'sequence')::bigint AND i.input_digest=x->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=i.tenant_id AND c.workspace_id=i.workspace_id AND c.relation_name='slice_pipeline_inputs' AND c.row_id=i.id AND NOT c.redacted WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.id=$3 ORDER BY 1,2",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(attempt)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    units.extend(input_units);
    let checkpoint: Vec<(Uuid, i64)> = sqlx::query_as(
        "SELECT DISTINCT c.unit_id,COALESCE(c.source_revision,0) FROM slice_pipeline_runs r JOIN knowledge_owned_copies c ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.relation_name='pipeline_research_checkpoints' AND c.row_id=r.source_checkpoint_id AND NOT c.redacted WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.id=(SELECT run_id FROM slice_pipeline_phase_attempts WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3) ORDER BY 1,2",
    )
    .bind(tenant).bind(workspace).bind(attempt)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    units.extend(checkpoint);
    units.sort_unstable();
    units.dedup();
    Ok(units)
}

fn phase_copy_targets(
    attempt: Uuid,
    output: Uuid,
    result: Option<Uuid>,
    planning_input: Option<Uuid>,
) -> Vec<(&'static str, &'static str, Uuid)> {
    let mut targets = vec![
        (
            "pipeline_attempt",
            CopyRelation::PipelineAttempt.name(),
            attempt,
        ),
        (
            "pipeline_output",
            CopyRelation::PipelineOutput.name(),
            output,
        ),
    ];
    if let Some(row) = result {
        targets.push(("slice_result", CopyRelation::SliceResult.name(), row));
    }
    if let Some(row) = planning_input {
        targets.push(("planning_input", CopyRelation::PlanningInput.name(), row));
    }
    targets
}

async fn insert_phase_copy_batch(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    units: &[(Uuid, i64)],
    targets: &[(&str, &str, Uuid)],
) -> Result<()> {
    if units.is_empty() {
        return Ok(());
    }
    let unit_ids: Vec<Uuid> = units.iter().map(|&(unit, _)| unit).collect();
    let revisions: Vec<i64> = units.iter().map(|&(_, revision)| revision).collect();
    let kinds: Vec<String> = targets
        .iter()
        .map(|&(kind, _, _)| kind.to_owned())
        .collect();
    let relations: Vec<String> = targets
        .iter()
        .map(|&(_, relation, _)| relation.to_owned())
        .collect();
    let rows: Vec<Uuid> = targets.iter().map(|&(_, _, row)| row).collect();
    sqlx::query(
        "INSERT INTO knowledge_owned_copies \
         (id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision) \
         SELECT pg_catalog.gen_random_uuid(),$1,$2,u.unit_id,h.copy_kind,h.relation_name,h.row_id,u.revision \
         FROM ROWS FROM(pg_catalog.unnest($3::uuid[]),pg_catalog.unnest($4::bigint[])) AS u(unit_id,revision) \
         CROSS JOIN ROWS FROM(pg_catalog.unnest($5::text[]),pg_catalog.unnest($6::text[]),pg_catalog.unnest($7::uuid[])) AS h(copy_kind,relation_name,row_id) \
         WHERE true \
         ON CONFLICT DO NOTHING",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(&unit_ids)
    .bind(&revisions)
    .bind(&kinds)
    .bind(&relations)
    .bind(&rows)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn register_pipeline_phase_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    manifest: Option<Uuid>,
    attempt: Uuid,
    output: Uuid,
    result: Option<Uuid>,
    planning_input: Option<Uuid>,
) -> Result<()> {
    let mut units = inherited_pipeline_units(tx, tenant, workspace, attempt, manifest).await?;
    let promoted:Vec<(Uuid,i64)>=sqlx::query_as("SELECT DISTINCT o.unit_id,COALESCE(o.applied_revision,0) FROM slice_pipeline_phase_outputs p CROSS JOIN LATERAL pg_catalog.jsonb_array_elements_text(COALESCE(p.knowledge_publication->'operation_ids','[]'::jsonb)) operation_id JOIN knowledge_change_operations o ON o.tenant_id=p.tenant_id AND o.workspace_id=p.workspace_id AND o.id=operation_id::uuid WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND p.id=$3 ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(output).fetch_all(&mut **tx).await.map_err(storage_error)?;
    units.extend(promoted);
    units.sort_unstable();
    units.dedup();
    let targets = phase_copy_targets(attempt, output, result, planning_input);
    insert_phase_copy_batch(tx, tenant, workspace, &units, &targets).await?;
    let _ = run;
    Ok(())
}

pub(crate) async fn register_pipeline_input_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    _run: Uuid,
    input: Uuid,
    request: Uuid,
) -> Result<()> {
    let manifests:Vec<Uuid>=sqlx::query_scalar("SELECT m.id FROM slice_pipeline_inputs i JOIN pipeline_knowledge_manifests m ON m.tenant_id=i.tenant_id AND m.workspace_id=i.workspace_id AND m.id=NULLIF(i.result_payload->'context'->'knowledge'->>'id','')::uuid WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND i.id=$3 AND NOT m.payload_erased")
        .bind(tenant).bind(workspace).bind(input).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut owners = Vec::new();
    for manifest in manifests {
        for (unit, revision) in units_for_manifest(tx, tenant, workspace, manifest).await? {
            owners.push(unit);
            register(
                tx,
                tenant,
                workspace,
                unit,
                "pipeline_input",
                CopyRelation::PipelineInput,
                input,
                revision,
            )
            .await?;
        }
    }
    let inherited:Vec<(Uuid,i64)>=sqlx::query_as("SELECT DISTINCT c.unit_id,COALESCE(c.source_revision,0) FROM slice_pipeline_inputs i CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(i.result_payload->'context'->'outputs','[]'::jsonb)) x JOIN slice_pipeline_phase_outputs o ON o.tenant_id=i.tenant_id AND o.workspace_id=i.workspace_id AND o.run_id=i.run_id AND o.id=NULLIF(x->>'id','')::uuid AND o.revision=(x->>'revision')::bigint AND o.body_digest=x->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.relation_name='slice_pipeline_phase_outputs' AND c.row_id=o.id AND NOT c.redacted WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND i.id=$3 ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(input).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (unit, revision) in inherited {
        owners.push(unit);
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_input",
            CopyRelation::PipelineInput,
            input,
            revision,
        )
        .await?;
    }
    owners.sort_unstable();
    owners.dedup();
    sqlx::query("UPDATE slice_pipeline_inputs SET owner_unit_ids=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(input).bind(&owners).execute(&mut **tx).await.map_err(storage_error)?;
    let _ = request;
    Ok(())
}

pub(crate) async fn register_pipeline_receipt_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    request: Uuid,
) -> Result<()> {
    let units:Vec<(Uuid,i64)>=sqlx::query_as("SELECT DISTINCT c.unit_id,COALESCE(c.source_revision,0) FROM slice_pipeline_receipts r CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(r.result_payload->'context'->'outputs','[]'::jsonb)) x JOIN slice_pipeline_phase_outputs o ON o.tenant_id=r.tenant_id AND o.workspace_id=r.workspace_id AND o.run_id=r.run_id AND o.id=NULLIF(x->>'id','')::uuid AND o.revision=(x->>'revision')::bigint AND o.body_digest=x->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=o.tenant_id AND c.workspace_id=o.workspace_id AND c.relation_name='slice_pipeline_phase_outputs' AND c.row_id=o.id AND NOT c.redacted WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.run_id=$3 AND r.request_id=$4 ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(run).bind(request).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut owners = Vec::new();
    for (unit, revision) in units {
        owners.push(unit);
        register_exact(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_receipt",
            CopyRelation::PipelineReceipt,
            run,
            revision,
            Some("delivery_escalate"),
            Some(request),
        )
        .await?;
    }
    sqlx::query("UPDATE slice_pipeline_receipts SET owner_unit_ids=$5 WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND request_id=$4")
        .bind(tenant).bind(workspace).bind(run).bind(request).bind(&owners).execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(in crate::knowledge_lifecycle::erase) async fn register_propagated(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: &str,
    row: Uuid,
    revision: i64,
) -> Result<()> {
    let relation = match relation {
        "slice_pipeline_phase_attempts" => CopyRelation::PipelineAttempt,
        "slice_pipeline_phase_outputs" => CopyRelation::PipelineOutput,
        "slice_results" => CopyRelation::SliceResult,
        "slice_planning_inputs" => CopyRelation::PlanningInput,
        "slice_planning_snapshots" => CopyRelation::PlanningSnapshot,
        "slice_candidate_drafts" => CopyRelation::CandidateDraft,
        "slice_candidate_reviews" => CopyRelation::CandidateReview,
        "native_planning_receipts" => CopyRelation::PlanningReceipt,
        "slice_pipeline_inputs" => CopyRelation::PipelineInput,
        "slice_pipeline_receipts" => CopyRelation::PipelineReceipt,
        "pipeline_knowledge_manifests" => CopyRelation::Manifest,
        _ => return Err(Error::InternalInvariant),
    };
    register(tx, tenant, workspace, unit, kind, relation, row, revision).await
}

#[cfg(test)]
mod phase_copy_batch_tests {
    use super::*;

    #[tokio::test]
    async fn batch_preserves_exact_holder_keys_and_conflict_behavior() {
        if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
            return;
        }
        let pool = sqlx::PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
            .await
            .unwrap();
        let (tenant, workspace): (Uuid, Uuid) = sqlx::query_as(
            "SELECT tenant_id,workspace_id FROM agent_sessions ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let mut tx = pool.begin().await.unwrap();
        let attempt = Uuid::new_v4();
        let output = Uuid::new_v4();
        let result = Uuid::new_v4();
        let input = Uuid::new_v4();
        let unit_a = Uuid::new_v4();
        let unit_b = Uuid::new_v4();
        let units = [(unit_a, 3), (unit_b, 7), (unit_a, 3)];
        let targets = phase_copy_targets(attempt, output, Some(result), Some(input));
        assert_eq!(targets.len(), 4);
        insert_phase_copy_batch(&mut tx, tenant, workspace, &units, &targets)
            .await
            .unwrap();
        insert_phase_copy_batch(&mut tx, tenant, workspace, &units, &targets)
            .await
            .unwrap();
        let mut found: Vec<(Uuid, String, String, Uuid, i64, Option<String>, Option<Uuid>, Option<i64>)> =
            sqlx::query_as("SELECT unit_id,copy_kind,relation_name,row_id,row_revision,row_operation,row_request_id,source_revision FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND row_id=ANY($3::uuid[])")
                .bind(tenant).bind(workspace).bind(vec![attempt, output, result, input])
                .fetch_all(&mut *tx).await.unwrap();
        found.sort();
        let mut expected = Vec::new();
        for (unit, revision) in [(unit_a, 3), (unit_b, 7)] {
            for (kind, relation, row) in &targets {
                expected.push((
                    unit,
                    (*kind).to_owned(),
                    (*relation).to_owned(),
                    *row,
                    revision,
                    None,
                    None,
                    None,
                ));
            }
        }
        expected.sort();
        assert_eq!(found, expected);

        let attempt_only = Uuid::new_v4();
        let output_only = Uuid::new_v4();
        let two_targets = phase_copy_targets(attempt_only, output_only, None, None);
        assert_eq!(two_targets.len(), 2);
        insert_phase_copy_batch(&mut tx, tenant, workspace, &[], &two_targets)
            .await
            .unwrap();
        insert_phase_copy_batch(&mut tx, tenant, workspace, &[(unit_a, 3)], &two_targets)
            .await
            .unwrap();
        let count: i64 = sqlx::query_scalar("SELECT count(*) FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND row_id=ANY($3::uuid[])")
            .bind(tenant).bind(workspace).bind(vec![attempt_only, output_only])
            .fetch_one(&mut *tx).await.unwrap();
        assert_eq!(count, 2);
        tx.rollback().await.unwrap();
    }
}
