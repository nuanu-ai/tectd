use crate::{knowledge_maintenance, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::KnowledgeMaintenanceStore;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
impl KnowledgeMaintenanceStore for PgUnitOfWork {
    async fn knowledge_maintenance(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        query: &KnowledgeMaintenanceQuery,
        method: &PipelineInstructionSnapshot,
    ) -> Result<KnowledgeMaintenanceContext> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::context(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            query,
            method,
        )
        .await
    }

    async fn observe_knowledge_maintenance(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &ObserveKnowledgeMaintenanceSignal,
    ) -> Result<ObserveKnowledgeMaintenanceOutcome> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::observe(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            session,
            request,
        )
        .await
    }

    async fn sweep_due_knowledge_maintenance(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        limit: u32,
    ) -> Result<u32> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::sweep_due(self.transaction()?, tenant, workspace, principal, limit)
            .await
    }

    async fn claim_knowledge_maintenance_task(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
    ) -> Result<KnowledgeMaintenanceClaimOutcome> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::claim(self.transaction()?, tenant, workspace, principal).await
    }

    async fn prepare_knowledge_maintenance_task(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        claim: &KnowledgeMaintenanceJobClaim,
    ) -> Result<KnowledgeMaintenancePrepareOutcome> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::prepare(self.transaction()?, tenant, workspace, principal, claim)
            .await
    }

    async fn fail_knowledge_maintenance_task(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        claim: &KnowledgeMaintenanceJobClaim,
        code: KnowledgeMaintenanceFailureCode,
    ) -> Result<KnowledgeMaintenanceFailureOutcome> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::fail(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            claim,
            code,
        )
        .await
    }

    async fn pending_knowledge_maintenance_tasks(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
    ) -> Result<u32> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::pending(self.transaction()?, tenant, workspace, principal).await
    }

    async fn begin_knowledge_maintenance_change(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        session: Uuid,
        request: &BeginKnowledgeMaintenanceChange,
        definition: &KnowledgeChangeDefinition,
        registry: &KnowledgeProfileRegistry,
    ) -> Result<BeginKnowledgeMaintenanceChangeOutcome> {
        let tenant = self.tenant_id()?;
        knowledge_maintenance::begin_change(
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
}
