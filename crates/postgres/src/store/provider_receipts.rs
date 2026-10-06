use super::*;

impl PgStore {
    async fn seal_matrix_raw_receipt(
        &self,
        continuation: &tect_application::AdvisoryDispatchContinuation,
        observation: &tect_application::AdvisoryProviderReceiptObservation,
        elapsed: i64,
    ) -> Result<tect_application::StoredAdvisoryProviderReceipt> {
        let mut tx = self
            .provider_receipt_transaction(continuation.tenant_id())
            .await?;
        let saved = crate::advisory::seal_provider_raw_observation(
            &mut tx,
            continuation,
            observation,
            elapsed,
        )
        .await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(saved)
    }

    async fn consume_matrix_raw_receipt(
        &self,
        continuation: &tect_application::AdvisoryDispatchContinuation,
        usage: tect_application::AdvisoryProviderReceiptUsage,
    ) -> Result<(
        tect_application::StoredAdvisoryProviderReceipt,
        AdvisoryBudgetConsumption,
    )> {
        let mut tx = self
            .provider_receipt_transaction(continuation.tenant_id())
            .await?;
        crate::advisory::seal_provider_observation_usage(&mut tx, continuation, usage).await?;
        tx.commit().await.map_err(storage_error)?;
        let mut tx = self
            .provider_receipt_transaction(continuation.tenant_id())
            .await?;
        let result = crate::advisory::consume_provider_observation(&mut tx, continuation).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(result)
    }
    async fn provider_receipt_transaction(
        &self,
        tenant: Uuid,
    ) -> Result<Transaction<'static, Postgres>> {
        if tenant.is_nil() {
            return Err(Error::InputConflict);
        }
        let mut tx = self.pool.begin().await.map_err(storage_error)?;
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .map_err(storage_error)?;
        Ok(tx)
    }
}

#[async_trait]
impl Store for PgStore {
    async fn seal_committed_advisory_observation(
        &self,
        continuation: &tect_application::AdvisoryDispatchContinuation,
        observation: &tect_application::AdvisoryProviderReceiptObservation,
        elapsed: i64,
    ) -> Result<tect_application::StoredAdvisoryProviderReceipt> {
        self.seal_matrix_raw_receipt(continuation, observation, elapsed)
            .await
    }

    async fn consume_committed_advisory_observation(
        &self,
        continuation: &tect_application::AdvisoryDispatchContinuation,
        usage: tect_application::AdvisoryProviderReceiptUsage,
    ) -> Result<(
        tect_application::StoredAdvisoryProviderReceipt,
        AdvisoryBudgetConsumption,
    )> {
        self.consume_matrix_raw_receipt(continuation, usage).await
    }

    async fn seal_committed_matrix_observation(
        &self,
        tenant: Uuid,
        continuation: &tect_application::MatrixDispatchContinuation,
        observation: &tect_application::MatrixProviderObservation,
        elapsed: i64,
    ) -> Result<tect_application::StoredMatrixDispatch> {
        let common = continuation.receipt_continuation();
        if common.tenant_id() != tenant {
            return Err(Error::InputConflict);
        }
        self.seal_matrix_raw_receipt(common, &observation.into(), elapsed)
            .await?;
        let mut tx = self.provider_receipt_transaction(tenant).await?;
        let saved = crate::advisory::matrix_receipt_view(&mut tx, common).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok(saved)
    }

    async fn consume_committed_matrix_observation(
        &self,
        tenant: Uuid,
        continuation: &tect_application::MatrixDispatchContinuation,
        usage: tect_application::MatrixProviderUsage,
    ) -> Result<(
        tect_application::StoredMatrixDispatch,
        AdvisoryBudgetConsumption,
    )> {
        let common = continuation.receipt_continuation();
        if common.tenant_id() != tenant {
            return Err(Error::InputConflict);
        }
        let (_, consumption) = self.consume_matrix_raw_receipt(common, usage).await?;
        let mut tx = self.provider_receipt_transaction(tenant).await?;
        let saved = crate::advisory::matrix_receipt_view(&mut tx, common).await?;
        tx.commit().await.map_err(storage_error)?;
        Ok((saved, consumption))
    }
    async fn seal_committed_model_route_observation(
        &self,
        tenant_id: Uuid,
        permit: &tect_application::ModelRouteSendPermit,
        observation: &tect_application::ModelRouteProviderObservation,
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
        let mut seal = PgUnitOfWork {
            transaction: Some(transaction),
            mode: TransactionMode::ReadWrite,
            identity: None,
            tenant_id: Some(tenant_id),
            budget_owner_keys: self.budget_owner_keys.clone(),
        };
        tect_application::ModelRouteAttemptStore::seal_observation(&mut seal, permit, observation)
            .await?;
        Box::new(seal).commit().await
    }
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
            budget_owner_keys: self.budget_owner_keys.clone(),
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
            budget_owner_keys: self.budget_owner_keys.clone(),
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
            budget_owner_keys: self.budget_owner_keys.clone(),
        };
        tect_application::ModelRouteAttemptStore::seal_raw_response(
            &mut seal,
            permit,
            raw,
            &tect_domain::model_route_wire_sha256(raw),
        )
        .await?;
        Box::new(seal).commit().await
    }

    async fn begin(&self, mode: TransactionMode) -> Result<Box<dyn UnitOfWork>> {
        let mut transaction = tect_application::request_diagnostics::measure(
            "pg.transaction_begin_including_acquire",
            self.pool.begin(),
        )
        .await
        .map_err(storage_error)?;
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
            budget_owner_keys: Arc::clone(&self.budget_owner_keys),
        }))
    }
}
