use super::*;

async fn count(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<i64> {
    sqlx::query_scalar("SELECT count(*) FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut **tx).await.map_err(storage_error)
}

pub(super) async fn reconcile_unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    recovery::reconcile_unit_direct(tx, tenant, workspace, unit).await?;
    let manifests:Vec<(Uuid,i64)>=sqlx::query_as("SELECT DISTINCT m.id,(item->>'revision')::bigint FROM pipeline_knowledge_manifests m CROSS JOIN LATERAL pg_catalog.jsonb_array_elements(COALESCE(m.selected,'[]'::jsonb)||COALESCE(m.selected_resources,'[]'::jsonb)) item WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND item->>'unit_id'=$3::text ORDER BY 1,2")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (row, revision) in manifests {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_manifest",
            "pipeline_knowledge_manifests",
            row,
            revision,
        )
        .await?;
    }
    for _ in 0..32 {
        let before = count(tx, tenant, workspace, unit).await?;
        propagate_pipeline(tx, tenant, workspace, unit).await?;
        propagate_results(tx, tenant, workspace, unit).await?;
        let after = count(tx, tenant, workspace, unit).await?;
        if after > 4096 {
            return Err(Error::CapacityExceeded);
        }
        if after == before {
            return Ok(());
        }
    }
    Err(Error::CapacityExceeded)
}

