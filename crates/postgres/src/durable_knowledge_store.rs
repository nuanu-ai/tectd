use crate::{durable_knowledge, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::DurableKnowledgeStore;
use tect_domain::*;
use uuid::Uuid;

pub(crate) async fn session_principal(store: &mut PgUnitOfWork, session_id: Uuid) -> Result<Uuid> {
    store.tenant_id()?;
    sqlx::query_scalar("SELECT tect_dk_session_principal($1)")
        .bind(session_id)
        .fetch_optional(&mut **store.transaction()?)
        .await
        .map_err(crate::storage_error)?
        .flatten()
        .ok_or(Error::Forbidden)
}

pub(crate) async fn ensure_workspace_state(
    store: &mut PgUnitOfWork,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<()> {
    sqlx::query("SELECT tect_dk_ensure_workspace_state($1,$2)")
        .bind(tenant)
        .bind(workspace)
        .execute(&mut **store.transaction()?)
        .await
        .map_err(crate::storage_error)?;
    Ok(())
}

#[async_trait]
impl DurableKnowledgeStore for PgUnitOfWork {
    async fn knowledge_owner(&mut self, principal_id: Uuid) -> Result<bool> {
        let tenant = self.tenant_id()?;
        let _ = tenant;
        sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
            .bind(principal_id)
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(crate::storage_error)
    }
    async fn knowledge_context(
        &mut self,
        workspace_id: Uuid,
        query: &KnowledgeContextQuery,
        preparation: &KnowledgeMethodSnapshot,
        review: &KnowledgeMethodSnapshot,
    ) -> Result<KnowledgeContext> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        durable_knowledge::context::context(
            self.transaction()?,
            tenant,
            workspace_id,
            query,
            preparation,
            review,
        )
        .await
    }
    async fn knowledge_change(
        &mut self,
        workspace_id: Uuid,
        change_id: Uuid,
    ) -> Result<Option<KnowledgeChange>> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        durable_knowledge::context::load_change(
            self.transaction()?,
            tenant,
            workspace_id,
            change_id,
        )
        .await
    }
    async fn prepare_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &PrepareKnowledgeChange,
        binding_provenance: Option<&KnowledgeBindingProvenance>,
        preparation: &KnowledgeMethodSnapshot,
        review: &KnowledgeMethodSnapshot,
    ) -> Result<PrepareKnowledgeChangeOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        durable_knowledge::change::prepare(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            session_id,
            request,
            binding_provenance,
            preparation,
            review,
        )
        .await
    }
    async fn review_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &ReviewKnowledgeChange,
    ) -> Result<ReviewKnowledgeChangeOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        durable_knowledge::change::review(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            session_id,
            request,
        )
        .await
    }
    async fn publish_knowledge_change(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &PublishKnowledgeChange,
    ) -> Result<PublishKnowledgeChangeOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        durable_knowledge::publish::publish(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            session_id,
            request,
        )
        .await
    }
    async fn refresh_pipeline_knowledge(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RefreshPipelineKnowledge,
    ) -> Result<RefreshPipelineKnowledgeOutcome> {
        let tenant = self.tenant_id()?;
        durable_knowledge::require_identity_ready(self.transaction()?).await?;
        durable_knowledge::manifest::refresh(
            self.transaction()?,
            tenant,
            workspace_id,
            session_id,
            request,
        )
        .await
    }
}
