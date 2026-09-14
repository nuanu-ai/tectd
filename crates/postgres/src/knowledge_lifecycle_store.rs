use crate::{durable_knowledge, knowledge_lifecycle, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::KnowledgeLifecycleStore;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
impl KnowledgeLifecycleStore for PgUnitOfWork {
    async fn knowledge_lifecycle(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        query: &KnowledgeLifecycleQuery,
    ) -> Result<KnowledgeLifecycleResponse> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::lifecycle(self.transaction()?, tenant, workspace, principal, query)
            .await
    }
    async fn knowledge_unit(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        query: &KnowledgeUnitQuery,
    ) -> Result<Option<KnowledgeUnitResponse>> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::unit(self.transaction()?, tenant, workspace, principal, query).await
    }
    async fn begin_knowledge_change(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &BeginKnowledgeChange,
        definition: &KnowledgeChangeDefinition,
        registry: &KnowledgeProfileRegistry,
    ) -> Result<BeginKnowledgeChangeOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::begin(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            session,
            request,
            definition,
            registry,
        )
        .await
    }
    async fn complete_knowledge_change_phase(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &CompleteKnowledgeChangePhase,
    ) -> Result<KnowledgeChangeMutationOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::complete_phase(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            session,
            request,
        )
        .await
    }
    async fn record_knowledge_change_input(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &RecordKnowledgeChangeInput,
    ) -> Result<KnowledgeChangeMutationOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::record_input(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            session,
            request,
        )
        .await
    }
    async fn commit_knowledge_change(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &CommitKnowledgeChange,
    ) -> Result<CommitKnowledgeChangeOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::commit(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            session,
            request,
        )
        .await
    }
    async fn settle_knowledge_change_effects(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &SettleKnowledgeChangeEffects,
    ) -> Result<SettleKnowledgeChangeEffectsOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        knowledge_lifecycle::settle(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            session,
            request,
        )
        .await
    }
}
