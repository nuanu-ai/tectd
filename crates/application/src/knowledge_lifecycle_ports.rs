use async_trait::async_trait;
use tect_domain::*;
use uuid::Uuid;

pub trait KnowledgeLifecycleDefinitionProvider: Send + Sync {
    fn definition(&self) -> Result<KnowledgeChangeDefinition>;
    fn registry(&self) -> Result<KnowledgeProfileRegistry>;
}

pub trait KnowledgeOutputGuard: Send + Sync {
    fn lifecycle(&self, value: &KnowledgeLifecycleResponse) -> Result<()>;
    fn unit(&self, value: &KnowledgeUnitResponse) -> Result<()>;
    fn begin(&self, value: &BeginKnowledgeChangeOutcome) -> Result<()>;
    fn mutation(&self, value: &KnowledgeChangeMutationOutcome) -> Result<()>;
    fn commit(&self, value: &CommitKnowledgeChangeOutcome) -> Result<()>;
    fn effects(&self, value: &SettleKnowledgeChangeEffectsOutcome) -> Result<()>;
}

#[async_trait]
pub trait KnowledgeLifecycleStore: Send {
    async fn knowledge_lifecycle(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        query: &KnowledgeLifecycleQuery,
    ) -> Result<KnowledgeLifecycleResponse>;

    async fn knowledge_unit(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        query: &KnowledgeUnitQuery,
    ) -> Result<Option<KnowledgeUnitResponse>>;

    #[allow(clippy::too_many_arguments)]
    async fn begin_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &BeginKnowledgeChange,
        definition: &KnowledgeChangeDefinition,
        registry: &KnowledgeProfileRegistry,
    ) -> Result<BeginKnowledgeChangeOutcome>;

    async fn complete_knowledge_change_phase(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &CompleteKnowledgeChangePhase,
    ) -> Result<KnowledgeChangeMutationOutcome>;

    async fn record_knowledge_change_input(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &RecordKnowledgeChangeInput,
    ) -> Result<KnowledgeChangeMutationOutcome>;

    async fn commit_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &CommitKnowledgeChange,
    ) -> Result<CommitKnowledgeChangeOutcome>;

    async fn settle_knowledge_change_effects(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &SettleKnowledgeChangeEffects,
    ) -> Result<SettleKnowledgeChangeEffectsOutcome>;
}
