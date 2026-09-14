use crate::{PipelineDefinitionProvider, TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use tect_domain::*;
use uuid::Uuid;

fn method(id: &str, body: &str) -> KnowledgeMethodSnapshot {
    KnowledgeMethodSnapshot {
        id: id.to_owned(),
        version: DK_METHOD_VERSION.to_owned(),
        digest: Sha256::digest(body.as_bytes())
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect(),
        body: body.to_owned(),
    }
}

fn methods() -> (KnowledgeMethodSnapshot, KnowledgeMethodSnapshot) {
    (
        method(DK_PREPARATION_METHOD_ID, DK_PREPARATION_METHOD_BODY),
        method(DK_REVIEW_METHOD_ID, DK_REVIEW_METHOD_BODY),
    )
}

fn validate_draft_capacity(draft: Option<&KnowledgeConstraintDraft>) -> Result<()> {
    match draft.map(serde_json::to_vec).transpose() {
        Ok(Some(bytes)) if bytes.len() > DK_MAX_DRAFT_BYTES => Err(Error::CapacityExceeded),
        Ok(_) => Ok(()),
        Err(_) => Err(Error::InvalidArguments),
    }
}

impl WorkspaceService {
    pub async fn knowledge_context(
        &self,
        context: &RequestContext,
        query: &KnowledgeContextQuery,
    ) -> Result<KnowledgeContext> {
        query.validate()?;
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let (preparation, review) = methods();
        let value = tx
            .knowledge_context(workspace.id, query, &preparation, &review)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn knowledge_change(
        &self,
        context: &RequestContext,
        change_id: Uuid,
    ) -> Result<KnowledgeChange> {
        if change_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, workspace, _) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let value = tx
            .knowledge_change(workspace.id, change_id)
            .await?
            .ok_or(Error::NotFound)?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn knowledge_change_prepare(
        &self,
        context: &RequestContext,
        request: &PrepareKnowledgeChange,
        definitions: &dyn PipelineDefinitionProvider,
    ) -> Result<PrepareKnowledgeChangeOutcome> {
        request.validate()?;
        validate_draft_capacity(request.draft.as_ref())?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal_id).await? {
            return Err(Error::Forbidden);
        }
        let provenance = if let Some(draft) = &request.draft {
            match &draft.binding {
                KnowledgeBinding::Workspace => None,
                KnowledgeBinding::SlicePhase {
                    scope_id,
                    slice_id,
                    phase_id,
                } => {
                    let slice = tx
                        .native_slice(workspace.id, *slice_id)
                        .await?
                        .ok_or(Error::NotFound)?;
                    if slice.scope_id != *scope_id {
                        return Err(Error::Forbidden);
                    }
                    let definition = if let Some(run_id) = slice.pipeline_run_id {
                        tx.pipeline_run_context(workspace.id, principal_id, run_id)
                            .await?
                            .ok_or(Error::NotFound)?
                            .definition
                    } else {
                        definitions.definition(slice.pipeline)?
                    };
                    definition.validate()?;
                    if !definition.phases.iter().any(|phase| phase.id == *phase_id) {
                        return Err(Error::InvalidArguments);
                    }
                    Some(KnowledgeBindingProvenance {
                        definition_kind: definition.kind,
                        definition_version: definition.version,
                        definition_digest: definition.digest,
                    })
                }
            }
        } else {
            None
        };
        let (preparation, review) = methods();
        let value = tx
            .prepare_knowledge_change(
                workspace.id,
                principal_id,
                session.id,
                request,
                provenance.as_ref(),
                &preparation,
                &review,
            )
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn knowledge_change_review(
        &self,
        context: &RequestContext,
        request: &ReviewKnowledgeChange,
    ) -> Result<ReviewKnowledgeChangeOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal_id).await? {
            return Err(Error::Forbidden);
        }
        let value = tx
            .review_knowledge_change(workspace.id, principal_id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn knowledge_change_publish(
        &self,
        context: &RequestContext,
        request: &PublishKnowledgeChange,
    ) -> Result<PublishKnowledgeChangeOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal_id).await? {
            return Err(Error::Forbidden);
        }
        let value = tx
            .publish_knowledge_change(workspace.id, principal_id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }

    pub async fn pipeline_knowledge_refresh(
        &self,
        context: &RequestContext,
        request: &RefreshPipelineKnowledge,
    ) -> Result<RefreshPipelineKnowledgeOutcome> {
        request.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let value = tx
            .refresh_pipeline_knowledge(workspace.id, session.id, request)
            .await?;
        tx.commit().await?;
        Ok(value)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_aggregate_draft_larger_than_native_publication_budget() {
        let draft = KnowledgeConstraintDraft {
            title: "title".into(),
            statement: "s".repeat(DK_MAX_TEXT_BYTES),
            modality: KnowledgeModality::Must,
            action: "act".into(),
            target_iri: "urn:target".into(),
            conditions: Vec::new(),
            exceptions: Vec::new(),
            source: KnowledgeSourceSnapshot {
                title: "source".into(),
                uri: "urn:source".into(),
                text: "x".repeat(DK_MAX_TEXT_BYTES),
            },
            binding: KnowledgeBinding::Workspace,
            purpose: KnowledgePurpose::ExecutionConstraint,
            version_resolution: KnowledgeVersionResolution::CurrentAccepted,
        };
        assert_eq!(
            validate_draft_capacity(Some(&draft)),
            Err(Error::CapacityExceeded)
        );
    }
}
