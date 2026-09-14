use super::*;

type OwnedCopyRow = (Uuid, String, Uuid, i64, Option<String>, Option<Uuid>);

async fn publisher_receipt_projection(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    unit: Uuid,
) -> Result<Option<serde_json::Value>> {
    let row:Option<(Option<serde_json::Value>,Option<serde_json::Value>,Option<serde_json::Value>)>=sqlx::query_as("SELECT r.publisher_receipt,r.erased_publisher_receipt,c.completion FROM knowledge_change_runs r JOIN knowledge_lifecycle_changes c ON c.tenant_id=r.tenant_id AND c.workspace_id=r.workspace_id AND c.id=r.change_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.id=$3")
        .bind(tenant).bind(workspace).bind(run).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((full, retained, completion)) = row else {
        return Ok(None);
    };
    let sequence: i64 = sqlx::query_scalar(
        "SELECT erasure_sequence FROM knowledge_suppression_ledger WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(unit)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let erased = if let Some(full) = full {
        let full: KnowledgePublisherReceipt = decode(full)?;
        KnowledgeErasedPublisherReceipt {
            id: full.id,
            request_id: full.request_id,
            change_id: full.change_id,
            run_id: full.run_id,
            completion: decode(completion.ok_or(Error::InternalInvariant)?)?,
            operations: full
                .applied_operations
                .into_iter()
                .map(|receipt| {
                    if receipt.unit_id == unit {
                        KnowledgeRetainedOperationReceipt::PayloadErased(
                            KnowledgeErasedOperationReceipt {
                                operation_id: receipt.operation_id,
                                unit_id: receipt.unit_id,
                                operation: receipt.operation,
                                event_id: receipt.event_id,
                                erasure_sequence: sequence,
                            },
                        )
                    } else {
                        KnowledgeRetainedOperationReceipt::Intact(receipt)
                    }
                })
                .collect(),
            effects: full
                .effects
                .into_iter()
                .map(|effect| KnowledgeOpaqueEffectReceipt {
                    effect_id: effect.effect_id,
                    kind: effect.kind,
                    status: effect.status,
                    generation: effect.generation,
                })
                .collect(),
        }
    } else if let Some(retained) = retained {
        let mut retained: KnowledgeErasedPublisherReceipt = decode(retained)?;
        retained.operations = retained
            .operations
            .into_iter()
            .map(|receipt| match receipt {
                KnowledgeRetainedOperationReceipt::Intact(value) if value.unit_id == unit => {
                    KnowledgeRetainedOperationReceipt::PayloadErased(
                        KnowledgeErasedOperationReceipt {
                            operation_id: value.operation_id,
                            unit_id: value.unit_id,
                            operation: value.operation,
                            event_id: value.event_id,
                            erasure_sequence: sequence,
                        },
                    )
                }
                other => other,
            })
            .collect();
        retained
    } else {
        return Ok(None);
    };
    Ok(Some(json(&erased)?))
}

