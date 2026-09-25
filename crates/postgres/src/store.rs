use crate::{programs, runtime, sources, storage_error};
use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};
use tect_application::{Store, TransactionMode, UnitOfWork};
use tect_domain::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
}

mod connection;

pub(crate) struct PgUnitOfWork {
    transaction: Option<Transaction<'static, Postgres>>,
    mode: TransactionMode,
    identity: Option<HostIdentity>,
    tenant_id: Option<Uuid>,
}

impl PgUnitOfWork {
    #[cfg(test)]
    pub(crate) async fn test_begin(pool: &PgPool, tenant_id: Uuid) -> Self {
        let mut transaction = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *transaction)
            .await
            .unwrap();
        Self {
            transaction: Some(transaction),
            mode: TransactionMode::ReadWrite,
            identity: None,
            tenant_id: Some(tenant_id),
        }
    }

    pub(crate) fn transaction(&mut self) -> Result<&mut Transaction<'static, Postgres>> {
        self.transaction.as_mut().ok_or(Error::StorageUnavailable)
    }

    pub(crate) fn tenant_id(&self) -> Result<Uuid> {
        self.tenant_id.ok_or(Error::Forbidden)
    }

    pub(crate) fn principal_id(&self) -> Result<Uuid> {
        self.identity
            .as_ref()
            .map(|identity| identity.principal_id)
            .ok_or(Error::Forbidden)
    }

    pub(crate) fn is_read_write(&self) -> bool {
        self.mode == TransactionMode::ReadWrite
    }
}

#[async_trait]
impl Store for PgStore {
    async fn begin(&self, mode: TransactionMode) -> Result<Box<dyn UnitOfWork>> {
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        if mode == TransactionMode::ReadOnly {
            sqlx::query("SET TRANSACTION ISOLATION LEVEL REPEATABLE READ, READ ONLY")
                .execute(&mut *transaction)
                .await
                .map_err(storage_error)?;
        }
        Ok(Box::new(PgUnitOfWork {
            transaction: Some(transaction),
            mode,
            identity: None,
            tenant_id: None,
        }))
    }
}

#[async_trait]
impl UnitOfWork for PgUnitOfWork {
    fn pipeline_recommendation_dispatch_store(
        &mut self,
    ) -> Option<&mut dyn tect_application::PipelineRecommendationDispatchStore> {
        Some(self)
    }

    fn pipeline_recommendation_store(
        &mut self,
    ) -> Option<&mut dyn tect_application::PipelineRecommendationStore> {
        Some(self)
    }

    fn matrix_planning_effect_store(
        &mut self,
    ) -> Option<&mut dyn tect_application::MatrixPlanningEffectStore> {
        Some(self)
    }
    fn pipeline_open_effect_store(&mut self) -> Option<&mut dyn tect_application::PipelineOpenEffectStore> {
        Some(self)
    }
    fn pipeline_phase_effect_store(&mut self) -> Option<&mut dyn tect_application::PipelinePhaseEffectStore> {
        Some(self)
    }

    fn matrix_planning_selection_store(
        &mut self,
    ) -> Option<&mut dyn tect_application::MatrixPlanningSelectionStore> {
        Some(self)
    }

    fn matrix_verification_store(
        &mut self,
    ) -> Option<&mut dyn tect_application::MatrixVerificationStore> {
        Some(self)
    }

    async fn authenticate(&mut self, auth: &HostAuth) -> Result<HostIdentity> {
        let digest = runtime::credential_digest(&auth.credential);
        let for_write = self.mode == TransactionMode::ReadWrite;
        let row: Option<(Uuid, Uuid, String, serde_json::Value, serde_json::Value)> = sqlx::query_as(
            "SELECT tenant_id, principal_id, principal_role, allowed_source_roots, allowed_setup_roots \
             FROM public.tect_authenticate_host($1, $2, $3)",
        )
        .bind(auth.host_id)
        .bind(digest)
        .bind(for_write)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let (tenant_id, principal_id, role, source_roots, setup_roots) =
            row.ok_or(Error::Unauthorized)?;
        let role = match role.as_str() {
            "owner" => PrincipalRole::Owner,
            "verifier" => PrincipalRole::Verifier,
            _ => return Err(Error::Unauthorized),
        };
        let allowed_source_roots = serde_json::from_value(source_roots).map_err(storage_error)?;
        let allowed_setup_roots = serde_json::from_value(setup_roots).map_err(storage_error)?;
        let identity = HostIdentity {
            host_id: auth.host_id,
            tenant_id,
            principal_id,
            role,
            allowed_source_roots,
            allowed_setup_roots,
        };
        self.identity = Some(identity.clone());
        Ok(identity)
    }

