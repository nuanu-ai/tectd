use async_trait::async_trait;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
pub trait KnowledgeEmbeddingProvider: Send + Sync {
    fn model(&self) -> Option<KnowledgeEmbeddingModelIdentity>;
    async fn embed(&self, request: &KnowledgeEmbeddingRequest) -> Result<Vec<f32>>;
}

#[derive(Debug, Default)]
pub struct DisabledKnowledgeEmbeddingProvider;

#[async_trait]
impl KnowledgeEmbeddingProvider for DisabledKnowledgeEmbeddingProvider {
    fn model(&self) -> Option<KnowledgeEmbeddingModelIdentity> {
        None
    }

    async fn embed(&self, _: &KnowledgeEmbeddingRequest) -> Result<Vec<f32>> {
        Err(Error::KnowledgeUnavailable)
    }
}

pub trait KnowledgeSearchOutputGuard: Send + Sync {
    fn check(&self, response: &KnowledgeSearchResponse) -> Result<()>;
}

#[async_trait]
pub trait KnowledgeSearchStore: Send {
    async fn knowledge_search_preflight(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        query: &KnowledgeSearchQuery,
        provider: Option<&KnowledgeEmbeddingModelIdentity>,
    ) -> Result<KnowledgeSearchPreflight>;

    async fn knowledge_search(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        query: &KnowledgeSearchQuery,
        preflight: &KnowledgeSearchPreflight,
        embedding: Option<&KnowledgeQueryEmbedding>,
    ) -> Result<KnowledgeSearchResponse>;

    async fn claim_knowledge_search_jobs(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        limit: u32,
        model: &KnowledgeEmbeddingModelIdentity,
    ) -> Result<Vec<KnowledgeEmbeddingJobClaim>>;

    async fn complete_knowledge_search_job(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        completion: &KnowledgeEmbeddingJobCompletion,
    ) -> Result<bool>;

    async fn fail_knowledge_search_job(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        job_id: Uuid,
        lease_token: Uuid,
    ) -> Result<bool>;

    async fn pending_knowledge_search_jobs(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        model: &KnowledgeEmbeddingModelIdentity,
    ) -> Result<u32>;
}
