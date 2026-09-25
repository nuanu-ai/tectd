use super::*;

pub(crate) async fn summaries(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    limit: u32,
) -> Result<Vec<NativePlanningSummary>> {
    let scope_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 \
         ORDER BY created_at DESC,id LIMIT $3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(i64::from(limit))
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut summaries = Vec::with_capacity(scope_ids.len());
    for scope_id in scope_ids {
        let context = load_context(tx, tenant, workspace, scope_id)
            .await?
            .ok_or(Error::InternalInvariant)?;
        let completed = context
            .slices
            .iter()
            .filter(|slice| slice.state == SliceState::Completed)
            .map(|slice| slice.candidate_id)
            .collect::<BTreeSet<_>>();
        let opened = context
            .slices
            .iter()
            .map(|slice| slice.candidate_id)
            .collect::<BTreeSet<_>>();
        let fresh = context.snapshot.planning_latest_input == context.candidate_set.latest_input;
        let eligible_work =
            if fresh && context.candidate_set.status == SliceCandidateSetStatus::Ready {
                context
                    .draft
                    .as_ref()
                    .map(|draft| {
                        draft
                            .nodes
                            .iter()
                            .filter_map(|node| match node {
                                SliceCandidateNode::Work {
                                    id,
                                    revision,
                                    dependencies,
                                    ..
                                } if !opened.contains(id)
                                    && dependencies.iter().all(|dep| completed.contains(dep)) =>
                                {
                                    Some(NativeWorkCandidateSummary {
                                        candidate_id: *id,
                                        candidate_revision: *revision,
                                    })
                                }
                                _ => None,
                            })
                            .collect()
                    })
                    .unwrap_or_default()
            } else {
                Vec::new()
            };
        let slices_needing_result = context
            .slices
            .iter()
            .filter(|slice| {
                slice.state == SliceState::Open
                    && slice.pipeline != PipelineKind::PromoteToDurableKnowledge
                    && slice.pipeline_run_id.is_none()
                    && slice.knowledge_run_id.is_none()
            })
            .map(|slice| NativeSliceSummary {
                slice_id: slice.id,
                slice_revision: slice.revision,
                state: slice.state,
            })
            .collect();
        let pipeline_runs = context
            .slices
            .iter()
            .filter_map(|slice| {
                slice
                    .pipeline_run_id
                    .map(|run_id| NativePipelineRunSummary {
                        run_id,
                        slice_id: slice.id,
                        status: slice.pipeline_status.clone(),
                    })
            })
            .collect();
        let change_rows: Vec<(Uuid, Uuid, Uuid, String, Option<String>)> = sqlx::query_as(
            "SELECT s.knowledge_change_id,s.knowledge_run_id,s.id,r.status,r.current_phase_id \
             FROM native_slices s JOIN knowledge_change_runs r \
             ON r.tenant_id=s.tenant_id AND r.workspace_id=s.workspace_id \
             AND r.change_id=s.knowledge_change_id AND r.id=s.knowledge_run_id \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.scope_id=$3 \
             ORDER BY s.created_at,s.id",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(scope_id)
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?;
        let knowledge_changes = change_rows
            .into_iter()
            .map(|row| {
                Ok(NativeKnowledgeChangeSummary {
                    change_id: row.0,
                    run_id: row.1,
                    slice_id: row.2,
                    status: decode(serde_json::Value::String(row.3))?,
                    current_phase_id: row
                        .4
                        .map(|value| decode(serde_json::Value::String(value)))
                        .transpose()?,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        summaries.push(NativePlanningSummary {
            scope_id,
            scope_revision: context.scope.revision,
            candidate_set_id: context.candidate_set.id,
            candidate_set_revision: context.candidate_set.revision,
            candidate_set_status: context.candidate_set.status,
            snapshot_id: context.snapshot.id,
            stale: !fresh,
            eligible_work,
            slices_needing_result,
            pipeline_runs,
            knowledge_changes,
        });
    }
    Ok(summaries)
}

#[allow(clippy::type_complexity)]
pub(crate) async fn load_scope(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope_id: Uuid,
) -> Result<Option<NativeScope>> {
    crate::planning_knowledge::require_owned_payload_identity(
        tx,
        tenant,
        workspace,
        &["native_scopes"],
        Some(scope_id),
    )
    .await?;
    let row:Option<(Uuid,i64,Uuid,i64,Uuid,Uuid,i64,String,String,String,serde_json::Value,serde_json::Value,Option<Uuid>,Option<i64>,Option<i64>,bool)>=sqlx::query_as(
        "SELECT n.id,n.revision,n.source_candidate_set_id,n.source_candidate_set_revision,n.source_snapshot_id,n.source_candidate_id,n.source_candidate_revision,n.boundary,n.title,n.outcome,n.includes,n.excludes,n.slice_candidate_set_id,s.input_cursor,s.latest_input,n.payload_erased FROM native_scopes n LEFT JOIN slice_candidate_sets s ON s.tenant_id=n.tenant_id AND s.workspace_id=n.workspace_id AND s.id=n.slice_candidate_set_id WHERE n.tenant_id=$1 AND n.workspace_id=$2 AND n.id=$3")
        .bind(tenant).bind(workspace).bind(scope_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(|r| {
        if r.15 {
            return Err(Error::KnowledgePayloadErased);
        }
        Ok(NativeScope {
            id: r.0,
            workspace_id: workspace,
            revision: r.1,
            source_candidate_set_id: r.2,
            source_candidate_set_revision: r.3,
            source_snapshot_id: r.4,
            source_candidate_id: r.5,
            source_candidate_revision: r.6,
            boundary: boundary(&r.7)?,
            title: r.8,
            outcome: r.9,
            includes: decode(r.10)?,
            excludes: decode(r.11)?,
            slice_candidate_set_id: r.12.ok_or(Error::InternalInvariant)?,
            slice_input_cursor: r.13.unwrap_or(0),
            slice_latest_input: r.14.unwrap_or(0),
        })
    })
    .transpose()
}

#[allow(clippy::type_complexity)]
pub(crate) async fn load_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope_id: Uuid,
) -> Result<Option<SliceCandidateContext>> {
    let Some(scope) = load_scope(tx, tenant, workspace, scope_id).await? else {
        return Ok(None);
    };
    let set_row:(Uuid,i64,String,Uuid,i64,i64)=sqlx::query_as("SELECT id,revision,status,current_snapshot_id,input_cursor,latest_input FROM slice_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3")
        .bind(tenant).bind(workspace).bind(scope_id).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let snap_row:(Uuid,i64,i64,i64,Uuid,i64,serde_json::Value,String,String,serde_json::Value,serde_json::Value,Vec<Uuid>)=sqlx::query_as(
        "SELECT id,sequence,scope_revision,source_candidate_set_revision,source_snapshot_id,planning_latest_input,method,registry_revision,registry_digest,rules,catalogue,result_ids FROM slice_planning_snapshots WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND id=$4")
        .bind(tenant).bind(workspace).bind(set_row.0).bind(set_row.3).fetch_one(&mut **tx).await.map_err(storage_error)?;
    crate::planning_knowledge::require_owned_payload_identity(
        tx,
        tenant,
        workspace,
        &["slice_candidate_drafts", "slice_candidate_reviews"],
        Some(set_row.0),
    )
    .await?;
    let draft=sqlx::query_as::<_,(Option<serde_json::Value>,bool)>("SELECT payload,payload_erased FROM slice_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 ORDER BY set_revision DESC LIMIT 1")
        .bind(tenant).bind(workspace).bind(set_row.0).fetch_optional(&mut **tx).await.map_err(storage_error)?
        .map(|(payload,erased)| if erased { Err(Error::KnowledgePayloadErased) } else { decode(payload.ok_or(Error::InternalInvariant)?) }).transpose()?;
    let review_values:Vec<(Option<serde_json::Value>,bool)>=sqlx::query_as("SELECT payload,payload_erased FROM slice_candidate_reviews WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 ORDER BY set_revision")
        .bind(tenant).bind(workspace).bind(set_row.0).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let input_rows:Vec<(Uuid,i64,Option<Uuid>,String)>=sqlx::query_as("SELECT id,sequence,source_result_id,input FROM slice_planning_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 ORDER BY sequence")
        .bind(tenant).bind(workspace).bind(set_row.0).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let inputs = input_rows
        .into_iter()
        .map(|r| SlicePlanningInput {
            id: r.0,
            sequence: r.1,
            source_result_id: r.2,
            input: r.3,
        })
        .collect();
    let slices = load_slices(tx, tenant, workspace, scope_id).await?;
    let results = load_results(tx, tenant, workspace, scope_id).await?;
    let draft_value: Option<ResolvedSliceCandidateDraft> = draft;
    let history = build_history(
        tx,
        tenant,
        workspace,
        set_row.0,
        draft_value.as_ref(),
        &slices,
    )
    .await?;
    Ok(Some(SliceCandidateContext {
        scope,
        candidate_set: SliceCandidateSet {
            id: set_row.0,
            scope_id,
            revision: set_row.1,
            status: set_status(&set_row.2)?,
            current_snapshot_id: set_row.3,
            input_cursor: set_row.4,
            latest_input: set_row.5,
        },
        snapshot: SlicePlanningSnapshot {
            id: snap_row.0,
            sequence: snap_row.1,
            scope_revision: snap_row.2,
            source_candidate_set_revision: snap_row.3,
            source_snapshot_id: snap_row.4,
            planning_latest_input: snap_row.5,
            method: decode(snap_row.6)?,
            registry_revision: snap_row.7,
            registry_digest: snap_row.8,
            rules: decode(snap_row.9)?,
            catalogue: decode(snap_row.10)?,
            result_ids: snap_row.11,
        },
        draft: draft_value,
        reviews: review_values
            .into_iter()
            .map(|(payload, erased)| {
                if erased {
                    Err(Error::KnowledgePayloadErased)
                } else {
                    decode(payload.ok_or(Error::InternalInvariant)?)
                }
            })
            .collect::<Result<Vec<_>>>()?,
        inputs,
        history,
        slices,
        results,
        checkpoints: crate::pipeline_execution::load_checkpoints_for_scope(
            tx, tenant, workspace, scope_id,
        )
        .await?,
        stale_reasons: Vec::new(),
        planning_knowledge: None,
    }))
}

#[allow(clippy::type_complexity)]
async fn load_slices(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
) -> Result<Vec<NativeSlice>> {
    let rows:Vec<(Uuid,i64,Uuid,i64,Uuid,String,String,String,String,Option<Uuid>,Option<String>,Option<Uuid>,Option<Uuid>,Option<String>,Option<Uuid>,Option<String>)>=sqlx::query_as("SELECT s.id,s.revision,s.candidate_id,s.candidate_revision,s.opening_snapshot_id,s.title,s.outcome,s.pipeline,s.state,r.id,r.status,s.knowledge_change_id,s.knowledge_run_id,kr.status,s.source_checkpoint_id,s.source_checkpoint_digest FROM native_slices s LEFT JOIN slice_pipeline_runs r ON r.tenant_id=s.tenant_id AND r.workspace_id=s.workspace_id AND r.slice_id=s.id LEFT JOIN knowledge_change_runs kr ON kr.tenant_id=s.tenant_id AND kr.workspace_id=s.workspace_id AND kr.id=s.knowledge_run_id AND kr.change_id=s.knowledge_change_id WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.scope_id=$3 ORDER BY s.created_at,s.id")
        .bind(tenant).bind(workspace).bind(scope).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut slices = Vec::with_capacity(rows.len());
    for r in rows {
        let plan = load_slice_plan_identity(tx, tenant, workspace, r.0).await?;
        slices.push(NativeSlice {
                id: r.0,
                scope_id: scope,
                revision: r.1,
                candidate_id: r.2,
                candidate_revision: r.3,
                opening_snapshot_id: r.4,
                title: r.5,
                outcome: r.6,
                pipeline: pipeline(&r.7)?,
                selected_option_id: plan.0,
                verification_plan_id: plan.1,
                verification_plan_schema: plan.2,
                verification_plan_digest: plan.3,
                verification_plan_source_definition_version: plan.4,
                verification_plan_source_definition_digest: plan.5,
                state: slice_state(&r.8)?,
                pipeline_status: r.10.unwrap_or_else(|| "not_started".into()),
                pipeline_run_id: r.9,
                knowledge_change_id: r.11,
                knowledge_run_id: r.12,
                knowledge_status: r
                    .13
                    .map(|value| decode(serde_json::Value::String(value)))
                    .transpose()?,
                source_checkpoint: r
                    .14
                    .map(|checkpoint_id| {
                        Ok(PipelineCheckpointRef {
                            checkpoint_id,
                            digest: r.15.clone().ok_or(Error::InternalInvariant)?,
                        })
                    })
                    .transpose()?,
                execution_claimed: false,
        });
    }
    Ok(slices)
}

#[allow(clippy::type_complexity)]
async fn load_slice_plan_identity(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    slice_id: Uuid,
) -> Result<(
    Option<String>, Option<String>, Option<String>, Option<String>,
    Option<String>, Option<String>,
)> {
    sqlx::query_as(
        "SELECT selected_option_id,verification_plan_id,verification_plan_schema, \
                verification_plan_digest,verification_plan_source_definition_version, \
                verification_plan_source_definition_digest \
         FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(slice_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)
}

#[allow(clippy::type_complexity)]
pub(crate) async fn load_slice(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    id: Uuid,
) -> Result<Option<NativeSlice>> {
    let row:Option<(Uuid,Uuid,i64,Uuid,i64,Uuid,String,String,String,String,Option<Uuid>,Option<String>,Option<Uuid>,Option<Uuid>,Option<String>)>=sqlx::query_as("SELECT s.id,s.scope_id,s.revision,s.candidate_id,s.candidate_revision,s.opening_snapshot_id,s.title,s.outcome,s.pipeline,s.state,r.id,r.status,s.knowledge_change_id,s.knowledge_run_id,kr.status FROM native_slices s LEFT JOIN slice_pipeline_runs r ON r.tenant_id=s.tenant_id AND r.workspace_id=s.workspace_id AND r.slice_id=s.id LEFT JOIN knowledge_change_runs kr ON kr.tenant_id=s.tenant_id AND kr.workspace_id=s.workspace_id AND kr.id=s.knowledge_run_id AND kr.change_id=s.knowledge_change_id WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3")
        .bind(tenant).bind(workspace).bind(id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let source:Option<(Uuid,String)>=sqlx::query_as("SELECT source_checkpoint_id,source_checkpoint_digest FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND source_checkpoint_id IS NOT NULL")
        .bind(tenant).bind(workspace).bind(id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let plan = if row.is_some() {
        Some(load_slice_plan_identity(tx, tenant, workspace, id).await?)
    } else {
        None
    };
    row.map(|r| {
        let plan = plan.ok_or(Error::InternalInvariant)?;
        Ok(NativeSlice {
            id: r.0,
            scope_id: r.1,
            revision: r.2,
            candidate_id: r.3,
            candidate_revision: r.4,
            opening_snapshot_id: r.5,
            title: r.6,
            outcome: r.7,
            pipeline: pipeline(&r.8)?,
            selected_option_id: plan.0,
            verification_plan_id: plan.1,
            verification_plan_schema: plan.2,
            verification_plan_digest: plan.3,
            verification_plan_source_definition_version: plan.4,
            verification_plan_source_definition_digest: plan.5,
            state: slice_state(&r.9)?,
            pipeline_status: r.11.unwrap_or_else(|| "not_started".into()),
            pipeline_run_id: r.10,
            knowledge_change_id: r.12,
            knowledge_run_id: r.13,
            knowledge_status: r
                .14
                .map(|value| decode(serde_json::Value::String(value)))
                .transpose()?,
            source_checkpoint: source.clone().map(|(checkpoint_id, digest)| {
                PipelineCheckpointRef {
                    checkpoint_id,
                    digest,
                }
            }),
            execution_claimed: false,
        })
    })
    .transpose()
}

#[allow(clippy::type_complexity)]
async fn load_results(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope: Uuid,
) -> Result<Vec<SliceResult>> {
    let rows: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object( \
         'id',id,'slice_id',slice_id,'slice_revision',slice_revision,'revision',revision, \
         'outcome',outcome,'summary',summary,'evidence',evidence,'scope_impact',scope_impact, \
         'remaining_work',remaining_work,'provenance',provenance,'pipeline_run_id',pipeline_run_id, \
         'pipeline_definition_version',pipeline_definition_version, \
         'pipeline_definition_digest',pipeline_definition_digest, \
         'pipeline_final_attempt_id',pipeline_final_attempt_id,'pipeline_result_origin',pipeline_result_origin, \
         'knowledge_provenance',CASE WHEN knowledge_run_id IS NULL THEN NULL ELSE \
            pg_catalog.jsonb_build_object('change_id',knowledge_change_id,'run_id',knowledge_run_id, \
            'definition_version',knowledge_definition_version,'definition_digest',knowledge_definition_digest, \
            'final_attempt_id',knowledge_final_attempt_id,'publisher_receipt_id',knowledge_publisher_receipt_id, \
            'publisher_receipt_digest',knowledge_publisher_receipt_digest, \
            'canonical',CASE knowledge_result_origin WHEN 'applied_erased' THEN 'applied' \
                ELSE knowledge_result_origin END) END) \
         FROM slice_results WHERE tenant_id=$1 AND workspace_id=$2 AND scope_id=$3 \
         AND NOT payload_erased ORDER BY created_at,id",
    )
        .bind(tenant).bind(workspace).bind(scope).fetch_all(&mut **tx).await.map_err(storage_error)?;
    rows.into_iter().map(decode).collect()
}

#[path = "context_history.rs"]
mod history;
use history::build_history;