    async fn set_tenant(&mut self, tenant_id: Uuid) -> Result<()> {
        let identity = self.identity.as_ref().ok_or(Error::Unauthorized)?;
        if identity.tenant_id != tenant_id {
            return Err(Error::Forbidden);
        }
        let value = tenant_id.to_string();
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(value)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        self.tenant_id = Some(tenant_id);
        Ok(())
    }

    async fn lock_native_session(&mut self, host_id: Uuid, native_id: &str) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        let identity = self.identity.as_ref().ok_or(Error::Unauthorized)?;
        if self.mode != TransactionMode::ReadWrite
            || identity.tenant_id != tenant_id
            || identity.host_id != host_id
        {
            return Err(Error::Forbidden);
        }
        sqlx::query(
            "SELECT pg_catalog.pg_advisory_xact_lock(\
                 pg_catalog.hashtextextended($1::text || ':' || $2, 0))",
        )
        .bind(host_id)
        .bind(native_id)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn session(&mut self, host_id: Uuid, native_id: &str) -> Result<Option<Session>> {
        let tenant_id = self.tenant_id()?;
        let row: Option<(Uuid, Uuid, Uuid, String, bool)> = sqlx::query_as(
            "SELECT id, workspace_id, host_id, native_session_id, revoked \
             FROM agent_sessions \
             WHERE tenant_id=$1 AND host_id=$2 AND native_session_id=$3",
        )
        .bind(tenant_id)
        .bind(host_id)
        .bind(native_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(row.map(
            |(id, workspace_id, host_id, native_session_id, revoked)| Session {
                id,
                workspace_id,
                host_id,
                native_session_id,
                revoked,
            },
        ))
    }

    async fn workspace(&mut self, id: Uuid) -> Result<Option<Workspace>> {
        let tenant_id = self.tenant_id()?;
        let row: Option<(Uuid, String)> =
            sqlx::query_as("SELECT id, key FROM workspaces WHERE tenant_id=$1 AND id=$2")
                .bind(tenant_id)
                .bind(id)
                .fetch_optional(&mut **self.transaction()?)
                .await
                .map_err(storage_error)?;
        Ok(row.map(|(id, key)| Workspace { id, key }))
    }

    async fn workspace_by_key(&mut self, key: &str) -> Result<Option<Workspace>> {
        let tenant_id = self.tenant_id()?;
        let row: Option<(Uuid, String)> =
            sqlx::query_as("SELECT id, key FROM workspaces WHERE tenant_id=$1 AND key=$2")
                .bind(tenant_id)
                .bind(key)
                .fetch_optional(&mut **self.transaction()?)
                .await
                .map_err(storage_error)?;
        Ok(row.map(|(id, key)| Workspace { id, key }))
    }

    async fn is_member(&mut self, workspace_id: Uuid, principal_id: Uuid) -> Result<bool> {
        let tenant_id = self.tenant_id()?;
        sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM memberships \
             WHERE tenant_id=$1 AND workspace_id=$2 AND principal_id=$3)",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(principal_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)
    }

