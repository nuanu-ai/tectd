use super::*;

pub(in crate::knowledge_lifecycle) async fn phase_digest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    phase_id: KnowledgeChangePhaseId,
) -> Result<String> {
    sqlx::query_scalar("SELECT o.digest FROM knowledge_change_output_bindings b JOIN knowledge_change_outputs o ON o.tenant_id=b.tenant_id AND o.workspace_id=b.workspace_id AND o.id=b.output_id WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.run_id=$3 AND b.phase_id=$4 AND b.stale=false AND o.payload_erased=false")
        .bind(tenant).bind(workspace).bind(run).bind(enum_text(&phase_id)?).fetch_optional(&mut **tx).await.map_err(storage_error)?.ok_or(Error::NeedsContext)
}

pub(in crate::knowledge_lifecycle) fn source_iri(
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    source_revision: i64,
    index: usize,
) -> String {
    if source_revision == 0 {
        format!("urn:tect:dk:source:{tenant}:{workspace}:{change}:{index}")
    } else {
        format!("urn:tect:dk:source:{tenant}:{workspace}:{change}:set:{source_revision}:{index}")
    }
}

pub(in crate::knowledge_lifecycle) async fn resolve_sources_at_revision(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    source_revision: i64,
    sources: &[KnowledgeSourceRef],
) -> Result<Vec<super::rdf::ResolvedSourcePayload>> {
    let mut values = Vec::with_capacity(sources.len());
    for (index, source) in sources.iter().enumerate() {
        let (evidence_kind, observed_at, evidence_scope, title, uri, text) = match source {
            KnowledgeSourceRef::Snapshot { snapshot } => (
                snapshot.evidence_kind,
                snapshot.observed_at.clone(),
                snapshot.title.clone(),
                snapshot.title.clone(),
                snapshot.uri.clone(),
                snapshot.text.clone(),
            ),
            KnowledgeSourceRef::PipelineOutput { output } => {
                let ordinary:Option<(serde_json::Value,String,String,bool)>=sqlx::query_as("SELECT pg_catalog.jsonb_build_object('body',o.body,'producer_context_id',o.producer_context_id,'fields',o.fields,'verdict',o.verdict,'dispositions',o.dispositions,'skill_reads',o.skill_reads,'resource_reads',o.resource_reads,'artifacts',o.artifacts,'validator_receipts',o.validator_receipts,'followup_proposal',o.followup_proposal,'reviewer_context',a.reviewer_context,'reference',o.reference,'knowledge_publication',o.knowledge_publication),o.body_digest,o.phase_id,COALESCE(b.stale,true) FROM slice_pipeline_phase_outputs o JOIN slice_pipeline_phase_attempts a ON a.tenant_id=o.tenant_id AND a.workspace_id=o.workspace_id AND a.id=o.attempt_id LEFT JOIN slice_pipeline_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.body_digest=$5").bind(tenant).bind(workspace).bind(output.run_id).bind(output.output_id).bind(&output.digest).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                if let Some((stored, body_digest, title, stale)) = ordinary {
                    let draft: PipelinePhaseOutputDraft = decode(stored)?;
                    if stale || digest(&draft)? != body_digest {
                        return Err(Error::InvalidSource);
                    }
                    let text = if let Some(expected) = &output.artifact {
                        let artifact = draft
                            .artifacts
                            .into_iter()
                            .find(|value| {
                                value.name == expected.name && value.digest == expected.digest
                            })
                            .ok_or(Error::InvalidSource)?;
                        if sha256_bytes(artifact.body.as_bytes()) != artifact.digest {
                            return Err(Error::InvalidSource);
                        }
                        artifact.body
                    } else {
                        draft.body
                    };
                    (
                        output.evidence_kind,
                        output.observed_at.clone(),
                        output.evidence_scope.clone(),
                        title,
                        format!(
                            "urn:tect:pipeline-output:{}:{}",
                            output.run_id, output.output_id
                        ),
                        text,
                    )
                } else {
                    if output.artifact.is_some() {
                        return Err(Error::InvalidSource);
                    }
                    let knowledge:Option<(serde_json::Value,bool,bool)>=sqlx::query_as("SELECT o.output,o.payload_erased,COALESCE(b.stale,true) FROM knowledge_change_outputs o LEFT JOIN knowledge_change_output_bindings b ON b.tenant_id=o.tenant_id AND b.workspace_id=o.workspace_id AND b.run_id=o.run_id AND b.output_id=o.id WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.run_id=$3 AND o.id=$4 AND o.digest=$5").bind(tenant).bind(workspace).bind(output.run_id).bind(output.output_id).bind(&output.digest).fetch_optional(&mut **tx).await.map_err(storage_error)?;
                    let (value, erased, stale) = knowledge.ok_or(Error::InvalidSource)?;
                    if erased || stale {
                        return Err(Error::InvalidSource);
                    }
                    let value: KnowledgeAgentPhaseOutputDraft = decode(value)?;
                    if digest(&value)? != output.digest {
                        return Err(Error::InvalidSource);
                    }
                    (
                        output.evidence_kind,
                        output.observed_at.clone(),
                        output.evidence_scope.clone(),
                        value.phase_id.as_str().into(),
                        format!(
                            "urn:tect:knowledge-output:{}:{}",
                            output.run_id, output.output_id
                        ),
                        value.body,
                    )
                }
            }
        };
        let digest = sha256_bytes(text.as_bytes());
        values.push(super::rdf::ResolvedSourcePayload {
            pin: KnowledgeResolvedSourcePin {
                source_index: index as u32,
                digest,
                evidence_kind,
                observed_at,
                evidence_scope,
                source_iri: source_iri(tenant, workspace, change, source_revision, index),
            },
            title,
            uri,
            text,
        });
    }
    Ok(values)
}

pub(in crate::knowledge_lifecycle) async fn resolve_sources(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
    sources: &[KnowledgeSourceRef],
) -> Result<Vec<super::rdf::ResolvedSourcePayload>> {
    let source_revision: i64 = sqlx::query_scalar(
        "SELECT source_revision FROM knowledge_lifecycle_changes \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(change)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .unwrap_or(0);
    resolve_sources_at_revision(tx, tenant, workspace, change, source_revision, sources).await
}

pub(in crate::knowledge_lifecycle) async fn current_source_pin_digest(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    change: Uuid,
) -> Result<String> {
    let sources: Vec<KnowledgeSourceRef> = decode(
        sqlx::query_scalar("SELECT sources FROM knowledge_lifecycle_changes WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(change).fetch_one(&mut **tx).await.map_err(storage_error)?,
    )?;
    match resolve_sources(tx, tenant, workspace, change, &sources).await {
        Ok(values) => digest(
            &values
                .into_iter()
                .map(|value| value.pin)
                .collect::<Vec<_>>(),
        ),
        Err(Error::InvalidSource) => Ok(String::new()),
        Err(error) => Err(error),
    }
}
