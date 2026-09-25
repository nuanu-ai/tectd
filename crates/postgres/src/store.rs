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
    async fn consume_committed_model_route_budget(
        &self,
        tenant_id: Uuid,
        permit: &tect_application::ModelRouteSendPermit,
        observation: &tect_application::ModelRouteProviderObservation,
    ) -> Result<bool> {
        if tenant_id.is_nil() || permit.attempt_id.is_nil() || permit.workspace_id.is_nil() {
            return Err(Error::InputConflict);
        }
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        let mut consume = PgUnitOfWork {
            transaction: Some(transaction),
            mode: TransactionMode::ReadWrite,
            identity: None,
            tenant_id: Some(tenant_id),
        };
        let exhausted = tect_application::ModelRouteAttemptStore::consume_budget(
            &mut consume,
            permit,
            observation,
        )
        .await?;
        Box::new(consume).commit().await?;
        Ok(exhausted)
    }

    async fn record_committed_model_route_failure(
        &self,
        tenant_id: Uuid,
        permit: &tect_application::ModelRouteSendPermit,
    ) -> Result<()> {
        if tenant_id.is_nil() || permit.attempt_id.is_nil() || permit.workspace_id.is_nil() {
            return Err(Error::InputConflict);
        }
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        let mut failed = PgUnitOfWork {
            transaction: Some(transaction),
            mode: TransactionMode::ReadWrite,
            identity: None,
            tenant_id: Some(tenant_id),
        };
        tect_application::ModelRouteAttemptStore::mark_send_unknown(&mut failed, permit).await?;
        Box::new(failed).commit().await
    }

    async fn seal_committed_model_route_response(
        &self,
        tenant_id: Uuid,
        permit: &tect_application::ModelRouteSendPermit,
        raw: &[u8],
    ) -> Result<()> {
        if tenant_id.is_nil()
            || permit.attempt_id.is_nil()
            || permit.workspace_id.is_nil()
            || permit.preparation_request_key.is_empty()
        {
            return Err(Error::InputConflict);
        }
        // This transaction deliberately carries no authenticated user identity.
        // It can only seal the exact committed attempt named by the server-held
        // permit. The normal authenticated path still owns parsing and decisions.
        let mut transaction = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(tenant_id.to_string())
            .execute(&mut *transaction)
            .await
            .map_err(storage_error)?;
        let mut seal = PgUnitOfWork {
            transaction: Some(transaction),
            mode: TransactionMode::ReadWrite,
            identity: None,
            tenant_id: Some(tenant_id),
        };
        tect_application::ModelRouteAttemptStore::seal_raw_response(
            &mut seal,
            permit,
            raw,
            &model_route_wire_sha256(raw),
        )
        .await?;
        Box::new(seal).commit().await
    }

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

mod unit_of_work;

async fn ensure_workspace_record(uow: &mut PgUnitOfWork, key: &str) -> Result<Created<Workspace>> {
    let tenant_id = uow.tenant_id()?;
    let inserted: Option<Uuid> = sqlx::query_scalar(
        "INSERT INTO workspaces (id, tenant_id, key) \
             VALUES (pg_catalog.gen_random_uuid(), $1, $2) \
             ON CONFLICT (tenant_id, key) DO NOTHING RETURNING id",
    )
    .bind(tenant_id)
    .bind(key)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    let (id, key): (Uuid, String) =
        sqlx::query_as("SELECT id, key FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(tenant_id)
            .bind(key)
            .fetch_one(&mut **uow.transaction()?)
            .await
            .map_err(storage_error)?;
    crate::durable_knowledge_store::ensure_workspace_state(uow, tenant_id, id).await?;
    Ok(Created {
        value: Workspace { id, key },
        created: inserted.is_some(),
    })
}