    async fn session_principal(&mut self, session_id: Uuid) -> Result<Uuid> {
        crate::durable_knowledge_store::session_principal(self, session_id).await
    }
    async fn ensure_workspace(&mut self, key: &str) -> Result<Created<Workspace>> {
        let tenant_id = self.tenant_id()?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO workspaces (id, tenant_id, key) \
             VALUES (pg_catalog.gen_random_uuid(), $1, $2) \
             ON CONFLICT (tenant_id, key) DO NOTHING RETURNING id",
        )
        .bind(tenant_id)
        .bind(key)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let (id, key): (Uuid, String) =
            sqlx::query_as("SELECT id, key FROM workspaces WHERE tenant_id=$1 AND key=$2")
                .bind(tenant_id)
                .bind(key)
                .fetch_one(&mut **self.transaction()?)
                .await
                .map_err(storage_error)?;
        crate::durable_knowledge_store::ensure_workspace_state(self, tenant_id, id).await?;
        Ok(Created {
            value: Workspace { id, key },
            created: inserted.is_some(),
        })
    }

    async fn ensure_membership(&mut self, workspace_id: Uuid, principal_id: Uuid) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        sqlx::query(
            "INSERT INTO memberships (tenant_id, workspace_id, principal_id) \
             VALUES ($1, $2, $3) ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(principal_id)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn ensure_session(
        &mut self,
        host_id: Uuid,
        workspace_id: Uuid,
        native_id: &str,
    ) -> Result<Created<Session>> {
        let tenant_id = self.tenant_id()?;
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO agent_sessions \
                 (id, tenant_id, host_id, workspace_id, native_session_id) \
             VALUES (pg_catalog.gen_random_uuid(), $1, $2, $3, $4) \
             ON CONFLICT (host_id, native_session_id) DO NOTHING RETURNING id",
        )
        .bind(tenant_id)
        .bind(host_id)
        .bind(workspace_id)
        .bind(native_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let row: (Uuid, Uuid, Uuid, String, bool) = sqlx::query_as(
            "SELECT id, workspace_id, host_id, native_session_id, revoked \
             FROM agent_sessions \
             WHERE tenant_id=$1 AND host_id=$2 AND native_session_id=$3",
        )
        .bind(tenant_id)
        .bind(host_id)
        .bind(native_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(Created {
            value: Session {
                id: row.0,
                workspace_id: row.1,
                host_id: row.2,
                native_session_id: row.3,
                revoked: row.4,
            },
            created: inserted.is_some(),
        })
    }

    async fn append_creation_event(
        &mut self,
        workspace_id: Uuid,
        kind: EventKind,
        entity_id: Uuid,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        sqlx::query(
            "INSERT INTO workspace_events (id, tenant_id, workspace_id, kind, entity_id) \
             VALUES (pg_catalog.gen_random_uuid(), $1, $2, $3, $4) \
             ON CONFLICT (kind, entity_id) DO NOTHING",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(kind.as_str())
        .bind(entity_id)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn register_source(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        location: &SourceLocation,
    ) -> Result<RegisteredSource> {
        let tenant_id = self.tenant_id()?;
        sources::register_source(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            location,
        )
        .await
    }

    async fn source_worktrees(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        ids: &[Uuid],
    ) -> Result<Vec<WorktreeSummary>> {
        let tenant_id = self.tenant_id()?;
        sources::source_worktrees(self.transaction()?, tenant_id, workspace_id, host_id, ids).await
    }

    async fn replace_selection(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
        ids: &[Uuid],
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        sources::replace_selection(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            session_id,
            ids,
        )
        .await
    }

    async fn selected_worktrees(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        session_id: Uuid,
    ) -> Result<Vec<WorktreeSummary>> {
        let tenant_id = self.tenant_id()?;
        sources::selected_worktrees(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            session_id,
        )
        .await
    }

    async fn list_sources(
        &mut self,
        workspace_id: Uuid,
        host_id: Uuid,
        after: Option<Uuid>,
        limit: u32,
    ) -> Result<Vec<RegisteredSource>> {
        let tenant_id = self.tenant_id()?;
        sources::list_sources(
            self.transaction()?,
            tenant_id,
            workspace_id,
            host_id,
            after,
            limit,
        )
        .await
    }

    async fn ensure_program(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        input: &NewProgramInput,
    ) -> Result<Program> {
        let tenant_id = self.tenant_id()?;
        programs::ensure_program(
            self.transaction()?,
            tenant_id,
            workspace_id,
            session_id,
            input,
        )
        .await
    }

    async fn program(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        for_update: bool,
    ) -> Result<Option<Program>> {
        let tenant_id = self.tenant_id()?;
        programs::program(
            self.transaction()?,
            tenant_id,
            workspace_id,
            program_id,
            for_update,
        )
        .await
    }

    async fn program_input(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<ProgramInput>> {
        let tenant_id = self.tenant_id()?;
        programs::program_input(
            self.transaction()?,
            tenant_id,
            workspace_id,
            program_id,
            request_id,
        )
        .await
    }

    async fn insert_program_input(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        session_id: Uuid,
        sequence: i64,
        input: &NewProgramInput,
    ) -> Result<ProgramInput> {
        let tenant_id = self.tenant_id()?;
        programs::insert_program_input(
            self.transaction()?,
            tenant_id,
            workspace_id,
            program_id,
            session_id,
            sequence,
            input,
        )
        .await
    }

    async fn update_program(&mut self, program: &Program) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        programs::update_program(self.transaction()?, tenant_id, program).await
    }

    async fn program_inputs(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<ProgramInput>> {
        let tenant_id = self.tenant_id()?;
        programs::program_inputs(
            self.transaction()?,
            tenant_id,
            workspace_id,
            program_id,
            after,
            limit,
        )
        .await
    }

    async fn list_programs(
        &mut self,
        workspace_id: Uuid,
        after: Option<ProgramCursor>,
        limit: u32,
    ) -> Result<Vec<ProgramSummary>> {
        let tenant_id = self.tenant_id()?;
        programs::list_programs(self.transaction()?, tenant_id, workspace_id, after, limit).await
    }

    async fn commit(mut self: Box<Self>) -> Result<()> {
        self.transaction
            .take()
            .ok_or(Error::StorageUnavailable)?
            .commit()
            .await
            .map_err(storage_error)
    }
}
