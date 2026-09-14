use super::*;

type QualifiedOperationRow = (
    Uuid,
    Uuid,
    String,
    String,
    Option<i64>,
    Option<String>,
    String,
    String,
    Vec<Uuid>,
    String,
    Vec<String>,
);

#[allow(clippy::too_many_arguments)]
pub(in crate::knowledge_lifecycle) async fn process_agent_data(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    change: Uuid,
    run: Uuid,
    output: &mut KnowledgeAgentPhaseOutputDraft,
    advancing: bool,
) -> Result<PhaseUpdates> {
    let mut updates = PhaseUpdates::empty();
    match &mut output.data {
        KnowledgeAgentPhaseData::KcIntake(intent) => {
            let row:(String,serde_json::Value,serde_json::Value)=sqlx::query_as("SELECT desired_outcome,operation_hints,completion FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?;
            if intent.bounded_outcome != row.0
                || json(&intent.operation_hints)? != row.1
                || json(&intent.completion)? != row.2
            {
                return Err(Error::InputConflict);
            }
        }
        KnowledgeAgentPhaseData::KcResolveBaseline(value) => {
            let current = current_baseline(tx, tenant, workspace, change, value, true).await?;
            if advancing && (!current.conflicts.is_empty() || !current.missing_context.is_empty()) {
                return Err(Error::NeedsContext);
            }
            *value = current.clone();
            updates.baseline = Some(current)
        }
        KnowledgeAgentPhaseData::KcQualifyPlan(value) => {
            updates.plan = Some(compile_plan(tx, tenant, workspace, change, run, value).await?)
        }
        KnowledgeAgentPhaseData::KcQualifyEvidence(value) => {
            let sources:Vec<KnowledgeSourceRef>=decode(sqlx::query_scalar("SELECT sources FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?)?;
            let pins = resolve_sources(tx, tenant, workspace, change, &sources)
                .await?
                .into_iter()
                .map(|v| v.pin)
                .collect::<Vec<_>>();
            let pin_digest = digest(&pins)?;
            if value.source_pins != pins {
                return Err(Error::InvalidSource);
            }
            if value.source_pin_digest != pin_digest {
                return Err(Error::StaleContext);
            }
            if advancing && !value.unresolved_gaps.is_empty() {
                return Err(Error::NeedsContext);
            }
        }
        KnowledgeAgentPhaseData::KcPrepareChange(value) => {
            for operation in &mut value.operations {
                if let Some(dependency) = operation
                    .successor
                    .as_ref()
                    .and_then(|successor| successor.operation_id)
                    && !operation.dependency_operation_ids.contains(&dependency)
                {
                    operation.dependency_operation_ids.push(dependency);
                    operation.dependency_operation_ids.sort();
                }
            }
            let origin_sources:Vec<KnowledgeSourceRef>=decode(sqlx::query_scalar("SELECT sources FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?)?;
            let evidence: KnowledgeEvidenceManifest = load_phase_data(
                tx,
                tenant,
                workspace,
                run,
                KnowledgeChangePhaseId::KcQualifyEvidence,
            )
            .await?;
            for operation in &value.operations {
                let sources = operation
                    .document
                    .as_ref()
                    .map(|document| document.sources.as_slice())
                    .or_else(|| {
                        operation
                            .revalidation
                            .as_ref()
                            .map(|revalidation| revalidation.sources.as_slice())
                    })
                    .unwrap_or_default();
                let mut source_indexes = BTreeSet::new();
                for source in sources {
                    let matches = origin_sources
                        .iter()
                        .enumerate()
                        .filter_map(|(index, candidate)| {
                            (candidate == source).then_some(index as u32)
                        })
                        .collect::<Vec<_>>();
                    if matches.len() != 1 {
                        return Err(Error::InvalidSource);
                    }
                    source_indexes.insert(matches[0]);
                }
                let reviewed_indexes = evidence
                    .claims
                    .iter()
                    .filter(|claim| claim.operation_id == operation.operation_id)
                    .flat_map(|claim| claim.source_indexes.iter().copied())
                    .collect::<BTreeSet<_>>();
                if source_indexes != reviewed_indexes {
                    return Err(Error::InvalidSource);
                }
            }
            value.validate()?;
            let plan:KnowledgeBranchPlan=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT branch_plan FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
            if value.evidence_digest != evidence.source_pin_digest
                || value.operations.len() != plan.operation_ids.len()
            {
                return Err(Error::InputConflict);
            }
            let qualified:Vec<QualifiedOperationRow>=sqlx::query_as("SELECT id,unit_id,client_label,operation,expected_revision,expected_lifecycle,reason,authority_basis,dependency_operation_ids,knowledge_kind,profile_ids FROM knowledge_change_operations WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 ORDER BY id").bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
            let proposed = value
                .operations
                .iter()
                .map(|v| (v.operation_id, v))
                .collect::<BTreeMap<_, _>>();
            let operation_kinds = qualified
                .iter()
                .map(|row| (row.0, row.3.as_str()))
                .collect::<BTreeMap<_, _>>();
            if value.operations.iter().any(|operation| {
                operation
                    .successor
                    .as_ref()
                    .and_then(|successor| successor.operation_id)
                    .is_some_and(|id| operation_kinds.get(&id) != Some(&"create"))
            }) {
                return Err(Error::InvalidArguments);
            }
            for row in qualified {
                let v = proposed.get(&row.0).ok_or(Error::InputConflict)?;
                let mut expected_dependencies = row.8.clone();
                if let Some(dependency) = v
                    .successor
                    .as_ref()
                    .and_then(|successor| successor.operation_id)
                    && !expected_dependencies.contains(&dependency)
                {
                    expected_dependencies.push(dependency);
                    expected_dependencies.sort();
                }
                if v.unit_id != row.1
                    || v.client_label != row.2
                    || enum_text(&v.operation)? != row.3
                    || v.expected_revision != row.4
                    || v.expected_lifecycle
                        .as_ref()
                        .map(enum_text)
                        .transpose()?
                        .as_ref()
                        != row.5.as_ref()
                    || v.reason != row.6
                    || v.authority_basis != row.7
                    || v.dependency_operation_ids != expected_dependencies
                    || v.document.as_ref().is_some_and(|d| {
                        enum_text(&d.knowledge_kind).ok().as_ref() != Some(&row.9)
                            || d.profiles
                                .iter()
                                .map(enum_text)
                                .collect::<Result<Vec<_>>>()
                                .ok()
                                .as_ref()
                                != Some(&row.10)
                    })
                {
                    return Err(Error::InputConflict);
                }
                if v.dependency_operation_ids != row.8 {
                    sqlx::query("UPDATE knowledge_change_operations SET dependency_operation_ids=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
                        .bind(tenant).bind(workspace).bind(v.operation_id).bind(&v.dependency_operation_ids)
                        .execute(&mut **tx).await.map_err(storage_error)?;
                }
            }
            value.digest = String::new();
            value.digest = digest(value)?
        }
        KnowledgeAgentPhaseData::KcDomainChecks(value) => {
            let plan:KnowledgeBranchPlan=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT branch_plan FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
            let changeset = load_phase_data(
                tx,
                tenant,
                workspace,
                run,
                KnowledgeChangePhaseId::KcPrepareChange,
            )
            .await?;
            validate_receipts(
                tx, tenant, workspace, run, value, &plan, &changeset, advancing,
            )
            .await?
        }
        KnowledgeAgentPhaseData::KcImpactPlan(value) => {
            super::erase::reconcile_change_owned_copies(tx, tenant, workspace, change).await?;
            reconcile_impact(tx, tenant, workspace, change, value).await?;
            if advancing && !value.blocking_conflicts.is_empty() {
                return Err(Error::NeedsContext);
            }
        }
        KnowledgeAgentPhaseData::KcReviewReconcile(value) => {
            require_prior_findings(tx, tenant, workspace, run, value).await?;
            if matches!(
                value.outcome,
                KnowledgeReviewOutcome::Ready | KnowledgeReviewOutcome::NoChange
            ) {
                let linked =
                    crate::knowledge_maintenance::linked_tasks(tx, tenant, workspace, change, run)
                        .await?;
                if linked.iter().any(|task| {
                    !value
                        .reviewed_digests
                        .iter()
                        .any(|digest| digest == &task.signal.basis_digest)
                }) {
                    return Err(Error::NeedsContext);
                }
            }
            if value.outcome == KnowledgeReviewOutcome::NoChange {
                validate_no_change(tx, tenant, workspace, principal, run).await?;
            }
            if value.outcome == KnowledgeReviewOutcome::Ready {
                let plan:KnowledgeBranchPlan=decode(sqlx::query_scalar::<_,Option<serde_json::Value>>("SELECT branch_plan FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)?)?;
                let changeset: KnowledgeProposedChangeset = load_phase_data(
                    tx,
                    tenant,
                    workspace,
                    run,
                    KnowledgeChangePhaseId::KcPrepareChange,
                )
                .await?;
                let evidence: KnowledgeEvidenceManifest = load_phase_data(
                    tx,
                    tenant,
                    workspace,
                    run,
                    KnowledgeChangePhaseId::KcQualifyEvidence,
                )
                .await?;
                let impact: KnowledgeImpactPlan = load_phase_data(
                    tx,
                    tenant,
                    workspace,
                    run,
                    KnowledgeChangePhaseId::KcImpactPlan,
                )
                .await?;
                let checks = phase_digest(
                    tx,
                    tenant,
                    workspace,
                    run,
                    KnowledgeChangePhaseId::KcDomainChecks,
                )
                .await?;
                let operations = value
                    .covered_operation_ids
                    .iter()
                    .copied()
                    .collect::<BTreeSet<_>>();
                let obligations = value
                    .covered_obligation_ids
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>();
                let expected_obligations = plan
                    .obligations
                    .iter()
                    .map(|v| v.obligation_id.as_str())
                    .collect::<BTreeSet<_>>();
                let reviewed = value
                    .reviewed_digests
                    .iter()
                    .map(String::as_str)
                    .collect::<BTreeSet<_>>();
                if operations != plan.operation_ids.iter().copied().collect()
                    || obligations != expected_obligations
                    || [
                        plan.digest.as_str(),
                        changeset.digest.as_str(),
                        evidence.source_pin_digest.as_str(),
                        impact.digest.as_str(),
                        checks.as_str(),
                    ]
                    .into_iter()
                    .any(|v| !reviewed.contains(v))
                {
                    return Err(Error::NeedsContext);
                }
            }
        }
        KnowledgeAgentPhaseData::KcResultHandoff(value) => {
            if value.canonical == KnowledgeCanonicalOutcome::NoChange {
                let row: (serde_json::Value, Option<serde_json::Value>) = sqlx::query_as(
                    "SELECT c.owner,r.erased_no_change_proof FROM knowledge_change_runs r \
                     JOIN knowledge_lifecycle_changes c ON c.tenant_id=r.tenant_id \
                     AND c.workspace_id=r.workspace_id AND c.id=r.change_id \
                     WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.id=$3",
                )
                .bind(tenant)
                .bind(workspace)
                .bind(run)
                .fetch_one(&mut **tx)
                .await
                .map_err(storage_error)?;
                if let Some(proof) = row.1 {
                    super::validate_erased_no_change(
                        tx,
                        tenant,
                        workspace,
                        principal,
                        &decode(row.0)?,
                        change,
                        run,
                        &decode(proof)?,
                    )
                    .await?;
                } else {
                    validate_no_change(tx, tenant, workspace, principal, run).await?;
                }
            }
            let row:(Option<String>,Option<serde_json::Value>,Option<serde_json::Value>)=sqlx::query_as("SELECT terminal_review_outcome,publisher_receipt,erased_publisher_receipt FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(run).fetch_one(&mut **tx).await.map_err(storage_error)?;
            match row.0.as_deref() {
                Some("ready") => {
                    let receipt_id = match (row.1, row.2) {
                        (Some(full), None) => decode::<KnowledgePublisherReceipt>(full)?.id,
                        (None, Some(erased)) => {
                            decode::<KnowledgeErasedPublisherReceipt>(erased)?.id
                        }
                        _ => return Err(Error::InternalInvariant),
                    };
                    let effects = super::settle::load_effects_report(tx, tenant, workspace, run)
                        .await?
                        .ok_or(Error::NeedsContext)?;
                    if value.canonical != KnowledgeCanonicalOutcome::Applied
                        || value.publisher_receipt_id != Some(receipt_id)
                        || !effects.required_complete
                        || value.effects != effects.effects
                        || value.user_outcome == KnowledgeUserOutcome::Achieved
                            && !effects.remaining_work.is_empty()
                    {
                        return Err(Error::InputConflict);
                    }
                }
                Some("no_change") => {
                    if value.canonical != KnowledgeCanonicalOutcome::NoChange
                        || value.user_outcome != KnowledgeUserOutcome::Achieved
                        || value.publisher_receipt_id.is_some()
                        || !value.effects.is_empty()
                    {
                        return Err(Error::InputConflict);
                    }
                }
                Some("rejected") => {
                    if value.canonical != KnowledgeCanonicalOutcome::Rejected
                        || value.user_outcome != KnowledgeUserOutcome::NotAchieved
                        || value.publisher_receipt_id.is_some()
                        || !value.effects.is_empty()
                        || value.remaining_work.is_empty()
                    {
                        return Err(Error::InputConflict);
                    }
                }
                _ => return Err(Error::NeedsContext),
            }
            updates.result = Some(value.clone())
        }
    }
    Ok(updates)
}
