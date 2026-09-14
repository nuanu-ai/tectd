use crate::{knowledge_search, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::KnowledgeSearchStore;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
impl KnowledgeSearchStore for PgUnitOfWork {
    async fn knowledge_search_preflight(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        query: &KnowledgeSearchQuery,
        provider: Option<&KnowledgeEmbeddingModelIdentity>,
    ) -> Result<KnowledgeSearchPreflight> {
        let tenant = self.tenant_id()?;
        knowledge_search::preflight(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            query,
            provider,
        )
        .await
    }

    async fn knowledge_search(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        query: &KnowledgeSearchQuery,
        preflight: &KnowledgeSearchPreflight,
        embedding: Option<&KnowledgeQueryEmbedding>,
    ) -> Result<KnowledgeSearchResponse> {
        let tenant = self.tenant_id()?;
        knowledge_search::search(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            query,
            preflight,
            embedding,
        )
        .await
    }

    async fn claim_knowledge_search_jobs(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        limit: u32,
        model: &KnowledgeEmbeddingModelIdentity,
    ) -> Result<Vec<KnowledgeEmbeddingJobClaim>> {
        let tenant = self.tenant_id()?;
        knowledge_search::claim(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            limit,
            model,
        )
        .await
    }

    async fn complete_knowledge_search_job(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        completion: &KnowledgeEmbeddingJobCompletion,
    ) -> Result<bool> {
        let tenant = self.tenant_id()?;
        knowledge_search::complete(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            completion,
        )
        .await
    }

    async fn fail_knowledge_search_job(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        job: Uuid,
        lease: Uuid,
    ) -> Result<bool> {
        let tenant = self.tenant_id()?;
        knowledge_search::fail(
            self.transaction()?,
            tenant,
            workspace,
            principal,
            job,
            lease,
        )
        .await
    }

    async fn pending_knowledge_search_jobs(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        model: &KnowledgeEmbeddingModelIdentity,
    ) -> Result<u32> {
        let tenant = self.tenant_id()?;
        knowledge_search::pending(self.transaction()?, tenant, workspace, principal, model).await
    }
}
