use async_trait::async_trait;
use tect_domain::*;
use uuid::Uuid;

pub trait KnowledgeMaintenanceOutputGuard: Send + Sync {
    fn observe(&self, value: &ObserveKnowledgeMaintenanceOutcome) -> Result<()>;
    fn begin(&self, value: &BeginKnowledgeMaintenanceChangeOutcome) -> Result<()>;
}

#[async_trait]
pub trait KnowledgeMaintenanceStore: Send {
    async fn knowledge_maintenance(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        query: &KnowledgeMaintenanceQuery,
        method: &PipelineInstructionSnapshot,
    ) -> Result<KnowledgeMaintenanceContext>;

    async fn observe_knowledge_maintenance(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &ObserveKnowledgeMaintenanceSignal,
    ) -> Result<ObserveKnowledgeMaintenanceOutcome>;

    async fn sweep_due_knowledge_maintenance(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        limit: u32,
    ) -> Result<u32>;

    async fn claim_knowledge_maintenance_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
    ) -> Result<KnowledgeMaintenanceClaimOutcome>;

    async fn prepare_knowledge_maintenance_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        claim: &KnowledgeMaintenanceJobClaim,
    ) -> Result<KnowledgeMaintenancePrepareOutcome>;

    async fn fail_knowledge_maintenance_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        claim: &KnowledgeMaintenanceJobClaim,
        code: KnowledgeMaintenanceFailureCode,
    ) -> Result<KnowledgeMaintenanceFailureOutcome>;

    async fn pending_knowledge_maintenance_tasks(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
    ) -> Result<u32>;

    #[allow(clippy::too_many_arguments)]
    async fn begin_knowledge_maintenance_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &BeginKnowledgeMaintenanceChange,
        definition: &KnowledgeChangeDefinition,
        registry: &KnowledgeProfileRegistry,
    ) -> Result<BeginKnowledgeMaintenanceChangeOutcome>;
}
