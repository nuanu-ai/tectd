use super::*;

pub(in crate::knowledge_lifecycle) async fn current_impact(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
) -> Result<KnowledgeImpactPlan> {
    let bindings:Vec<(Uuid,String)>=sqlx::query_as("SELECT b.id,b.purpose FROM knowledge_bindings b JOIN knowledge_change_operations o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.unit_id=b.unit_id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.change_id=$3 AND b.active ORDER BY b.id").bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let manifests:Vec<Uuid>=sqlx::query_scalar("SELECT DISTINCT m.id FROM pipeline_knowledge_manifests m JOIN knowledge_change_operations o ON o.tenant_id=m.tenant_id AND o.workspace_id=m.workspace_id WHERE m.tenant_id=$1 AND m.workspace_id=$2 AND o.change_id=$3 AND (m.selected @> pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object('unit_id',o.unit_id)) OR m.selected_resources @> pg_catalog.jsonb_build_array(pg_catalog.jsonb_build_object('unit_id',o.unit_id))) ORDER BY m.id").bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let copies: Vec<Uuid> = sqlx::query_scalar(
        "WITH current_control(relation_name,row_id) AS ( \
             SELECT 'knowledge_lifecycle_changes',id FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
             UNION ALL SELECT 'knowledge_change_operations',id FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 \
             UNION ALL SELECT 'knowledge_change_runs',id FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 \
             UNION ALL SELECT 'knowledge_change_outputs',o.id FROM knowledge_change_outputs o JOIN knowledge_change_runs r ON r.tenant_id=o.tenant_id AND r.workspace_id=o.workspace_id AND r.id=o.run_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.change_id=$3 \
             UNION ALL SELECT 'knowledge_change_attempts',a.id FROM knowledge_change_attempts a JOIN knowledge_change_runs r ON r.tenant_id=a.tenant_id AND r.workspace_id=a.workspace_id AND r.id=a.run_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.change_id=$3 \
             UNION ALL SELECT 'knowledge_change_inputs',i.id FROM knowledge_change_inputs i JOIN knowledge_change_runs r ON r.tenant_id=i.tenant_id AND r.workspace_id=i.workspace_id AND r.id=i.run_id WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.change_id=$3 \
             UNION ALL SELECT 'knowledge_lifecycle_command_receipts',$3 \
         ) \
         SELECT DISTINCT c.id FROM knowledge_owned_copies c \
         JOIN knowledge_change_operations o ON o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.unit_id=c.unit_id \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.change_id=$3 AND NOT c.redacted \
           AND NOT (c.copy_kind='change_control' AND EXISTS (SELECT 1 FROM current_control x WHERE x.relation_name=c.relation_name AND x.row_id=c.row_id)) \
         ORDER BY c.id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut affected_contexts = bindings
        .into_iter()
        .map(|row| KnowledgeImpactTarget {
            reference: format!("knowledge-binding:{}", row.0),
            owner_ref: "backend".into(),
            effect: "binding_selection_change".into(),
            blocking: row.1 == "required",
        })
        .chain(manifests.into_iter().map(|id| KnowledgeImpactTarget {
            reference: format!("pipeline-manifest:{id}"),
            owner_ref: "backend".into(),
            effect: "context_invalidation".into(),
            blocking: true,
        }))
        .collect::<Vec<_>>();
    affected_contexts.push(KnowledgeImpactTarget {
        reference: format!("workspace-knowledge-state:{workspace}"),
        owner_ref: "backend".into(),
        effect: "invalidate_manifest_generation".into(),
        blocking: true,
    });
    let mut owned_copies = copies
        .into_iter()
        .map(|id| KnowledgeImpactTarget {
            reference: format!("owned-copy:{id}"),
            owner_ref: "backend".into(),
            effect: "owned_copy_review".into(),
            blocking: true,
        })
        .collect::<Vec<_>>();
    let erases: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND operation='erase')")
        .bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if erases {
        owned_copies.push(KnowledgeImpactTarget {
            reference: format!("knowledge-change-control:{change}"),
            owner_ref: "backend".into(),
            effect: "redact_change_control_payloads".into(),
            blocking: true,
        });
    }
    let mut value = KnowledgeImpactPlan {
        synchronous_changes: Vec::new(),
        affected_contexts,
        derivations: Vec::new(),
        owned_copies,
        followups: Vec::new(),
        blocking_conflicts: Vec::new(),
        digest: String::new(),
    };
    value.digest = digest(&value)?;
    Ok(value)
}

pub(in crate::knowledge_lifecycle) async fn reconcile_impact(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    impact: &mut KnowledgeImpactPlan,
) -> Result<()> {
    let candidate = current_impact(tx, tenant, workspace, change).await?;
    if !machine_impact_matches(&candidate, impact) {
        return Err(Error::NeedsContext);
    }
    impact.digest = String::new();
    impact.digest = digest(impact)?;
    Ok(())
}

fn machine_target(value: &KnowledgeImpactTarget) -> bool {
    [
        "knowledge-binding:",
        "pipeline-manifest:",
        "owned-copy:",
        "knowledge-change-control:",
        "workspace-knowledge-state:",
    ]
    .iter()
    .any(|prefix| value.reference.starts_with(prefix))
}

fn target_key(value: &KnowledgeImpactTarget) -> (String, String, String, bool) {
    (
        value.reference.clone(),
        value.owner_ref.clone(),
        value.effect.clone(),
        value.blocking,
    )
}

pub(in crate::knowledge_lifecycle) fn machine_impact_matches(
    candidate: &KnowledgeImpactPlan,
    reviewed: &KnowledgeImpactPlan,
) -> bool {
    let keys = |values: &[KnowledgeImpactTarget]| {
        values
            .iter()
            .filter(|value| machine_target(value))
            .map(target_key)
            .collect::<BTreeSet<_>>()
    };
    keys(&candidate.affected_contexts) == keys(&reviewed.affected_contexts)
        && keys(&candidate.owned_copies) == keys(&reviewed.owned_copies)
}