async fn propagate_pipeline(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let attempts:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT a.id FROM slice_pipeline_phase_attempts a WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND NOT a.payload_erased AND (EXISTS (SELECT 1 FROM knowledge_owned_copies c WHERE c.tenant_id=a.tenant_id AND c.workspace_id=a.workspace_id AND c.unit_id=$3 AND c.relation_name='pipeline_knowledge_manifests' AND c.row_id=NULLIF(a.request_payload->'consumed_knowledge'->>'manifest_id','')::uuid) OR EXISTS (SELECT 1 FROM pg_catalog.jsonb_array_elements(COALESCE(a.request_payload->'consumed_outputs','[]'::jsonb)) x JOIN slice_pipeline_phase_outputs source ON source.tenant_id=a.tenant_id AND source.workspace_id=a.workspace_id AND source.run_id=a.run_id AND source.phase_id=x->>'phase_id' AND source.revision=(x->>'output_revision')::bigint AND source.body_digest=x->>'digest' JOIN knowledge_owned_copies c ON c.tenant_id=source.tenant_id AND c.workspace_id=source.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_pipeline_phase_outputs' AND c.row_id=source.id AND NOT c.redacted)) ORDER BY a.id")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for attempt in attempts {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_attempt",
            "slice_pipeline_phase_attempts",
            attempt,
            0,
        )
        .await?;
        let outputs:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM slice_pipeline_phase_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND attempt_id=$3")
            .bind(tenant).bind(workspace).bind(attempt).fetch_all(&mut **tx).await.map_err(storage_error)?;
        for output in outputs {
            registry::register_propagated(
                tx,
                tenant,
                workspace,
                unit,
                "pipeline_output",
                "slice_pipeline_phase_outputs",
                output,
                0,
            )
            .await?;
        }
    }
    let promoted:Vec<(Uuid,Uuid)>=sqlx::query_as("SELECT DISTINCT p.id,p.attempt_id FROM slice_pipeline_phase_outputs p CROSS JOIN LATERAL pg_catalog.jsonb_array_elements_text(COALESCE(p.knowledge_publication->'operation_ids','[]'::jsonb)) operation_id JOIN knowledge_change_operations o ON o.tenant_id=p.tenant_id AND o.workspace_id=p.workspace_id AND o.id=operation_id::uuid WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND o.unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (output, attempt) in promoted {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "promoted_pipeline_output",
            "slice_pipeline_phase_outputs",
            output,
            0,
        )
        .await?;
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "promoted_pipeline_attempt",
            "slice_pipeline_phase_attempts",
            attempt,
            0,
        )
        .await?;
        let results:Vec<Uuid>=sqlx::query_scalar("SELECT id FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND pipeline_final_attempt_id=$3")
            .bind(tenant).bind(workspace).bind(attempt).fetch_all(&mut **tx).await.map_err(storage_error)?;
        for result in results {
            registry::register_propagated(
                tx,
                tenant,
                workspace,
                unit,
                "promoted_slice_result",
                "slice_results",
                result,
                0,
            )
            .await?;
        }
    }
    let inputs:Vec<Uuid>=sqlx::query_scalar("SELECT i.id FROM slice_pipeline_inputs i WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND ($3=ANY(i.owner_unit_ids) OR EXISTS(SELECT 1 FROM knowledge_owned_copies c WHERE c.tenant_id=i.tenant_id AND c.workspace_id=i.workspace_id AND c.unit_id=$3 AND ((c.relation_name='pipeline_knowledge_manifests' AND c.row_id=NULLIF(i.result_payload->'context'->'knowledge'->>'id','')::uuid) OR (c.relation_name='slice_pipeline_phase_outputs' AND EXISTS(SELECT 1 FROM pg_catalog.jsonb_array_elements(COALESCE(i.result_payload->'context'->'outputs','[]'::jsonb)) x WHERE NULLIF(x->>'id','')::uuid=c.row_id))))) ORDER BY i.id")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for input in inputs {
        sqlx::query("UPDATE slice_pipeline_inputs SET owner_unit_ids=array(SELECT DISTINCT x FROM pg_catalog.unnest(owner_unit_ids||$4::uuid[]) x ORDER BY x) WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(input).bind(vec![unit]).execute(&mut **tx).await.map_err(storage_error)?;
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "pipeline_input",
            "slice_pipeline_inputs",
            input,
            0,
        )
        .await?;
    }
    let receipts:Vec<(Uuid,String,Uuid)>=sqlx::query_as("SELECT r.run_id,r.operation,r.request_id FROM slice_pipeline_receipts r WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND ($3=ANY(r.owner_unit_ids) OR EXISTS(SELECT 1 FROM knowledge_owned_copies c WHERE c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_pipeline_phase_outputs' AND EXISTS(SELECT 1 FROM pg_catalog.jsonb_array_elements(COALESCE(r.result_payload->'context'->'outputs','[]'::jsonb)) x WHERE NULLIF(x->>'id','')::uuid=c.row_id))) ORDER BY r.run_id,r.operation,r.request_id")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (run, operation, receipt) in receipts {
        sqlx::query("UPDATE slice_pipeline_receipts SET owner_unit_ids=array(SELECT DISTINCT x FROM pg_catalog.unnest(owner_unit_ids||$6::uuid[]) x ORDER BY x) WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND operation=$4 AND request_id=$5")
            .bind(tenant).bind(workspace).bind(run).bind(&operation).bind(receipt).bind(vec![unit]).execute(&mut **tx).await.map_err(storage_error)?;
        recovery::register_pipeline_receipt_exact(
            tx, tenant, workspace, unit, run, &operation, receipt,
        )
        .await?;
    }
    Ok(())
}

