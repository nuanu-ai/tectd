use super::*;

pub(super) fn binding(row: &BindingRow) -> Result<PipelineKnowledgeBindingPin> {
    let target = match row.binding_kind.as_str() {
        "workspace" => KnowledgeBindingTarget::Workspace,
        "program" => KnowledgeBindingTarget::Program {
            program_id: row.program_id.ok_or(Error::InternalInvariant)?,
        },
        "scope" => KnowledgeBindingTarget::Scope {
            scope_id: row.scope_id.ok_or(Error::InternalInvariant)?,
        },
        "slice" => KnowledgeBindingTarget::Slice {
            scope_id: row.scope_id.ok_or(Error::InternalInvariant)?,
            slice_id: row.slice_id.ok_or(Error::InternalInvariant)?,
        },
        "slice_phase" => KnowledgeBindingTarget::SlicePhase {
            scope_id: row.scope_id.ok_or(Error::InternalInvariant)?,
            slice_id: row.slice_id.ok_or(Error::InternalInvariant)?,
            phase_id: row.phase_id.clone().ok_or(Error::InternalInvariant)?,
        },
        _ => return Err(Error::InternalInvariant),
    };
    Ok(PipelineKnowledgeBindingPin {
        binding_iri: format!("urn:tect:dk:binding:{}", row.binding_id),
        target,
        purpose: decode(serde_json::Value::String(row.purpose.clone()))?,
        version_resolution: if row.version_resolution == "pinned_revision" {
            KnowledgeBindingVersion::PinnedRevision {
                revision: row.revision,
            }
        } else {
            KnowledgeBindingVersion::CurrentAccepted
        },
        definition_kind: row.definition_kind.clone(),
        definition_version: row.definition_version.clone(),
        definition_digest: row.definition_digest.clone(),
    })
}

pub(super) fn blocking(purpose: KnowledgeBindingPurpose) -> bool {
    !matches!(purpose, KnowledgeBindingPurpose::Reference)
}

pub(super) async fn covered_supersession(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &BindingRow,
    mut proofs: Option<crate::knowledge_lifecycle::PublicationProofContext<'_>>,
) -> Result<bool> {
    if row.version_resolution != "current_accepted" {
        return Ok(false);
    }
    let candidates: Vec<(Uuid, Uuid)> = sqlx::query_as(
        "SELECT sup.event_id,sup.successor_unit_id FROM knowledge_supersessions sup WHERE sup.tenant_id=$1 AND sup.workspace_id=$2 AND sup.predecessor_unit_id=$3 AND $4=ANY(sup.replacement_binding_ids) AND EXISTS(SELECT 1 FROM knowledge_bindings replacement JOIN knowledge_unit_heads successor ON successor.tenant_id=replacement.tenant_id AND successor.workspace_id=replacement.workspace_id AND successor.unit_id=replacement.unit_id WHERE replacement.tenant_id=sup.tenant_id AND replacement.workspace_id=sup.workspace_id AND replacement.unit_id=sup.successor_unit_id AND replacement.active AND replacement.revision=successor.accepted_revision AND replacement.binding_kind=$5 AND replacement.program_id IS NOT DISTINCT FROM $6 AND replacement.scope_id IS NOT DISTINCT FROM $7 AND replacement.slice_id IS NOT DISTINCT FROM $8 AND replacement.phase_id IS NOT DISTINCT FROM $9 AND replacement.purpose=$10 AND replacement.version_resolution='current_accepted' AND successor.active AND NOT successor.payload_erased)",
    )
    .bind(tenant).bind(workspace).bind(row.unit_id).bind(row.binding_id)
    .bind(&row.binding_kind).bind(row.program_id).bind(row.scope_id).bind(row.slice_id)
    .bind(&row.phase_id).bind(&row.purpose)
    .fetch_all(&mut **tx).await.map_err(storage_error)?;
    let expected = KnowledgeDocumentBinding {
        target: binding(row)?.target,
        purpose: decode(serde_json::Value::String(row.purpose.clone()))?,
        version_resolution: KnowledgeBindingVersion::CurrentAccepted,
    };
    for (event, successor) in candidates {
        let verified = crate::knowledge_lifecycle::verify_publication_event_with_proofs(
            tx,
            tenant,
            workspace,
            row.unit_id,
            row.revision,
            event,
            false,
            proofs
                .as_mut()
                .map(|(principal, session, scope)| (*principal, *session, &mut **scope)),
        )
        .await?;
        if verified.input.successor_unit == Some(successor)
            && verified.input.planned.operation == KnowledgeLifecycleOperation::Supersede
            && verified
                .input
                .planned
                .replacement_bindings
                .contains(&expected)
        {
            return Ok(true);
        }
    }
    Ok(false)
}

pub(super) fn methods(
    definition: &PipelineDefinitionSnapshot,
    phase: &str,
) -> Result<Vec<KnowledgeContractRef>> {
    let phase = definition
        .phases
        .iter()
        .find(|value| value.id == phase)
        .ok_or(Error::InternalInvariant)?;
    let mut values =
        phase
            .instructions
            .iter()
            .chain(&phase.skills)
            .chain(&phase.resources)
            .map(|value| KnowledgeContractRef {
                id: value.id.clone(),
                version: value.version.clone(),
                digest: value.digest.clone(),
                source_ref: value.origin_refs.first().cloned().unwrap_or_else(|| {
                    format!("embedded:{}:{}", definition.kind.as_str(), phase.id)
                }),
            })
            .collect::<Vec<_>>();
    values.sort();
    values.dedup();
    Ok(values)
}
