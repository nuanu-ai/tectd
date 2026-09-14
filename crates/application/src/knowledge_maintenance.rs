use crate::{
    KnowledgeLifecycleDefinitionProvider, KnowledgeMaintenanceOutputGuard, TransactionMode,
    WorkspaceService,
};
use tect_domain::*;

impl WorkspaceService {
    pub async fn knowledge_maintenance(
        &self,
        context: &RequestContext,
        query: &KnowledgeMaintenanceQuery,
        definitions: &dyn KnowledgeLifecycleDefinitionProvider,
    ) -> Result<KnowledgeMaintenanceContext> {
        query.validate()?;
        let method = definitions.maintenance_method()?;
        if method.id.is_empty()
            || method.version.is_empty()
            || method.digest.is_empty()
            || method.body.is_empty()
        {
            return Err(Error::InvalidConfiguration);
        }
        let (mut tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let principal = tx.session_principal(session.id).await?;
        if !tx.knowledge_owner(principal).await? {
            return Err(Error::Forbidden);
        }
        let result = tx
            .knowledge_maintenance(workspace.id, principal, query, &method)
            .await?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn observe_knowledge_maintenance(
        &self,
        context: &RequestContext,
        request: &ObserveKnowledgeMaintenanceSignal,
        guard: &dyn KnowledgeMaintenanceOutputGuard,
    ) -> Result<ObserveKnowledgeMaintenanceOutcome> {
        request.validate()?;
        let (mut tx, workspace, session, principal) =
            self.knowledge_owner_transaction(context).await?;
        let result = tx
            .observe_knowledge_maintenance(workspace.id, principal, session.id, request)
            .await?;
        guard.observe(&result)?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn begin_knowledge_maintenance_change(
        &self,
        context: &RequestContext,
        request: &BeginKnowledgeMaintenanceChange,
        definitions: &dyn KnowledgeLifecycleDefinitionProvider,
        guard: &dyn KnowledgeMaintenanceOutputGuard,
    ) -> Result<BeginKnowledgeMaintenanceChangeOutcome> {
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
        let (mut tx, workspace, session, principal) =
            self.knowledge_owner_transaction(context).await?;
        let result = tx
            .begin_knowledge_maintenance_change(
                workspace.id,
                principal,
                session.id,
                request,
                &definition,
                &registry,
            )
            .await?;
        guard.begin(&result)?;
        tx.commit().await?;
        Ok(result)
    }

    pub async fn process_knowledge_maintenance_tasks(
        &self,
        context: &RequestContext,
        limit: u32,
    ) -> Result<KnowledgeMaintenanceProcessOutcome> {
        if limit == 0 || limit > KNOWLEDGE_MAINTENANCE_MAX_BATCH {
            return Err(Error::InvalidArguments);
        }
        let (mut sweep, workspace, session, principal) =
            self.knowledge_owner_transaction(context).await?;
        let due_created = sweep
            .sweep_due_knowledge_maintenance(workspace.id, principal, limit)
            .await?;
        sweep.commit().await?;
        let mut result = KnowledgeMaintenanceProcessOutcome {
            due_created,
            claimed: 0,
            needs_review: 0,
            obsolete: 0,
            exhausted: 0,
            retry_scheduled: 0,
            pending: 0,
        };
        for _ in 0..limit {
            let (mut claim_tx, current_workspace, current_session, current_principal) =
                self.knowledge_owner_transaction(context).await?;
            if current_workspace.id != workspace.id
                || current_session.id != session.id
                || current_principal != principal
            {
                return Err(Error::ContextChanged);
            }
            let claimed = claim_tx
                .claim_knowledge_maintenance_task(workspace.id, principal)
                .await?;
            claim_tx.commit().await?;
            result.exhausted = result
                .exhausted
                .checked_add(claimed.exhausted)
                .ok_or(Error::InternalInvariant)?;
            let Some(claim) = claimed.claim else { break };
            result.claimed += 1;
            let (mut finish, finish_workspace, _, finish_principal) =
                self.knowledge_owner_transaction(context).await?;
            if finish_workspace.id != claim.workspace_id || finish_principal != claim.principal_id {
                return Err(Error::ContextChanged);
            }
            let prepared = finish
                .prepare_knowledge_maintenance_task(workspace.id, principal, &claim)
                .await;
            match prepared {
                Ok(KnowledgeMaintenancePrepareOutcome::NeedsReview) => result.needs_review += 1,
                Ok(KnowledgeMaintenancePrepareOutcome::Obsolete) => result.obsolete += 1,
                Ok(KnowledgeMaintenancePrepareOutcome::Exhausted) => result.exhausted += 1,
                Err(error) => {
                    let code = match error {
                        Error::StorageUnavailable => {
                            KnowledgeMaintenanceFailureCode::StorageUnavailable
                        }
                        Error::TransportUnavailable => {
                            KnowledgeMaintenanceFailureCode::TransportUnavailable
                        }
                        Error::InvalidConfiguration => {
                            KnowledgeMaintenanceFailureCode::InvalidConfiguration
                        }
                        Error::InternalInvariant => {
                            KnowledgeMaintenanceFailureCode::InternalInvariant
                        }
                        Error::CapacityExceeded => {
                            KnowledgeMaintenanceFailureCode::CapacityExceeded
                        }
                        Error::NeedsContext => KnowledgeMaintenanceFailureCode::NeedsContext,
                        Error::ContextChanged => KnowledgeMaintenanceFailureCode::LeaseExpired,
                        _ => return Err(error),
                    };
                    drop(finish);
                    let (mut failed, failed_workspace, _, failed_principal) =
                        self.knowledge_owner_transaction(context).await?;
                    if failed_workspace.id != claim.workspace_id
                        || failed_principal != claim.principal_id
                    {
                        return Err(Error::ContextChanged);
                    }
                    match failed
                        .fail_knowledge_maintenance_task(
                            failed_workspace.id,
                            failed_principal,
                            &claim,
                            code,
                        )
                        .await?
                    {
                        KnowledgeMaintenanceFailureOutcome::RetryScheduled => {
                            result.retry_scheduled += 1
                        }
                        KnowledgeMaintenanceFailureOutcome::NeedsReview => result.needs_review += 1,
                        KnowledgeMaintenanceFailureOutcome::Exhausted => result.exhausted += 1,
                    }
                    failed.commit().await?;
                    continue;
                }
            }
            finish.commit().await?;
        }
        let (mut pending, current_workspace, _, current_principal) =
            self.knowledge_owner_transaction(context).await?;
        if current_workspace.id != workspace.id || current_principal != principal {
            return Err(Error::ContextChanged);
        }
        result.pending = pending
            .pending_knowledge_maintenance_tasks(workspace.id, principal)
            .await?;
        pending.commit().await?;
        Ok(result)
    }
}
