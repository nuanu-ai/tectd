use super::*;
use std::collections::BTreeSet;

pub(super) async fn validate(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    session: Uuid,
    producer_run: Uuid,
    phase: &PipelinePhaseDefinition,
    output: &PipelinePhaseOutputDraft,
) -> Result<()> {
    let required = phase
        .output_constraints
        .iter()
        .any(|constraint| match constraint {
            PipelineOutputConstraint::ResolvedKnowledgePublication { when_verdicts } => output
                .verdict
                .as_ref()
                .is_some_and(|verdict| when_verdicts.contains(verdict)),
            _ => false,
        });
    let Some(reference) = output.knowledge_publication.as_ref() else {
        return if required {
            Err(Error::InvalidSource)
        } else {
            Ok(())
        };
    };
    if !required
        || reference.change_id.is_nil()
        || reference.publisher_receipt_id.is_nil()
        || reference.publisher_receipt_digest.trim().is_empty()
        || reference.operation_ids.is_empty()
        || reference.operation_ids.iter().any(Uuid::is_nil)
        || reference
            .operation_ids
            .iter()
            .collect::<BTreeSet<_>>()
            .len()
            != reference.operation_ids.len()
    {
        return Err(Error::InvalidSource);
    }
    let principal = session_principal(tx, session).await?;
    let stored: Option<(Uuid, serde_json::Value, serde_json::Value)> = sqlx::query_as(
        "SELECT id,publisher_receipt,ready_to_commit FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2 AND change_id=$3 AND publisher_receipt IS NOT NULL AND ready_to_commit IS NOT NULL",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(reference.change_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (receipt_run, stored, ready) = stored.ok_or(Error::InvalidSource)?;
    let mut receipt: KnowledgePublisherReceipt = decode(stored)?;
    let ready: KnowledgeReadyToCommit = decode(ready)?;
    let expected_digest = receipt.digest.clone();
    receipt.digest.clear();
    if receipt.id != reference.publisher_receipt_id
        || receipt.change_id != reference.change_id
        || receipt.run_id != receipt_run
        || receipt.sealed_command_digest.trim().is_empty()
        || receipt.sealed_command_digest != ready.command_digest
        || expected_digest != reference.publisher_receipt_digest
        || digest(&receipt)? != expected_digest
    {
        return Err(Error::InvalidSource);
    }
    validate_compatibility_fields(output, reference, &receipt, &expected_digest)?;
    let applied = receipt
        .applied_operations
        .iter()
        .map(|value| (value.operation_id, value))
        .collect::<std::collections::BTreeMap<_, _>>();
    for operation_id in &reference.operation_ids {
        let applied = applied.get(operation_id).ok_or(Error::InvalidSource)?;
        validate_operation(
            tx,
            tenant,
            workspace,
            producer_run,
            reference.change_id,
            applied,
            principal,
        )
        .await?;
    }
    Ok(())
}

async fn validate_operation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    producer_run: Uuid,
    change: Uuid,
    applied: &KnowledgeAppliedOperationReceipt,
    principal: Uuid,
) -> Result<()> {
    type CurrentOperation = (
        Uuid,
        Option<i64>,
        Uuid,
        serde_json::Value,
        i64,
        String,
        bool,
        String,
        String,
        String,
        Option<String>,
        String,
        String,
        String,
    );
    let row: Option<CurrentOperation> = sqlx::query_as(
            "SELECT o.unit_id,o.applied_revision,e.id,e.event_payload,h.accepted_revision,h.lifecycle,r.payload_erased, \
             e.operation,e.rdf_digest,e.unit_iri,e.revision_iri,e.event_iri,e.rdf_digest_method,e.rdf_digest_scope \
             FROM knowledge_change_operations o \
             JOIN knowledge_publication_events e ON e.tenant_id=o.tenant_id AND e.workspace_id=o.workspace_id AND e.id=o.applied_event_id \
             JOIN knowledge_unit_heads h ON h.tenant_id=o.tenant_id AND h.workspace_id=o.workspace_id AND h.unit_id=o.unit_id \
             JOIN knowledge_revisions r ON r.tenant_id=h.tenant_id AND r.workspace_id=h.workspace_id AND r.unit_id=h.unit_id AND r.revision=h.accepted_revision \
             WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.change_id=$3 AND o.id=$4 AND e.event_payload IS NOT NULL",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(change)
        .bind(applied.operation_id)
        .fetch_optional(&mut **tx)
        .await
        .map_err(storage_error)?;
    let (
        unit,
        revision,
        event,
        payload,
        current_revision,
        lifecycle,
        erased,
        operation,
        rdf_digest,
        unit_iri,
        revision_iri,
        event_iri,
        rdf_method,
        rdf_scope,
    ) = row.ok_or(Error::InvalidSource)?;
    if unit != applied.unit_id
        || revision != applied.revision
        || event != applied.event_id
        || current_revision != applied.revision.ok_or(Error::InvalidSource)?
        || lifecycle != "active"
        || erased
        || operation != enum_text(&applied.operation)?
        || rdf_digest != applied.rdf_digest
        || unit_iri != applied.unit_iri
        || matches!(
            applied.operation,
            KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
        ) && revision_iri != applied.revision_iri
        || event_iri != applied.event_iri
        || rdf_method != applied.rdf_digest_method
        || rdf_scope != enum_text(&applied.rdf_digest_scope)?
    {
        return Err(Error::InvalidSource);
    }
    let actual = crate::knowledge_lifecycle::eligible_unit(
        tx,
        tenant,
        workspace,
        principal,
        unit,
        applied.revision,
    )
    .await?;
    let KnowledgeUnitResponse::Document(actual) = actual else {
        return Err(Error::InvalidSource);
    };
    if actual.unit_id != applied.unit_id
        || actual.revision != applied.revision.ok_or(Error::InvalidSource)?
        || actual.unit_iri != applied.unit_iri
        || matches!(
            applied.operation,
            KnowledgeLifecycleOperation::Create | KnowledgeLifecycleOperation::Revise
        ) && Some(actual.revision_iri.as_str()) != applied.revision_iri.as_deref()
    {
        return Err(Error::InvalidSource);
    }
    let sources = payload
        .get("planned")
        .and_then(|planned| {
            planned
                .get("document")
                .or_else(|| planned.get("revalidation"))
        })
        .and_then(|value| value.get("sources"))
        .cloned()
        .ok_or(Error::InvalidSource)?;
    let sources: Vec<KnowledgeSourceRef> = decode(sources)?;
    let mut producer_lineage = false;
    for source in sources {
        if let KnowledgeSourceRef::PipelineOutput { output } = source {
            let ordinary = validate_exact_source(tx, tenant, workspace, principal, &output).await?;
            producer_lineage |= ordinary && output.run_id == producer_run;
        }
    }
    if producer_lineage {
        Ok(())
    } else {
        Err(Error::InvalidSource)
    }
}

fn validate_compatibility_fields(
    output: &PipelinePhaseOutputDraft,
    reference: &KnowledgePublicationReference,
    receipt: &KnowledgePublisherReceipt,
    receipt_digest: &str,
) -> Result<()> {
    let required = [
        ("external_promotion_owner", "knowledge.change".to_owned()),
        (
            "external_promotion_reference",
            reference.publisher_receipt_id.to_string(),
        ),
        ("external_promotion_digest", receipt_digest.to_owned()),
        (
            "external_promotion_authority_evidence",
            format!("sealed-command:{}", receipt.sealed_command_digest),
        ),
    ];
    if required
        .iter()
        .any(|(field, expected)| output.fields.get(*field) != Some(expected))
    {
        Err(Error::InvalidSource)
    } else {
        Ok(())
    }
}

async fn validate_exact_source(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    source: &KnowledgePipelineOutputRef,
) -> Result<bool> {
    let row: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT pg_catalog.jsonb_build_object( \
         'body',o.body,'producer_context_id',o.producer_context_id,'fields',o.fields, \
         'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads, \
         'resource_reads',o.resource_reads,'artifacts',o.artifacts, \
         'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal, \
         'reviewer_context',a.reviewer_context,'reference',o.reference, \
         'knowledge_publication',o.knowledge_publication) \
         FROM slice_pipeline_phase_outputs o \
         JOIN slice_pipeline_phase_attempts a ON a.tenant_id=o.tenant_id AND a.workspace_id=o.workspace_id AND a.id=o.attempt_id \
         JOIN slice_pipeline_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id AND NOT b.stale \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.body_digest=$5 AND NOT o.payload_erased AND NOT a.payload_erased",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(source.run_id)
    .bind(source.output_id)
    .bind(&source.digest)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if let Some(value) = row {
        let output = verified_output(value, &source.digest)?;
        if let Some(expected) = &source.artifact {
            let artifact = output
                .artifacts
                .iter()
                .find(|value| value.name == expected.name && value.digest == expected.digest)
                .ok_or(Error::InvalidSource)?;
            if Sha256::digest(artifact.body.as_bytes())
                .iter()
                .map(|byte| format!("{byte:02x}"))
                .collect::<String>()
                != artifact.digest
            {
                return Err(Error::InvalidSource);
            }
        }
        return Ok(true);
    }
    if source.artifact.is_some() {
        return Err(Error::InvalidSource);
    }
    let value = crate::knowledge_lifecycle::current_knowledge_output(
        tx,
        tenant,
        workspace,
        principal,
        source.run_id,
        source.output_id,
        &source.digest,
    )
    .await?;
    if digest(&value.output)? != source.digest {
        Err(Error::InvalidSource)
    } else {
        Ok(false)
    }
}

fn verified_output(
    value: serde_json::Value,
    expected_digest: &str,
) -> Result<PipelinePhaseOutputDraft> {
    let output: PipelinePhaseOutputDraft = decode(value)?;
    if digest(&output)? == expected_digest {
        Ok(output)
    } else {
        Err(Error::InvalidSource)
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::BTreeMap;

    fn publication() -> (KnowledgePublicationReference, KnowledgePublisherReceipt) {
        let receipt_id = Uuid::parse_str("12345678-1234-4567-89ab-1234567890ab").unwrap();
        (
            KnowledgePublicationReference {
                change_id: Uuid::new_v4(),
                publisher_receipt_id: receipt_id,
                publisher_receipt_digest: "receipt-digest".into(),
                operation_ids: vec![Uuid::new_v4()],
            },
            KnowledgePublisherReceipt {
                id: receipt_id,
                request_id: Uuid::new_v4(),
                change_id: Uuid::new_v4(),
                run_id: Uuid::new_v4(),
                sealed_command_digest: "sealed-digest".into(),
                workspace_generation: 2,
                applied_operations: Vec::new(),
                effects: Vec::new(),
                digest: "receipt-digest".into(),
            },
        )
    }

    fn old_output() -> PipelinePhaseOutputDraft {
        PipelinePhaseOutputDraft {
            body: "immutable old producer body".into(),
            producer_context_id: "producer-context".into(),
            fields: BTreeMap::from([("proof".into(), "bounded".into())]),
            verdict: Some("handoff_ready".into()),
            dispositions: vec!["preserved".into()],
            skill_reads: Vec::new(),
            resource_reads: Vec::new(),
            artifacts: Vec::new(),
            validator_receipts: Vec::new(),
            followup_proposal: None,
            reviewer_context: None,
            reference: None,
            knowledge_publication: None,
        }
    }

    #[test]
    fn old_output_digest_reconstructs_without_a_retrofitted_null_field() {
        let output = old_output();
        let expected = digest(&output).unwrap();
        let value = json(&output).unwrap();
        assert!(value.get("knowledge_publication").is_none());
        assert_eq!(verified_output(value, &expected).unwrap(), output);
    }

    #[test]
    fn whole_output_digest_does_not_accept_a_body_only_hash() {
        let output = old_output();
        let body_only = Sha256::digest(output.body.as_bytes())
            .iter()
            .map(|byte| format!("{byte:02x}"))
            .collect::<String>();
        assert_eq!(
            verified_output(json(&output).unwrap(), &body_only),
            Err(Error::InvalidSource)
        );
    }

    #[test]
    fn promoted_compatibility_fields_are_exact_receipt_projections() {
        let (reference, receipt) = publication();
        let mut output = old_output();
        output.fields.extend([
            ("external_promotion_owner".into(), "knowledge.change".into()),
            (
                "external_promotion_reference".into(),
                reference.publisher_receipt_id.to_string(),
            ),
            (
                "external_promotion_digest".into(),
                reference.publisher_receipt_digest.clone(),
            ),
            (
                "external_promotion_authority_evidence".into(),
                "sealed-command:sealed-digest".into(),
            ),
        ]);
        assert_eq!(
            validate_compatibility_fields(
                &output,
                &reference,
                &receipt,
                &reference.publisher_receipt_digest,
            ),
            Ok(())
        );
        output.fields.insert(
            "external_promotion_reference".into(),
            Uuid::new_v4().to_string(),
        );
        assert_eq!(
            validate_compatibility_fields(
                &output,
                &reference,
                &receipt,
                &reference.publisher_receipt_digest,
            ),
            Err(Error::InvalidSource)
        );
    }
}
