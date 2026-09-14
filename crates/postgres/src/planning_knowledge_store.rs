use crate::{planning_knowledge, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::PlanningKnowledgeStore;
use tect_domain::*;
use uuid::Uuid;

#[async_trait]
impl PlanningKnowledgeStore for PgUnitOfWork {
    async fn capture_planning_knowledge(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        stage: PlanningStage,
        owner_id: Uuid,
        owner_revision: i64,
        input_revision: i64,
        request_id: Uuid,
        program_id: Option<Uuid>,
        scope_id: Option<Uuid>,
        task_context: Option<&PlanningTaskContext>,
        method: &PlanningMethodSnapshot,
    ) -> Result<PlanningKnowledgeManifest> {
        let tenant = self.tenant_id()?;
        planning_knowledge::capture(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            stage,
            owner_id,
            owner_revision,
            input_revision,
            request_id,
            program_id,
            scope_id,
            task_context,
            method,
        )
        .await
    }
    async fn planning_knowledge_status(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        stage: PlanningStage,
        owner_id: Uuid,
    ) -> Result<PlanningKnowledgeStatus> {
        let tenant = self.tenant_id()?;
        planning_knowledge::status(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            stage,
            owner_id,
        )
        .await
    }
    async fn planning_manifest_status(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        manifest: PlanningKnowledgeManifest,
    ) -> Result<PlanningKnowledgeStatus> {
        let tenant = self.tenant_id()?;
        planning_knowledge::status_for_manifest(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            manifest,
        )
        .await
    }
    async fn require_planning_knowledge(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        stage: PlanningStage,
        owner_id: Uuid,
        guard: Option<&PlanningManifestGuard>,
    ) -> Result<Option<PlanningKnowledgeManifest>> {
        let tenant = self.tenant_id()?;
        planning_knowledge::require(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            stage,
            owner_id,
            guard,
        )
        .await
    }
    async fn register_planning_consumption(
        &mut self,
        workspace_id: Uuid,
        manifest_id: Uuid,
        relation_name: &str,
        row_id: Uuid,
        row_revision: i64,
    ) -> Result<()> {
        let tenant = self.tenant_id()?;
        planning_knowledge::register_consumption(
            self.transaction()?,
            tenant,
            workspace_id,
            manifest_id,
            relation_name,
            row_id,
            row_revision,
        )
        .await
    }
    async fn program_knowledge_refresh_replay(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        request: &RefreshProgramKnowledge,
    ) -> Result<Option<Program>> {
        let tenant = self.tenant_id()?;
        planning_knowledge::program_refresh_replay(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            request,
        )
        .await
    }
    async fn save_program_knowledge_refresh_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &RefreshProgramKnowledge,
        program: &Program,
    ) -> Result<()> {
        let tenant = self.tenant_id()?;
        planning_knowledge::save_program_refresh_receipt(
            self.transaction()?,
            tenant,
            workspace_id,
            request,
            program,
        )
        .await
    }
    async fn planning_consumption_status(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        relation_name: &str,
        row_id: Uuid,
        row_revision: i64,
    ) -> Result<Option<PlanningKnowledgeStatus>> {
        let tenant = self.tenant_id()?;
        planning_knowledge::consumption_status(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            relation_name,
            row_id,
            row_revision,
        )
        .await
    }
    async fn register_planning_receipt_copy(
        &mut self,
        workspace_id: Uuid,
        manifest_id: Uuid,
        relation_name: &str,
        row_id: Uuid,
        row_revision: i64,
        operation: &str,
        request_id: Uuid,
    ) -> Result<()> {
        let tenant = self.tenant_id()?;
        planning_knowledge::register_receipt_copy(
            self.transaction()?,
            tenant,
            workspace_id,
            manifest_id,
            relation_name,
            row_id,
            row_revision,
            operation,
            request_id,
        )
        .await
    }
}
