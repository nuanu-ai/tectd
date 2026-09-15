use super::*;

pub(super) struct TypedResource {
    pub resource: Option<PipelineKnowledgeResource>,
    pub needs_context: bool,
    pub valid_from: Option<String>,
    pub valid_until: Option<String>,
    pub review_due_at: Option<String>,
}

async fn latest_validation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
    revision: i64,
) -> Result<Option<PipelineKnowledgeValidationPin>> {
    let row: Option<(Uuid, i64)> = sqlx::query_as(
        "SELECT v.id,(SELECT count(*) FROM knowledge_validation_events x WHERE x.tenant_id=v.tenant_id AND x.workspace_id=v.workspace_id AND x.unit_id=v.unit_id AND x.unit_revision=v.unit_revision AND NOT x.payload_erased AND (x.created_at,x.id)<=(v.created_at,v.id)) FROM knowledge_validation_events v WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=$3 AND v.unit_revision=$4 AND NOT v.payload_erased ORDER BY v.created_at DESC,v.id DESC LIMIT 1",
    ).bind(tenant).bind(workspace).bind(unit).bind(revision).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    let Some((event_id, sequence)) = row else {
        return Ok(None);
    };
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx, tenant, workspace, unit, revision, event_id, false,
    )
    .await?;
    if verified.input.planned.operation != KnowledgeLifecycleOperation::Revalidate {
        return Err(Error::InternalInvariant);
    }
    let revalidation = verified
        .input
        .planned
        .revalidation
        .as_ref()
        .ok_or(Error::InternalInvariant)?;
    let source_pin_digest = digest(&verified.input.resolved_sources)?;
    let exact: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM knowledge_validation_events WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND unit_id=$4 AND unit_revision=$5 AND NOT payload_erased AND sources=$6 AND source_pin_digest=$7 AND evidence_basis=$8 AND valid_until IS NOT DISTINCT FROM $9::timestamptz AND review_due_at IS NOT DISTINCT FROM $10::timestamptz)")
        .bind(tenant).bind(workspace).bind(event_id).bind(unit).bind(revision)
        .bind(json(&revalidation.sources)?).bind(source_pin_digest).bind(&revalidation.evidence_basis)
        .bind(&revalidation.valid_until).bind(&revalidation.review_due_at)
        .fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !exact {
        return Err(Error::InternalInvariant);
    }
    Ok(Some(PipelineKnowledgeValidationPin {
        event_id,
        event_iri: format!("urn:tect:dk:event:{tenant}:{workspace}:{event_id}"),
        event_digest: verified.rdf_digest,
        sequence,
        valid_until: revalidation.valid_until.clone(),
        review_due_at: revalidation.review_due_at.clone(),
        source_pins: verified
            .input
            .resolved_sources
            .into_iter()
            .map(|value| PipelineKnowledgeSourcePin {
                source_iri: value.pin.source_iri,
                digest: value.pin.digest,
                evidence_kind: value.pin.evidence_kind,
                observed_at: value.pin.observed_at,
                evidence_scope: value.pin.evidence_scope,
                title: value.title,
                uri: value.uri,
            })
            .collect(),
    }))
}

