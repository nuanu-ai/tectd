use super::*;

#[derive(Clone, Copy)]
pub(super) enum CopyRelation {
    LegacyChange,
    LegacyReceipt,
    LifecycleChange,
    LifecycleRun,
    LifecycleOperation,
    LifecycleOutput,
    LifecycleAttempt,
    LifecycleInput,
    LifecycleReceipt,
    Manifest,
    PipelineRun,
    PipelineAttempt,
    PipelineOutput,
    PipelineInput,
    PipelineReceipt,
    SliceResult,
    PlanningInput,
    PlanningSnapshot,
    CandidateDraft,
    CandidateReview,
    PlanningReceipt,
}

impl CopyRelation {
    fn name(self) -> &'static str {
        match self {
            Self::LegacyChange => "knowledge_changes",
            Self::LegacyReceipt => "knowledge_command_receipts",
            Self::LifecycleChange => "knowledge_lifecycle_changes",
            Self::LifecycleRun => "knowledge_change_runs",
            Self::LifecycleOperation => "knowledge_change_operations",
            Self::LifecycleOutput => "knowledge_change_outputs",
            Self::LifecycleAttempt => "knowledge_change_attempts",
            Self::LifecycleInput => "knowledge_change_inputs",
            Self::LifecycleReceipt => "knowledge_lifecycle_command_receipts",
            Self::Manifest => "pipeline_knowledge_manifests",
            Self::PipelineRun => "slice_pipeline_runs",
            Self::PipelineAttempt => "slice_pipeline_phase_attempts",
            Self::PipelineOutput => "slice_pipeline_phase_outputs",
            Self::PipelineInput => "slice_pipeline_inputs",
            Self::PipelineReceipt => "slice_pipeline_receipts",
            Self::SliceResult => "slice_results",
            Self::PlanningInput => "slice_planning_inputs",
            Self::PlanningSnapshot => "slice_planning_snapshots",
            Self::CandidateDraft => "slice_candidate_drafts",
            Self::CandidateReview => "slice_candidate_reviews",
            Self::PlanningReceipt => "native_planning_receipts",
        }
    }
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: CopyRelation,
    row: Uuid,
    revision: i64,
) -> Result<()> {
    register_exact(
        tx, tenant, workspace, unit, kind, relation, row, revision, None, None,
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register_exact(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: CopyRelation,
    row: Uuid,
    revision: i64,
    operation: Option<&str>,
    request: Option<Uuid>,
) -> Result<()> {
    sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision,row_operation,row_request_id) VALUES(pg_catalog.gen_random_uuid(),$1,$2,$3,$4,$5,$6,$7,$8,$9) ON CONFLICT DO NOTHING")
        .bind(tenant).bind(workspace).bind(unit).bind(kind).bind(relation.name()).bind(row).bind(revision)
        .bind(operation).bind(request)
        .execute(&mut **tx).await.map_err(storage_error)?;
    Ok(())
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn register_receipt(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    kind: &str,
    relation: CopyRelation,
    owner: Uuid,
    operation: &str,
    request: Uuid,
) -> Result<()> {
    register_exact(
        tx,
        tenant,
        workspace,
        unit,
        kind,
        relation,
        owner,
        0,
        Some(operation),
        Some(request),
    )
    .await
}

async fn units_for_manifest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Result<Vec<(Uuid, i64)>> {
    sqlx::query_as("SELECT DISTINCT (item->>'unit_id')::uuid,(item->>'revision')::bigint FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)||COALESCE(m.selected_resources,'[]'::jsonb)) item WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND m.id=$3 AND NOT m.payload_erased AND item ? 'unit_id' AND item ? 'revision' ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(manifest).fetch_all(&mut **tx).await.map_err(storage_error)
}

pub(crate) async fn register_pipeline_manifest_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: Uuid,
) -> Result<()> {
    for (unit, revision) in units_for_manifest(tx, tenant, workspace, manifest).await? {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_manifest",
            CopyRelation::Manifest,
            manifest,
            revision,
        )
        .await?;
    }
    Ok(())
}

pub(crate) async fn register_pipeline_run_origin_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    manifest: Uuid,
) -> Result<()> {
    for (unit, revision) in units_for_manifest(tx, tenant, workspace, manifest).await? {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_run_origin",
            CopyRelation::PipelineRun,
            run,
            revision,
        )
        .await?;
    }
    Ok(())
}

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
    units.sort_unstable();
    units.dedup();
    Ok(units)
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
    for (unit, revision) in units {
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_attempt",
            CopyRelation::PipelineAttempt,
            attempt,
            revision,
        )
        .await?;
        register(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_output",
            CopyRelation::PipelineOutput,
            output,
            revision,
        )
        .await?;
        if let Some(row) = result {
            register(
                tx,
                tenant,
                workspace,
                unit,
                "slice_result",
                CopyRelation::SliceResult,
                row,
                revision,
            )
            .await?;
        }
        if let Some(row) = planning_input {
            register(
                tx,
                tenant,
                workspace,
                unit,
                "planning_input",
                CopyRelation::PlanningInput,
                row,
                revision,
            )
            .await?;
        }
    }
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
pub(super) async fn register_propagated(
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
