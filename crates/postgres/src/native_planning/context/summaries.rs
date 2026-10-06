pub(crate) async fn summaries(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    after: Option<WorkspaceCollectionCursor>,
    limit: u32,
) -> Result<NativePlanningList> {
    if !(1..=25).contains(&limit) {
        return Err(Error::InvalidArguments);
    }
    if let Some(cursor) = after {
        cursor.validate(workspace, WorkspaceCollection::NativePlanning)?;
        let anchor: Option<Uuid> = sqlx::query_scalar(
            "SELECT id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(cursor.anchor_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
        if anchor.is_none() {
            return Err(Error::InvalidArguments);
        }
    }
    let mut scope_ids: Vec<Uuid> = sqlx::query_scalar(
        "SELECT id FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 \
         AND ($3::uuid IS NULL OR created_at < (SELECT created_at FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3) \
         OR (created_at = (SELECT created_at FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3) AND id > $3)) \
         ORDER BY created_at DESC,id LIMIT $4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(after.map(|cursor| cursor.anchor_id))
    .bind(i64::from(limit) + 1)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let more = scope_ids.len() > limit as usize;
    scope_ids.truncate(limit as usize);
    let next_after = more.then(|| WorkspaceCollectionCursor {
        workspace_id: workspace,
        collection: WorkspaceCollection::NativePlanning,
        anchor_id: *scope_ids.last().expect("nonempty bounded page"),
    });
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
    Ok(NativePlanningList {
        native_planning: summaries,
        next_after,
    })
}