pub(super) async fn typed(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &BindingRow,
    projection: Option<&crate::durable_knowledge::manifest::inquiry::Projection>,
    purpose: KnowledgeBindingPurpose,
) -> Result<TypedResource> {
    let event_id = row.event_id.ok_or(Error::InternalInvariant)?;
    let verified = crate::knowledge_lifecycle::verify_publication_event(
        tx,
        tenant,
        workspace,
        row.unit_id,
        row.revision,
        event_id,
        true,
    )
    .await?;
    if row.rdf_digest.as_deref() != Some(verified.rdf_digest.as_str()) {
        return Err(Error::InternalInvariant);
    }
    let input = verified.input;
    let Some(document) = input.planned.document.as_ref() else {
        return Ok(TypedResource {
            resource: None,
            needs_context: false,
            valid_from: None,
            valid_until: None,
            review_due_at: None,
        });
    };
    if input.planned.unit_id != row.unit_id || input.content_revision != row.revision {
        return Err(Error::InternalInvariant);
    }
    let selection = projection
        .map(|value| value.select(document, purpose))
        .unwrap_or(crate::durable_knowledge::manifest::inquiry::BriefSelection::Full);
    let (
        canonical_text,
        target_iris,
        conditions,
        exceptions,
        sections,
        inquiry_briefs,
        needs_context,
    ) = match selection {
        crate::durable_knowledge::manifest::inquiry::BriefSelection::Full => (
            document.canonical_text.clone(),
            document.target_iris.clone(),
            document.conditions.clone(),
            document.exceptions.clone(),
            document.sections.clone(),
            None,
            false,
        ),
        crate::durable_knowledge::manifest::inquiry::BriefSelection::Omit { needs_context } => {
            return Ok(TypedResource {
                resource: None,
                needs_context,
                valid_from: document.valid_from.clone(),
                valid_until: document.valid_until.clone(),
                review_due_at: document.review_due_at.clone(),
            });
        }
        crate::durable_knowledge::manifest::inquiry::BriefSelection::Briefs {
            values,
            needs_context,
        } => (
            crate::durable_knowledge::manifest::inquiry::projected_text(&values),
            crate::durable_knowledge::manifest::inquiry::projected_targets(&values),
            crate::durable_knowledge::manifest::inquiry::projected_conditions(&values),
            crate::durable_knowledge::manifest::inquiry::projected_exceptions(&values),
            KnowledgeProfileSections::default(),
            Some(values),
            needs_context,
        ),
    };
    let expected = rdf::build(&input)?;
    let latest = latest_validation(tx, tenant, workspace, row.unit_id, row.revision).await?;
    let valid_until = latest
        .as_ref()
        .and_then(|value| value.valid_until.clone())
        .or_else(|| document.valid_until.clone());
    let review_due_at = latest
        .as_ref()
        .and_then(|value| value.review_due_at.clone())
        .or_else(|| document.review_due_at.clone());
    Ok(TypedResource {
        needs_context,
        valid_from: document.valid_from.clone(),
        valid_until,
        review_due_at,
        resource: Some(PipelineKnowledgeResource {
            unit_id: row.unit_id,
            revision: row.revision,
            lifecycle: decode(serde_json::Value::String(row.lifecycle.clone()))?,
            access_scope: decode(serde_json::Value::String(row.head_access.clone()))?,
            rdf_digest: row.rdf_digest.clone().ok_or(Error::InternalInvariant)?,
            unit_iri: expected.refs.unit,
            revision_iri: expected.refs.revision,
            title: document.title.clone(),
            canonical_text,
            knowledge_kind: document.knowledge_kind,
            epistemic_state: document.epistemic_state,
            target_iris,
            profiles: document.profiles.clone(),
            conditions,
            exceptions,
            sections,
            inquiry_briefs,
            source_pins: input
                .resolved_sources
                .into_iter()
                .map(|value| PipelineKnowledgeSourcePin {
                    source_iri: value.pin.source_iri,
                    digest: value.pin.digest,
                    evidence_kind: value.pin.evidence_kind,
                    observed_at: value.pin.observed_at,
                    evidence_scope: value.pin.evidence_scope,
                    title: value.title,
                    uri: value.uri,
                })
                .collect(),
            latest_validation: latest,
            binding: binding(row)?,
            why_included: format!("{}_binding", row.binding_kind),
        }),
    })
}

pub(super) async fn legacy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    row: &BindingRow,
) -> Result<PipelineKnowledgeResource> {
    let value =
        context::load_revision(tx, tenant, workspace, row.unit_id, Some(row.revision), true)
            .await?
            .ok_or(Error::InternalInvariant)?;
    let target = value.constraint.target_iri.clone();
    Ok(PipelineKnowledgeResource {
        unit_id: value.unit_id,
        revision: value.revision,
        lifecycle: if value.active {
            KnowledgeLifecycleState::Active
        } else {
            KnowledgeLifecycleState::Retracted
        },
        access_scope: decode(serde_json::Value::String(row.head_access.clone()))?,
        rdf_digest: value.rdf_digest,
        unit_iri: value.unit_iri,
        revision_iri: value.revision_iri,
        title: value.constraint.title.clone(),
        canonical_text: value.constraint.statement.clone(),
        knowledge_kind: KnowledgeKind::Constraint,
        epistemic_state: KnowledgeEpistemicState::Normative,
        target_iris: vec![target.clone()],
        profiles: vec![KnowledgeProfileId::General],
        conditions: value.constraint.conditions.clone(),
        exceptions: value.constraint.exceptions.clone(),
        sections: KnowledgeProfileSections {
            constraint: Some(KnowledgeConstraintSection {
                modality: value.constraint.modality,
                action: value.constraint.action.clone(),
                target_iri: target,
            }),
            ..Default::default()
        },
        inquiry_briefs: None,
        source_pins: vec![PipelineKnowledgeSourcePin {
            source_iri: value.source_iri,
            digest: value.source_sha256,
            evidence_kind: KnowledgeEvidenceKind::Document,
            observed_at: None,
            evidence_scope: "legacy_dk1_revision".into(),
            title: value.constraint.source.title,
            uri: value.constraint.source.uri,
        }],
        latest_validation: None,
        binding: binding(row)?,
        why_included: format!("{}_binding", row.binding_kind),
    })
}
