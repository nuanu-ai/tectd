use async_trait::async_trait;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
pub trait DurableKnowledgeStore: Send {
    async fn knowledge_owner(&mut self, principal_id: Uuid) -> Result<bool>;
    async fn knowledge_context(
        &mut self,
        workspace_id: Uuid,
        query: &KnowledgeContextQuery,
        preparation: &KnowledgeMethodSnapshot,
        review: &KnowledgeMethodSnapshot,
    ) -> Result<KnowledgeContext>;
    async fn knowledge_change(
        &mut self,
        workspace_id: Uuid,
        change_id: Uuid,
    ) -> Result<Option<KnowledgeChange>>;
    #[allow(clippy::too_many_arguments)]
    async fn prepare_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &PrepareKnowledgeChange,
        binding_provenance: Option<&KnowledgeBindingProvenance>,
        preparation: &KnowledgeMethodSnapshot,
        review: &KnowledgeMethodSnapshot,
    ) -> Result<PrepareKnowledgeChangeOutcome>;
    async fn review_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &ReviewKnowledgeChange,
    ) -> Result<ReviewKnowledgeChangeOutcome>;
    async fn publish_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &PublishKnowledgeChange,
    ) -> Result<PublishKnowledgeChangeOutcome>;
    async fn refresh_pipeline_knowledge(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RefreshPipelineKnowledge,
    ) -> Result<RefreshPipelineKnowledgeOutcome>;
}
