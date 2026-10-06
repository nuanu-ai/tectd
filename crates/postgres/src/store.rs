use crate::{BudgetOwnerKeys, programs, runtime, sources, storage_error};
use async_trait::async_trait;
use sqlx::postgres::PgPoolOptions;
use sqlx::{PgPool, Postgres, Transaction};
use std::sync::Arc;
use tect_application::{Store, TransactionMode, UnitOfWork};
use tect_domain::*;
use uuid::Uuid;

#[derive(Clone)]
pub struct PgStore {
    pool: PgPool,
    budget_owner_keys: Arc<BudgetOwnerKeys>,
}

mod connection;
mod unit_of_work;

mod role;
use role::decode_principal_role;

pub(crate) struct PgUnitOfWork {
    transaction: Option<Transaction<'static, Postgres>>,
    mode: TransactionMode,
    identity: Option<HostIdentity>,
    tenant_id: Option<Uuid>,
    budget_owner_keys: Arc<BudgetOwnerKeys>,
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
            budget_owner_keys: Arc::new(BudgetOwnerKeys::default()),
        }
    }

    pub(crate) fn is_read_write(&self) -> bool {
        self.mode == TransactionMode::ReadWrite
    }

    pub(crate) fn is_owner(&self) -> bool {
        self.identity
            .as_ref()
            .is_some_and(|identity| identity.role == PrincipalRole::Owner)
    }

    pub(crate) async fn abort_matrix_lock_contention(&mut self) -> Result<()> {
        // Invalidate before rollback: retained context failures cannot commit
        // earlier writes or convert lock contention into a saved NoCall.
        self.transaction
            .take()
            .ok_or(Error::StorageUnavailable)?
            .rollback()
            .await
            .map_err(storage_error)
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
}

mod provider_receipts;

#[cfg(test)]
mod matrix_aborted_uow_tests {
    use super::*;

    #[tokio::test]
    async fn an_aborted_matrix_uow_cannot_resume_or_commit() {
        // This is the state left after abort_matrix_lock_contention takes the
        // transaction. The PostgreSQL rollback itself requires live tests.
        let mut uow = PgUnitOfWork {
            transaction: None,
            mode: TransactionMode::ReadWrite,
            identity: None,
            tenant_id: Some(Uuid::new_v4()),
            budget_owner_keys: Arc::new(BudgetOwnerKeys::default()),
        };
        assert!(matches!(uow.transaction(), Err(Error::StorageUnavailable)));
        assert!(matches!(
            uow.abort_matrix_lock_contention().await,
            Err(Error::StorageUnavailable)
        ));
        assert!(matches!(
            Box::new(uow).commit().await,
            Err(Error::StorageUnavailable)
        ));
    }
}
