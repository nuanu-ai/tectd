use async_trait::async_trait;
use tect_domain::{
    PlanningKnowledgeManifest, PlanningKnowledgeStatus, PlanningManifestGuard, PlanningStage,
    PlanningTaskContext, Result,
};
use uuid::Uuid;

#[async_trait]
pub trait PlanningKnowledgeStore: Send {
    #[allow(clippy::too_many_arguments)]
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
        method: &tect_domain::PlanningMethodSnapshot,
    ) -> Result<PlanningKnowledgeManifest>;

    async fn planning_knowledge_status(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        stage: PlanningStage,
        owner_id: Uuid,
    ) -> Result<PlanningKnowledgeStatus>;

    async fn planning_manifest_status(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        manifest: PlanningKnowledgeManifest,
    ) -> Result<PlanningKnowledgeStatus>;

    async fn require_planning_knowledge(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        stage: PlanningStage,
        owner_id: Uuid,
        guard: Option<&PlanningManifestGuard>,
    ) -> Result<Option<PlanningKnowledgeManifest>>;

    async fn register_planning_consumption(
        &mut self,
        workspace_id: Uuid,
        manifest_id: Uuid,
        relation_name: &str,
        row_id: Uuid,
        row_revision: i64,
    ) -> Result<()>;

    async fn program_knowledge_refresh_replay(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        request: &tect_domain::RefreshProgramKnowledge,
    ) -> Result<Option<tect_domain::Program>>;

    async fn save_program_knowledge_refresh_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &tect_domain::RefreshProgramKnowledge,
        program: &tect_domain::Program,
    ) -> Result<()>;

    async fn planning_consumption_status(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        relation_name: &str,
        row_id: Uuid,
        row_revision: i64,
    ) -> Result<Option<PlanningKnowledgeStatus>>;

    #[allow(clippy::too_many_arguments)]
    async fn register_planning_receipt_copy(
        &mut self,
        workspace_id: Uuid,
        manifest_id: Uuid,
        relation_name: &str,
        row_id: Uuid,
        row_revision: i64,
        operation: &str,
        request_id: Uuid,
    ) -> Result<()>;
}