pub(super) async fn canonical(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<i64> {
    let mut changed = 0;
    changed+=sqlx::query("UPDATE knowledge_unit_heads SET proposal_fingerprint='[erased]',payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR proposal_fingerprint<>'[erased]')").bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?.rows_affected() as i64;
    changed+=sqlx::query("UPDATE knowledge_revisions SET constraint_payload=NULL,document_payload=NULL,source_sha256=NULL,rdf_digest=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR constraint_payload IS NOT NULL OR document_payload IS NOT NULL OR source_sha256 IS NOT NULL OR rdf_digest IS NOT NULL)").bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?.rows_affected() as i64;
    changed+=sqlx::query("UPDATE knowledge_publication_events SET event_payload=NULL,rdf_digest=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR event_payload IS NOT NULL OR rdf_digest IS NOT NULL)").bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?.rows_affected() as i64;
    changed+=sqlx::query("UPDATE knowledge_validation_events SET sources=NULL,evidence_basis=NULL,source_pin_digest=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR sources IS NOT NULL OR evidence_basis IS NOT NULL OR source_pin_digest IS NOT NULL)").bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?.rows_affected() as i64;
    changed+=sqlx::query("UPDATE knowledge_changes SET proposal_digest=NULL,proposal_fingerprint=NULL,source_sha256=NULL,semantic_diff=NULL,baseline=NULL,proposal=NULL,binding_provenance=NULL,reason=NULL,authority_basis=NULL,review=NULL,publication_receipt=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 AND (NOT payload_erased OR proposal_digest IS NOT NULL OR proposal_fingerprint IS NOT NULL OR source_sha256 IS NOT NULL OR semantic_diff IS NOT NULL OR baseline IS NOT NULL OR proposal IS NOT NULL OR binding_provenance IS NOT NULL OR reason IS NOT NULL OR authority_basis IS NOT NULL OR review IS NOT NULL OR publication_receipt IS NOT NULL)").bind(tenant).bind(workspace).bind(unit).execute(&mut **tx).await.map_err(storage_error)?.rows_affected() as i64;
    Ok(changed)
}

pub(super) async fn registered(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<i64> {
    let copies:Vec<OwnedCopyRow>=sqlx::query_as("SELECT id,relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3 FOR UPDATE")
        .bind(tenant).bind(workspace).bind(unit).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut changed = 0;
    for (id, relation, row, revision, operation, request) in copies {
        let retained_receipt = if relation == "knowledge_change_runs" {
            publisher_receipt_projection(tx, tenant, workspace, row, unit).await?
        } else {
            None
        };
        let result=match relation.as_str(){
            "knowledge_changes"=>sqlx::query("UPDATE knowledge_changes SET proposal_digest=NULL,proposal_fingerprint=NULL,source_sha256=NULL,semantic_diff=NULL,baseline=NULL,proposal=NULL,binding_provenance=NULL,reason=NULL,authority_basis=NULL,review=NULL,publication_receipt=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "knowledge_command_receipts"=>sqlx::query("UPDATE knowledge_command_receipts SET request_payload=NULL,result_payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND operation=$3 AND request_id=$4").bind(tenant).bind(workspace).bind(operation.ok_or(Error::InternalInvariant)?).bind(request.ok_or(Error::InternalInvariant)?).execute(&mut **tx).await,
            "knowledge_lifecycle_changes"=>sqlx::query("UPDATE knowledge_lifecycle_changes SET intent=NULL,desired_outcome=NULL,sources=NULL,source_pins=NULL,operation_hints=NULL,completion=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "knowledge_change_runs"=>sqlx::query("UPDATE knowledge_change_runs SET baseline=NULL,branch_plan=NULL,ready_to_commit=NULL,publisher_receipt=NULL,effects_report=NULL,result=NULL,erased_publisher_receipt=COALESCE($4,erased_publisher_receipt),payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).bind(retained_receipt).execute(&mut **tx).await,
            "knowledge_change_operations"=>sqlx::query("UPDATE knowledge_change_operations SET client_label=NULL,reason=NULL,authority_basis=NULL,knowledge_kind=NULL,profile_ids=NULL,qualification_basis=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "knowledge_change_outputs"=>sqlx::query("UPDATE knowledge_change_outputs SET digest=NULL,output=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "knowledge_change_attempts"=>sqlx::query("UPDATE knowledge_change_attempts SET output_digest=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "knowledge_change_inputs"=>sqlx::query("UPDATE knowledge_change_inputs SET reason=NULL,input=NULL,digest=NULL,applied_basis_amendment=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "knowledge_lifecycle_command_receipts"=>sqlx::query("UPDATE knowledge_lifecycle_command_receipts SET erased_change_id=COALESCE(erased_change_id,$3),request_payload=NULL,result_payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND operation=$4 AND request_id=$5").bind(tenant).bind(workspace).bind(row).bind(operation.ok_or(Error::InternalInvariant)?).bind(request.ok_or(Error::InternalInvariant)?).execute(&mut **tx).await,
            "pipeline_knowledge_manifests"=>sqlx::query("UPDATE pipeline_knowledge_manifests SET digest=NULL,semantic_digest=NULL,selected=NULL,unresolved_needs=NULL,definition_version=NULL,definition_digest=NULL,method_requirements=NULL,selected_resources=NULL,resource_unresolved_needs=NULL,freshness_warnings=NULL,resource_semantic_digest=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_pipeline_runs"=>sqlx::query("UPDATE slice_pipeline_runs SET origin_payload=NULL,origin_result=NULL,qualification_reason=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_pipeline_phase_attempts"=>sqlx::query("UPDATE slice_pipeline_phase_attempts SET reviewer_context=NULL,request_payload=NULL,result_payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_pipeline_phase_outputs"=>sqlx::query("UPDATE slice_pipeline_phase_outputs SET body='[erased]',producer_context_id='[erased]',body_digest=NULL,reference=NULL,fields='{}'::jsonb,verdict=NULL,dispositions='[]'::jsonb,skill_reads='[]'::jsonb,resource_reads='[]'::jsonb,artifacts='[]'::jsonb,validator_receipts='[]'::jsonb,followup_proposal=NULL,knowledge_publication=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_pipeline_inputs"=>sqlx::query("UPDATE slice_pipeline_inputs SET input='[erased]',input_digest=NULL,request_payload=NULL,result_payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_pipeline_receipts"=>sqlx::query("UPDATE slice_pipeline_receipts SET request_payload=NULL,result_payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND operation=$4 AND request_id=$5").bind(tenant).bind(workspace).bind(row).bind(operation.ok_or(Error::InternalInvariant)?).bind(request.ok_or(Error::InternalInvariant)?).execute(&mut **tx).await,
            "slice_results"=>sqlx::query("UPDATE slice_results SET summary=NULL,evidence=NULL,scope_impact=NULL,remaining_work=NULL,request_payload=NULL,result_payload=NULL,knowledge_definition_version=NULL,knowledge_definition_digest=NULL,knowledge_publisher_receipt_digest=NULL,knowledge_result_origin=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_planning_inputs"=>sqlx::query("UPDATE slice_planning_inputs SET input=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(row).execute(&mut **tx).await,
            "slice_planning_snapshots"=>sqlx::query("UPDATE slice_planning_snapshots SET result_ids=ARRAY(SELECT x FROM pg_catalog.unnest(result_ids) x WHERE NOT EXISTS(SELECT 1 FROM knowledge_owned_copies c WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.unit_id=$3 AND c.relation_name='slice_results' AND c.row_id=x)),payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$4 AND sequence=$5").bind(tenant).bind(workspace).bind(unit).bind(row).bind(revision).execute(&mut **tx).await,
            "slice_candidate_drafts"=>sqlx::query("UPDATE slice_candidate_drafts SET payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4").bind(tenant).bind(workspace).bind(row).bind(revision).execute(&mut **tx).await,
            "slice_candidate_reviews"=>sqlx::query("UPDATE slice_candidate_reviews SET payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4").bind(tenant).bind(workspace).bind(row).bind(revision).execute(&mut **tx).await,
            "native_planning_receipts"=>sqlx::query("UPDATE native_planning_receipts SET request_payload=NULL,result_payload=NULL,payload_erased=true WHERE tenant_id=$1 AND workspace_id=$2 AND entity_id=$3 AND operation=$4 AND request_id=$5").bind(tenant).bind(workspace).bind(row).bind(operation.ok_or(Error::InternalInvariant)?).bind(request.ok_or(Error::InternalInvariant)?).execute(&mut **tx).await,
            _=>return Err(Error::InternalInvariant),
        }.map_err(storage_error)?;
        changed += result.rows_affected() as i64;
        sqlx::query("UPDATE knowledge_owned_copies SET redacted=true WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(id).execute(&mut **tx).await.map_err(storage_error)?;
    }
    Ok(changed)
}
