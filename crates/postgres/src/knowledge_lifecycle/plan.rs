use super::*;
use std::collections::{BTreeMap, BTreeSet};

fn applies(
    value: &KnowledgeObligationApplicability,
    operation: KnowledgeLifecycleOperation,
    knowledge_kind: KnowledgeKind,
) -> Result<bool> {
    match value {
        KnowledgeObligationApplicability::Always => Ok(true),
        KnowledgeObligationApplicability::Operation {
            operation: expected,
        } => Ok(*expected == operation),
        KnowledgeObligationApplicability::Operations { operations } => {
            Ok(operations.contains(&operation))
        }
        KnowledgeObligationApplicability::KnowledgeKind {
            knowledge_kind: expected,
        } => Ok(*expected == knowledge_kind),
        KnowledgeObligationApplicability::DeclaredCondition { .. } => {
            Err(Error::InvalidConfiguration)
        }
    }
}

fn primary_profile(kind: KnowledgeKind) -> KnowledgeProfileId {
    match kind {
        KnowledgeKind::Procedure => KnowledgeProfileId::Runbook,
        KnowledgeKind::Protocol => KnowledgeProfileId::Protocol,
        KnowledgeKind::Infrastructure => KnowledgeProfileId::Devops,
        KnowledgeKind::OperatingModel => KnowledgeProfileId::Operations,
        KnowledgeKind::ProductResearch => KnowledgeProfileId::ProductResearch,
        KnowledgeKind::Security => KnowledgeProfileId::Security,
        _ => KnowledgeProfileId::General,
    }
}

