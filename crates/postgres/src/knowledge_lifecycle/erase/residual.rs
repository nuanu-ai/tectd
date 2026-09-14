use super::*;

type OwnedCopyStateRow = (bool, String, Uuid, i64, Option<String>, Option<Uuid>);

pub(super) async fn relational(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<i64> {
    let direct:i64=sqlx::query_scalar("SELECT (SELECT count(*) FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR proposal_fingerprint<>'[erased]'))+(SELECT count(*) FROM knowledge_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR constraint_payload IS NOT NULL OR document_payload IS NOT NULL OR source_sha256 IS NOT NULL OR rdf_digest IS NOT NULL))+(SELECT count(*) FROM knowledge_publication_events WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR event_payload IS NOT NULL OR rdf_digest IS NOT NULL))+(SELECT count(*) FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR sources IS NOT NULL OR source_pin_digest IS NOT NULL OR evidence_basis IS NOT NULL))+(SELECT count(*) FROM knowledge_changes WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR proposal_digest IS NOT NULL OR proposal_fingerprint IS NOT NULL OR source_sha256 IS NOT NULL OR semantic_diff IS NOT NULL OR baseline IS NOT NULL OR proposal IS NOT NULL OR binding_provenance IS NOT NULL OR reason IS NOT NULL OR authority_basis IS NOT NULL OR review IS NOT NULL OR publication_receipt IS NOT NULL))")
        .bind(tenant).bind(workspace).bind(unit).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let copies:Vec<OwnedCopyStateRow>=sqlx::query_as("SELECT redacted,relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut readable = direct;
    for (marked, relation, row, revision, operation, request) in copies {
        let clean=match relation.as_str(){
            "knowledge_changes"=>id_clean(tx,tenant,workspace,"knowledge_changes",row,"payload_erased AND proposal_digest IS NULL AND proposal_fingerprint IS NULL AND source_sha256 IS NULL AND semantic_diff IS NULL AND baseline IS NULL AND proposal IS NULL AND binding_provenance IS NULL AND reason IS NULL AND authority_basis IS NULL AND review IS NULL AND publication_receipt IS NULL").await?,
            "knowledge_command_receipts"=>receipt_clean(tx,tenant,workspace,"knowledge_command_receipts",None,operation.as_deref().ok_or(Error::InternalInvariant)?,request.ok_or(Error::InternalInvariant)?).await?,
            "knowledge_lifecycle_changes"=>id_clean(tx,tenant,workspace,"knowledge_lifecycle_changes",row,"payload_erased AND intent IS NULL AND desired_outcome IS NULL AND sources IS NULL AND source_pins IS NULL AND operation_hints IS NULL AND completion IS NULL").await?,
            "knowledge_change_runs"=>id_clean(tx,tenant,workspace,"knowledge_change_runs",row,"payload_erased AND baseline IS NULL AND branch_plan IS NULL AND ready_to_commit IS NULL AND publisher_receipt IS NULL AND effects_report IS NULL AND result IS NULL AND (erased_effects_report IS NULL OR tect_dk_opaque_effects_valid(erased_effects_report->'effects')) AND (erased_result IS NULL OR tect_dk_opaque_effects_valid(erased_result->'effects'))").await?,
            "knowledge_change_operations"=>id_clean(tx,tenant,workspace,"knowledge_change_operations",row,"payload_erased AND client_label IS NULL AND reason IS NULL AND authority_basis IS NULL AND knowledge_kind IS NULL AND profile_ids IS NULL AND qualification_basis IS NULL").await?,
            "knowledge_change_outputs"=>id_clean(tx,tenant,workspace,"knowledge_change_outputs",row,"payload_erased AND output IS NULL AND digest IS NULL").await?,
            "knowledge_change_attempts"=>id_clean(tx,tenant,workspace,"knowledge_change_attempts",row,"payload_erased AND output_digest IS NULL").await?,
            "knowledge_change_inputs"=>id_clean(tx,tenant,workspace,"knowledge_change_inputs",row,"payload_erased AND reason IS NULL AND input IS NULL AND digest IS NULL AND applied_basis_amendment IS NULL").await?,
            "knowledge_lifecycle_command_receipts"=>receipt_clean(tx,tenant,workspace,"knowledge_lifecycle_command_receipts",None,operation.as_deref().ok_or(Error::InternalInvariant)?,request.ok_or(Error::InternalInvariant)?).await?,
            "pipeline_knowledge_manifests"=>id_clean(tx,tenant,workspace,"pipeline_knowledge_manifests",row,"payload_erased AND digest IS NULL AND semantic_digest IS NULL AND selected IS NULL AND unresolved_needs IS NULL AND definition_version IS NULL AND definition_digest IS NULL AND method_requirements IS NULL AND selected_resources IS NULL AND resource_unresolved_needs IS NULL AND freshness_warnings IS NULL AND resource_semantic_digest IS NULL").await?,
            "slice_pipeline_runs"=>id_clean(tx,tenant,workspace,"slice_pipeline_runs",row,"payload_erased AND origin_payload IS NULL AND origin_result IS NULL AND qualification_reason IS NULL").await?,
            "slice_pipeline_phase_attempts"=>id_clean(tx,tenant,workspace,"slice_pipeline_phase_attempts",row,"payload_erased AND reviewer_context IS NULL AND request_payload IS NULL AND result_payload IS NULL").await?,
            "slice_pipeline_phase_outputs"=>id_clean(tx,tenant,workspace,"slice_pipeline_phase_outputs",row,"payload_erased AND body='[erased]' AND producer_context_id='[erased]' AND body_digest IS NULL AND reference IS NULL AND fields='{}'::jsonb AND verdict IS NULL AND dispositions='[]'::jsonb AND skill_reads='[]'::jsonb AND resource_reads='[]'::jsonb AND artifacts='[]'::jsonb AND validator_receipts='[]'::jsonb AND followup_proposal IS NULL AND knowledge_publication IS NULL").await?,
            "slice_pipeline_inputs"=>id_clean(tx,tenant,workspace,"slice_pipeline_inputs",row,"payload_erased AND input='[erased]' AND input_digest IS NULL AND request_payload IS NULL AND result_payload IS NULL").await?,
            "slice_pipeline_receipts"=>receipt_clean(tx,tenant,workspace,"slice_pipeline_receipts",Some(row),operation.as_deref().ok_or(Error::InternalInvariant)?,request.ok_or(Error::InternalInvariant)?).await?,
            "slice_results"=>id_clean(tx,tenant,workspace,"slice_results",row,"payload_erased AND summary IS NULL AND evidence IS NULL AND scope_impact IS NULL AND remaining_work IS NULL AND request_payload IS NULL AND result_payload IS NULL AND knowledge_definition_version IS NULL AND knowledge_definition_digest IS NULL AND knowledge_publisher_receipt_digest IS NULL AND knowledge_result_origin IS NULL").await?,
            "slice_planning_inputs"=>id_clean(tx,tenant,workspace,"slice_planning_inputs",row,"payload_erased AND input IS NULL").await?,
            "slice_planning_snapshots"=>snapshot_clean(tx,tenant,workspace,unit,row,revision).await?,
            "slice_candidate_drafts"=>versioned_clean(tx,tenant,workspace,"slice_candidate_drafts",row,revision).await?,
            "slice_candidate_reviews"=>versioned_clean(tx,tenant,workspace,"slice_candidate_reviews",row,revision).await?,
            "native_planning_receipts"=>planning_receipt_clean(tx,tenant,workspace,row,operation.as_deref().ok_or(Error::InternalInvariant)?,request.ok_or(Error::InternalInvariant)?).await?,
            _=>return Err(Error::InternalInvariant),
        };
        if !marked || !clean {
            readable += 1
        }
    }
    Ok(readable)
}

async fn id_clean(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    table: &str,
    row: Uuid,
    predicate: &str,
) -> Result<bool> {
    key_clean(tx, tenant, workspace, table, "id", row, predicate).await
}
async fn key_clean(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    table: &str,
    key: &str,
    row: Uuid,
    predicate: &str,
) -> Result<bool> {
    let sql = format!(
        "SELECT COALESCE((SELECT {predicate} FROM {table} WHERE tenant_id=$1 AND workspace_id=$2 AND {key}=$3),true)"
    );
    sqlx::query_scalar(&sql)
        .bind(tenant)
        .bind(workspace)
        .bind(row)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)
}
async fn snapshot_clean(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    row: Uuid,
    revision: i64,
) -> Result<bool> {
    sqlx::query_scalar("SELECT COALESCE((SELECT payload_erased AND NOT EXISTS(SELECT 1 FROM pg_catalog.unnest(result_ids) result JOIN knowledge_owned_copies c ON c.tenant_id=$1 AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name='slice_results' AND c.row_id=result) FROM slice_planning_snapshots WHERE tenant_id=$1 AND workspace_id=$2 AND id=$4 AND sequence=$5),true)")
        .bind(tenant).bind(workspace).bind(unit).bind(row).bind(revision).fetch_one(&mut **tx).await.map_err(storage_error)
}
async fn versioned_clean(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    table: &str,
    row: Uuid,
    revision: i64,
) -> Result<bool> {
    let sql = format!(
        "SELECT COALESCE((SELECT payload_erased AND payload IS NULL FROM {table} WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4),true)"
    );
    sqlx::query_scalar(&sql)
        .bind(tenant)
        .bind(workspace)
        .bind(row)
        .bind(revision)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)
}
async fn planning_receipt_clean(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    entity: Uuid,
    operation: &str,
    request: Uuid,
) -> Result<bool> {
    sqlx::query_scalar("SELECT COALESCE((SELECT payload_erased AND request_payload IS NULL AND result_payload IS NULL FROM native_planning_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 AND operation=$4 AND request_id=$5),true)")
        .bind(tenant).bind(workspace).bind(entity).bind(operation).bind(request).fetch_one(&mut **tx).await.map_err(storage_error)
}

async fn receipt_clean(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    table: &str,
    owner: Option<Uuid>,
    operation: &str,
    request: Uuid,
) -> Result<bool> {
    let owner_clause = if owner.is_some() {
        " AND run_id=$5"
    } else {
        ""
    };
    let sql = format!(
        "SELECT COALESCE((SELECT payload_erased AND request_payload IS NULL AND result_payload IS NULL FROM {table} WHERE tenant_id=$1 AND workspace_id=$2 AND operation=$3 AND request_id=$4{owner_clause}),true)"
    );
    let mut query = sqlx::query_scalar(&sql)
        .bind(tenant)
        .bind(workspace)
        .bind(operation)
        .bind(request);
    if let Some(owner) = owner {
        query = query.bind(owner);
    }
    query.fetch_one(&mut **tx).await.map_err(storage_error)
}
