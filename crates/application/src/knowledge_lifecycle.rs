use crate::{
    KnowledgeLifecycleDefinitionProvider, KnowledgeOutputGuard, PipelineDefinitionProvider,
    TransactionMode, WorkspaceService,
};
use tect_domain::*;

impl WorkspaceService {
    pub async fn knowledge_lifecycle(
        &self,
        context: &RequestContext,
        query: &KnowledgeLifecycleQuery,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<KnowledgeLifecycleResponse> {
        query.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal_id).await? {
            return Err(Error::Forbidden);
        }
        let response = tx
            .knowledge_lifecycle(workspace.id, principal_id, query)
            .await?;
        guard.lifecycle(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn knowledge_unit(
        &self,
        context: &RequestContext,
        query: &KnowledgeUnitQuery,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<KnowledgeUnitResponse> {
        query.validate()?;
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        let response = tx
            .knowledge_unit(workspace.id, principal_id, query)
            .await?
            .ok_or(Error::NotFound)?;
        guard.unit(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn knowledge_change_begin(
        &self,
        context: &RequestContext,
        request: &BeginKnowledgeChange,
        definitions: &dyn KnowledgeLifecycleDefinitionProvider,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<BeginKnowledgeChangeOutcome> {
        request.validate()?;
        let definition = definitions.definition()?;
        let registry = definitions.registry()?;
        definition.validate()?;
        registry.validate()?;
        if definition.registry_version != registry.version
            || definition.registry_digest != registry.digest
        {
            return Err(Error::InvalidConfiguration);
        }
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal_id).await? {
            return Err(Error::Forbidden);
        }
        let response = tx
            .begin_knowledge_change(
                workspace.id,
                principal_id,
                session.id,
                request,
                &definition,
                &registry,
            )
            .await?;
        guard.begin(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn knowledge_change_phase_complete(
        &self,
        context: &RequestContext,
        request: &CompleteKnowledgeChangePhase,
        definitions: &dyn PipelineDefinitionProvider,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<KnowledgeChangeMutationOutcome> {
        if request.phase_id == KnowledgeChangePhaseId::KcPrepareChange {
            request.validate_before_binding_resolution()?;
        } else {
            request.validate()?;
        }
        let (mut tx, workspace, session, principal_id) =
            self.knowledge_owner_transaction(context).await?;
        let mut resolved = request.clone();
        if let Some(output) = resolved.output.as_mut()
            && let KnowledgeAgentPhaseData::KcPrepareChange(changeset) = &mut output.data
        {
            for operation in &mut changeset.operations {
                if !operation.binding_pins.is_empty() {
                    return Err(Error::InvalidArguments);
                }
                let bindings = operation
                    .document
                    .as_ref()
                    .map(|document| document.bindings.as_slice())
                    .unwrap_or(operation.replacement_bindings.as_slice());
                let slice_phase_targets = bindings
                    .iter()
                    .enumerate()
                    .filter_map(|(index, binding)| match &binding.target {
                        KnowledgeBindingTarget::SlicePhase {
                            scope_id,
                            slice_id,
                            phase_id,
                        } => Some((index as u32, *scope_id, *slice_id, phase_id.clone())),
                        _ => None,
                    })
                    .collect::<Vec<_>>();
                for (index, scope_id, slice_id, phase_id) in slice_phase_targets {
                    let slice = tx
                        .native_slice(workspace.id, slice_id)
                        .await?
                        .ok_or(Error::NotFound)?;
                    if slice.scope_id != scope_id {
                        return Err(Error::Forbidden);
                    }
                    let (kind, version, digest, contains) =
                        if slice.pipeline == PipelineKind::PromoteToDurableKnowledge {
                            if slice.knowledge_change_id != Some(request.change_id) {
                                return Err(Error::InputConflict);
                            }
                            let current = tx
                                .knowledge_lifecycle(
                                    workspace.id,
                                    principal_id,
                                    &KnowledgeLifecycleQuery {
                                        change_id: Some(request.change_id),
                                        view: KnowledgeLifecycleView::Current,
                                        output_id: None,
                                        digest: None,
                                        fragment: None,
                                    },
                                )
                                .await?;
                            let KnowledgeLifecycleResponse::Current(current) = current else {
                                return Err(Error::InternalInvariant);
                            };
                            (
                                slice.pipeline,
                                current.definition.version.clone(),
                                current.definition.digest.clone(),
                                current
                                    .definition
                                    .phases
                                    .iter()
                                    .any(|phase| phase.id.as_str() == phase_id.as_str()),
                            )
                        } else {
                            let definition = if let Some(run_id) = slice.pipeline_run_id {
                                tx.pipeline_run_context(workspace.id, principal_id, run_id)
                                    .await?
                                    .ok_or(Error::NotFound)?
                                    .definition
                            } else {
                                definitions.definition(slice.pipeline)?
                            };
                            (
                                definition.kind,
                                definition.version,
                                definition.digest,
                                definition.phases.iter().any(|phase| phase.id == phase_id),
                            )
                        };
                    if !contains {
                        return Err(Error::InvalidArguments);
                    }
                    operation.binding_pins.push(KnowledgeResolvedBindingPin {
                        binding_index: index,
                        definition_kind: kind,
                        definition_version: version,
                        definition_digest: digest,
                        phase_id,
                    });
                }
            }
        }
        resolved.validate()?;
        let response = tx
            .complete_knowledge_change_phase(workspace.id, principal_id, session.id, &resolved)
            .await?;
        guard.mutation(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn knowledge_change_record_input(
        &self,
        context: &RequestContext,
        request: &RecordKnowledgeChangeInput,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<KnowledgeChangeMutationOutcome> {
        request.validate()?;
        let (mut tx, workspace, session, principal_id) =
            self.knowledge_owner_transaction(context).await?;
        let response = tx
            .record_knowledge_change_input(workspace.id, principal_id, session.id, request)
            .await?;
        guard.mutation(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn knowledge_change_commit(
        &self,
        context: &RequestContext,
        request: &CommitKnowledgeChange,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<CommitKnowledgeChangeOutcome> {
        request.validate()?;
        let (mut tx, workspace, session, principal_id) =
            self.knowledge_owner_transaction(context).await?;
        let response = tx
            .commit_knowledge_change(workspace.id, principal_id, session.id, request)
            .await?;
        guard.commit(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    pub async fn knowledge_change_settle_effects(
        &self,
        context: &RequestContext,
        request: &SettleKnowledgeChangeEffects,
        guard: &dyn KnowledgeOutputGuard,
    ) -> Result<SettleKnowledgeChangeEffectsOutcome> {
        request.validate()?;
        let (mut tx, workspace, session, principal_id) =
            self.knowledge_owner_transaction(context).await?;
        let response = tx
            .settle_knowledge_change_effects(workspace.id, principal_id, session.id, request)
            .await?;
        guard.effects(&response)?;
        tx.commit().await?;
        Ok(response)
    }

    async fn knowledge_owner_transaction(
        &self,
        context: &RequestContext,
    ) -> Result<(Box<dyn crate::UnitOfWork>, Workspace, Session, uuid::Uuid)> {
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadWrite)
            .await?;
        let principal_id = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal_id).await? {
            return Err(Error::Forbidden);
        }
        Ok((tx, workspace, session, principal_id))
    }
}
