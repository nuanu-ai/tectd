use super::*;

type BaselineTargetRow = (
    Uuid,
    Uuid,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<i64>,
    Option<String>,
    Option<String>,
    Option<String>,
);

pub(in crate::knowledge_lifecycle) async fn current_baseline(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    assessment: &KnowledgeBaselineManifest,
    strict_assessment: bool,
) -> Result<KnowledgeBaselineManifest> {
    let generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let rows:Vec<BaselineTargetRow>=sqlx::query_as("SELECT o.id,o.unit_id,o.expected_revision,o.expected_lifecycle,h.lifecycle,h.accepted_revision,r.rdf_digest,r.unit_iri,r.revision_iri FROM knowledge_change_operations o LEFT JOIN knowledge_unit_heads h ON h.tenant_id=o.tenant_id AND h.workspace_id=o.workspace_id AND h.unit_id=o.unit_id LEFT JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.change_id=$3 AND o.operation<>'create' ORDER BY o.unit_id").bind(tenant).bind(workspace).bind(change).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut guards = BTreeMap::new();
    let mut machine_conflicts = Vec::new();
    for row in rows {
        match (row.4, row.5, row.6, row.7, row.8) {
            (
                Some(lifecycle),
                Some(revision),
                Some(rdf_digest),
                Some(unit_iri),
                Some(revision_iri),
            ) => {
                if row.2 != Some(revision) || row.3.as_deref() != Some(lifecycle.as_str()) {
                    machine_conflicts.push(format!("target_guard_changed:{}", row.1));
                }
                guards.insert(
                    row.0,
                    KnowledgeRevisionGuard {
                        unit_id: row.1,
                        revision,
                        lifecycle: decode(serde_json::Value::String(lifecycle))?,
                        rdf_digest,
                        unit_iri,
                        revision_iri,
                    },
                );
            }
            _ => machine_conflicts.push(format!("target_missing:{}", row.1)),
        }
    }
    for value in &assessment.identity_matches {
        let current:Option<i64>=sqlx::query_scalar("SELECT accepted_revision FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3").bind(tenant).bind(workspace).bind(value.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        if current != Some(value.revision) {
            machine_conflicts.push(format!(
                "identity_match_changed:{}:{}",
                value.client_label, value.unit_id
            ));
        }
    }
    let mut dependencies = Vec::new();
    for dependency in &assessment.dependencies {
        let current:Option<(i64,String,String,String,String)>=sqlx::query_as("SELECT h.accepted_revision,h.lifecycle,r.rdf_digest,r.unit_iri,r.revision_iri FROM knowledge_unit_heads h JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision WHERE h.tenant_id=$1 AND h.workspace_id=$2 AND h.unit_id=$3").bind(tenant).bind(workspace).bind(dependency.unit_id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
        let Some((revision, lifecycle, rdf_digest, unit_iri, revision_iri)) = current else {
            machine_conflicts.push(format!("dependency_missing:{}", dependency.unit_id));
            continue;
        };
        dependencies.push(KnowledgeRevisionGuard {
            unit_id: dependency.unit_id,
            revision,
            lifecycle: decode(serde_json::Value::String(lifecycle))?,
            rdf_digest,
            unit_iri,
            revision_iri,
        });
    }
    let (sources, source_revision):(serde_json::Value,i64)=sqlx::query_as("SELECT sources,source_revision FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3").bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?;
    let sources: Vec<KnowledgeSourceRef> = decode(sources)?;
    let source_availability: Vec<String> =
        match resolve_sources(tx, tenant, workspace, change, &sources).await {
            Ok(values) => values
                .into_iter()
                .map(|value| format!("{}:available", value.pin.source_iri))
                .collect(),
            Err(Error::InvalidSource) => sources
                .iter()
                .enumerate()
                .map(|(index, _)| {
                    format!(
                        "{}:unavailable",
                        source_iri(tenant, workspace, change, source_revision, index)
                    )
                })
                .collect(),
            Err(error) => return Err(error),
        };
    let machine_gaps = source_availability
        .iter()
        .filter(|value| value.ends_with(":unavailable"))
        .cloned()
        .collect::<Vec<_>>();
    let mut conflicts = machine_conflicts.clone();
    conflicts.extend(assessment.assessment_conflicts.iter().cloned());
    let mut missing_context = machine_gaps.clone();
    missing_context.extend(assessment.assessment_gaps.iter().cloned());
    conflicts.sort();
    conflicts.dedup();
    missing_context.sort();
    missing_context.dedup();
    if strict_assessment {
        let mut supplied_conflicts = assessment.conflicts.clone();
        supplied_conflicts.sort();
        supplied_conflicts.dedup();
        let mut supplied_gaps = assessment.missing_context.clone();
        supplied_gaps.sort();
        supplied_gaps.dedup();
        if supplied_conflicts != conflicts || supplied_gaps != missing_context {
            return Err(Error::InvalidArguments);
        }
    }
    let mut baseline = KnowledgeBaselineManifest {
        workspace_generation: generation,
        registry_generation: 0,
        policy_generation: 0,
        targets: guards.into_values().collect(),
        dependencies,
        identity_matches: assessment.identity_matches.clone(),
        source_availability,
        assessment_conflicts: assessment.assessment_conflicts.clone(),
        assessment_gaps: assessment.assessment_gaps.clone(),
        conflicts,
        missing_context,
        digest: String::new(),
    };
    baseline.digest = digest(&baseline)?;
    Ok(baseline)
}

#[allow(clippy::too_many_arguments)]
pub(in crate::knowledge_lifecycle) async fn validate_receipts(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    receipts: &KnowledgeObligationReceipts,
    plan: &KnowledgeBranchPlan,
    changeset: &KnowledgeProposedChangeset,
    advancing: bool,
) -> Result<()> {
    let expected_inputs:Vec<String>=sqlx::query_scalar("SELECT digest FROM knowledge_change_inputs WHERE tenant_id=$1 AND workspace_id=$2 AND run_id=$3 AND payload_erased=false ORDER BY sequence").bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = plan
        .obligations
        .iter()
        .map(|value| {
            (
                value.operation_id,
                value.profile_id,
                value.obligation_id.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let actual = receipts
        .receipts
        .iter()
        .map(|value| {
            (
                value.operation_id,
                value.profile_id,
                value.obligation_id.as_str(),
            )
        })
        .collect::<BTreeSet<_>>();
    let unresolved = receipts
        .receipts
        .iter()
        .filter(|value| value.disposition == KnowledgeObligationDisposition::Unresolved)
        .map(|value| value.obligation_id.as_str())
        .collect::<BTreeSet<_>>();
    let declared_unresolved = receipts
        .unresolved_obligation_ids
        .iter()
        .map(String::as_str)
        .collect::<BTreeSet<_>>();
    if expected != actual
        || receipts.receipts.len() != expected.len()
        || declared_unresolved.len() != receipts.unresolved_obligation_ids.len()
        || unresolved != declared_unresolved
        || advancing && !unresolved.is_empty()
    {
        return Err(Error::NeedsContext);
    }
    for receipt in &receipts.receipts {
        let obligation = plan
            .obligations
            .iter()
            .find(|value| {
                value.operation_id == receipt.operation_id
                    && value.profile_id == receipt.profile_id
                    && value.obligation_id == receipt.obligation_id
            })
            .ok_or(Error::InvalidArguments)?;
        let methods = receipt
            .method_reads
            .iter()
            .map(|value| (&value.instruction_id, &value.version, &value.digest))
            .collect::<BTreeSet<_>>();
        let required = obligation
            .method_refs
            .iter()
            .map(|value| (&value.id, &value.version, &value.digest))
            .collect::<BTreeSet<_>>();
        if receipt.changeset_digest != changeset.digest
            || receipt.input_digests != expected_inputs
            || methods != required
        {
            return Err(Error::StaleContext);
        }
        match receipt.disposition {
            KnowledgeObligationDisposition::Satisfied if receipt.reused_receipt_id.is_none() => {}
            KnowledgeObligationDisposition::Reused => {
                let mut prior = receipt.reused_receipt_id.ok_or(Error::InvalidArguments)?;
                let mut visited = BTreeSet::new();
                loop {
                    if !visited.insert(prior) {
                        return Err(Error::InvalidArguments);
                    }
                    let value:Option<serde_json::Value>=sqlx::query_scalar("SELECT output->'data'->'data' FROM knowledge_change_outputs WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND payload_erased=false").bind(tenant).bind(workspace).bind(prior).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                    let prior_receipts: KnowledgeObligationReceipts = value
                        .map(decode)
                        .transpose()?
                        .ok_or(Error::InvalidArguments)?;
                    let prior_receipt = prior_receipts
                        .receipts
                        .iter()
                        .find(|value| {
                            value.operation_id == receipt.operation_id
                                && value.profile_id == receipt.profile_id
                                && value.obligation_id == receipt.obligation_id
                                && value.changeset_digest == receipt.changeset_digest
                                && value.method_reads == receipt.method_reads
                                && value.input_digests == receipt.input_digests
                        })
                        .ok_or(Error::InvalidArguments)?;
                    match prior_receipt.disposition {
                        KnowledgeObligationDisposition::Satisfied
                            if prior_receipt.reused_receipt_id.is_none() =>
                        {
                            break;
                        }
                        KnowledgeObligationDisposition::Reused => {
                            prior = prior_receipt
                                .reused_receipt_id
                                .ok_or(Error::InvalidArguments)?;
                        }
                        _ => return Err(Error::InvalidArguments),
                    }
                }
            }
            KnowledgeObligationDisposition::Unresolved
                if !advancing && receipt.reused_receipt_id.is_none() => {}
            _ => return Err(Error::NeedsContext),
        }
    }
    Ok(())
}