async fn propagate_results(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<()> {
    let results:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT r.id FROM slice_results r JOIN knowledge_owned_copies c ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.unit_id=$3 AND ((c.relation_name='slice_pipeline_phase_attempts' AND c.row_id=r.pipeline_final_attempt_id) OR (c.relation_name='slice_pipeline_phase_outputs' AND EXISTS(SELECT 1 FROM slice_pipeline_phase_outputs o WHERE o.tenant_id=r.tenant_id AND o.workspace_id=r.workspace_id AND o.attempt_id=r.pipeline_final_attempt_id AND o.id=c.row_id)) OR (c.relation_name='knowledge_change_attempts' AND c.row_id=r.knowledge_final_attempt_id)) WHERE r.tenant_id=$1 AND r.workspace_id=$2 ORDER BY r.id")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for result in results {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "slice_result",
            "slice_results",
            result,
            0,
        )
        .await?;
    }
    let inputs:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT i.id FROM slice_planning_inputs i JOIN knowledge_owned_copies c ON c.tenant_id=i.tenant_id AND c.workspace_id=i.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_results' AND c.row_id=i.source_result_id WHERE i.tenant_id=$1 AND i.workspace_id=$2 ORDER BY i.id")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for input in inputs {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "planning_input",
            "slice_planning_inputs",
            input,
            0,
        )
        .await?;
    }
    let snapshots:Vec<(Uuid,i64)>=sqlx::query_as("SELECT DISTINCT s.id,s.sequence FROM slice_planning_snapshots s JOIN knowledge_owned_copies c ON c.tenant_id=s.tenant_id AND c.workspace_id=s.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_results' AND c.row_id=ANY(s.result_ids) WHERE s.tenant_id=$1 AND s.workspace_id=$2 ORDER BY s.id,s.sequence")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (snapshot, sequence) in snapshots {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "planning_snapshot",
            "slice_planning_snapshots",
            snapshot,
            sequence,
        )
        .await?;
    }
    let authored:Vec<(String,Uuid,i64,String,Uuid)>=sqlx::query_as("SELECT CASE r.operation WHEN 'save_slice_draft' THEN 'slice_candidate_drafts' ELSE 'slice_candidate_reviews' END,r.entity_id,(r.result_payload->>'revision')::bigint,r.operation,r.request_id FROM native_planning_receipts r JOIN knowledge_owned_copies c ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_planning_snapshots' AND c.row_id=(r.request_payload->>'snapshot_id')::uuid WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.operation IN ('save_slice_draft','review_slice_set') AND r.request_payload ? 'snapshot_id' AND r.result_payload ? 'revision' ORDER BY 1,2,3")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (relation, row, revision, operation, request) in authored {
        registry::register_propagated(
            tx,
            tenant,
            workspace,
            unit,
            "planning_authored",
            &relation,
            row,
            revision,
        )
        .await?;
        recovery::register_planning_receipt(tx, tenant, workspace, unit, row, &operation, request)
            .await?;
        let table = if relation == "slice_candidate_drafts" {
            "slice_candidate_drafts"
        } else {
            "slice_candidate_reviews"
        };
        let sql = format!(
            "UPDATE {table} SET owner_unit_ids=array(SELECT DISTINCT x FROM pg_catalog.unnest(owner_unit_ids||$5::uuid[]) x ORDER BY x) WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4"
        );
        sqlx::query(&sql)
            .bind(tenant)
            .bind(workspace)
            .bind(row)
            .bind(revision)
            .bind(vec![unit])
            .execute(&mut **tx)
            .await
            .map_err(storage_error)?;
        sqlx::query("UPDATE native_planning_receipts SET owner_unit_ids=array(SELECT DISTINCT x FROM pg_catalog.unnest(owner_unit_ids||$6::uuid[]) x ORDER BY x) WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 AND operation=$4 AND request_id=$5")
            .bind(tenant).bind(workspace).bind(row).bind(&operation).bind(request).bind(vec![unit]).execute(&mut **tx).await.map_err(storage_error)?;
    }
    let receipts:Vec<(Uuid,String,Uuid)>=sqlx::query_as("SELECT DISTINCT r.entity_id,r.operation,r.request_id FROM native_planning_receipts r WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND (EXISTS(SELECT 1 FROM slice_planning_inputs i JOIN knowledge_owned_copies c ON c.tenant_id=i.tenant_id AND c.workspace_id=i.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_planning_inputs' AND c.row_id=i.id WHERE i.request_id=r.request_id AND r.operation='record_slice_input') OR EXISTS(SELECT 1 FROM knowledge_owned_copies c WHERE c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.unit_id=$3 AND c.relation_name='slice_planning_snapshots' AND c.row_id=NULLIF(r.result_payload->>'current_snapshot_id','')::uuid AND r.operation='refresh_slice_set')) ORDER BY 1,2,3")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    for (entity, operation, request) in receipts {
        recovery::register_planning_receipt(
            tx, tenant, workspace, unit, entity, &operation, request,
        )
        .await?;
        sqlx::query("UPDATE native_planning_receipts SET owner_unit_ids=array(SELECT DISTINCT x FROM pg_catalog.unnest(owner_unit_ids||$6::uuid[]) x ORDER BY x) WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 AND operation=$4 AND request_id=$5")
            .bind(tenant).bind(workspace).bind(entity).bind(&operation).bind(request).bind(vec![unit]).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(())
}