pub(crate) async fn compile_plan(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    run: Uuid,
    qualification: &KnowledgePlanQualification,
) -> Result<KnowledgeBranchPlan> {
    qualification.validate()?;
    let (definition_value, registry_value, delivery): (serde_json::Value, serde_json::Value, String) =
        sqlx::query_as(
            "SELECT definition,registry,delivery_mode FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND id=$4",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(change)
        .bind(run)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NotFound)?;
    let definition: KnowledgeChangeDefinition = decode(definition_value)?;
    let registry: KnowledgeProfileRegistry = decode(registry_value)?;
    let next_revision: i64 = sqlx::query_scalar(
        "SELECT COALESCE(MAX(o.revision),0)+1 FROM knowledge_change_outputs o \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 \
         AND o.phase_id='kc-qualify-plan'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(run)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let rows: Vec<(Uuid, String)> = sqlx::query_as(
        "SELECT id,operation FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 ORDER BY id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let operations = rows
        .iter()
        .map(|(id, operation)| Ok((*id, decode(serde_json::Value::String(operation.clone()))?)))
        .collect::<Result<BTreeMap<Uuid, KnowledgeLifecycleOperation>>>()?;
    let qualifications = qualification
        .operations
        .iter()
        .map(|value| (value.operation_id, value))
        .collect::<BTreeMap<_, _>>();
    if operations.len() != qualifications.len()
        || operations.keys().any(|id| !qualifications.contains_key(id))
    {
        return Err(Error::InvalidArguments);
    }
    let mut obligations = Vec::new();
    let mut profiles = BTreeSet::new();
    let mut policy_refs = BTreeSet::new();
    let mut shape_refs = BTreeSet::new();
    let mut method_refs = BTreeSet::new();
    for (operation_id, operation) in &operations {
        let selected = qualifications[operation_id];
        let primary_id = primary_profile(selected.knowledge_kind);
        if !selected.profiles.contains(&primary_id) {
            return Err(Error::InvalidArguments);
        }
        let primary = registry
            .profiles
            .iter()
            .find(|value| value.profile_id == primary_id)
            .ok_or(Error::InvalidConfiguration)?;
        let retained:Option<Vec<String>>=sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT r.document_payload->'profiles' FROM knowledge_change_operations o JOIN knowledge_unit_heads h ON h.tenant_id=o.tenant_id AND h.workspace_id=o.workspace_id AND h.unit_id=o.unit_id JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 AND r.contract_version='dk-2'").bind(tenant).bind(workspace).bind(operation_id).fetch_optional(&mut **tx).await.map_err(storage_error)?.flatten().map(decode).transpose()?;
        if retained.unwrap_or_default().iter().any(|value| {
            !selected
                .profiles
                .iter()
                .any(|profile| enum_text(profile).ok().as_ref() == Some(value))
        }) {
            return Err(Error::InvalidArguments);
        }
        for profile_id in &selected.profiles {
            let profile = registry
                .profiles
                .iter()
                .find(|value| value.profile_id == *profile_id)
                .ok_or(Error::InvalidArguments)?;
            let compatible = *profile_id == KnowledgeProfileId::General
                || *profile_id == primary_id
                || primary.compatible_profiles.contains(profile_id)
                || profile.compatible_profiles.contains(&primary_id);
            if !profile.operations.contains(operation)
                || !compatible
                || (*profile_id == primary_id
                    && !profile.applicable_kinds.contains(&selected.knowledge_kind))
            {
                return Err(Error::InvalidArguments);
            }
            profiles.insert(*profile_id);
            shape_refs.extend(profile.shape_refs.iter().cloned());
            policy_refs.extend(profile.evidence_rule_refs.iter().cloned());
            policy_refs.extend(profile.freshness_rule_refs.iter().cloned());
            policy_refs.extend(profile.authority_rule_refs.iter().cloned());
            policy_refs.extend(profile.impact_rule_refs.iter().cloned());
            policy_refs.extend(profile.retention_rule_refs.iter().cloned());
            policy_refs.extend(profile.index_rule_refs.iter().cloned());
            policy_refs.extend(profile.terminal_rule_refs.iter().cloned());
            for obligation in &profile.obligations {
                if applies(
                    &obligation.applicability,
                    *operation,
                    selected.knowledge_kind,
                )? {
                    method_refs.extend(obligation.method_refs.iter().cloned());
                    shape_refs.extend(obligation.shape_refs.iter().cloned());
                    obligations.push(KnowledgeBranchObligation {
                        operation_id: *operation_id,
                        profile_id: *profile_id,
                        profile_version: profile.version.clone(),
                        profile_digest: profile.digest.clone(),
                        obligation_id: obligation.id.clone(),
                        requirement: obligation.requirement.clone(),
                        phase_id: obligation.phase_id,
                        applicability: obligation.applicability.clone(),
                        method_refs: obligation.method_refs.clone(),
                        shape_refs: obligation.shape_refs.clone(),
                        dependency_obligation_ids: obligation.depends_on.clone(),
                        pending_qualification: false,
                    });
                }
            }
        }
        sqlx::query(
            "UPDATE knowledge_change_operations SET knowledge_kind=$4,profile_ids=$5,qualification_basis=$6 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(operation_id)
        .bind(enum_text(&selected.knowledge_kind)?)
        .bind(selected.profiles.iter().map(enum_text).collect::<Result<Vec<_>>>()?)
        .bind(&selected.classification_basis)
        .execute(&mut **tx)
        .await
        .map_err(storage_error)?;
    }
    obligations.sort_by(|left, right| {
        (left.operation_id, left.profile_id, &left.obligation_id).cmp(&(
            right.operation_id,
            right.profile_id,
            &right.obligation_id,
        ))
    });
    let mut plan = KnowledgeBranchPlan {
        revision: next_revision,
        digest: String::new(),
        definition_version: definition.version,
        definition_digest: definition.digest,
        registry_version: registry.version,
        registry_digest: registry.digest,
        delivery_mode: decode(serde_json::Value::String(delivery))?,
        operation_ids: operations.keys().copied().collect(),
        profiles: profiles.into_iter().collect(),
        obligations,
        policy_refs: policy_refs.into_iter().collect(),
        shape_refs: shape_refs.into_iter().collect(),
        method_refs: method_refs.into_iter().collect(),
    };
    plan.digest = digest(&plan)?;
    plan.validate()?;
    Ok(plan)
}
