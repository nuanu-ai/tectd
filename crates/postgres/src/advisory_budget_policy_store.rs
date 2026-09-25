use async_trait::async_trait;
use sqlx::Row;
use tect_application::AdvisoryBudgetPolicyStore;
use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy, Error, Result};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

#[async_trait]
impl AdvisoryBudgetPolicyStore for PgUnitOfWork {
    async fn candidate_budget_policy(
        &mut self,
        workspace_id: Uuid,
        now_unix_ms: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        if workspace_id.is_nil() || now_unix_ms < 0 {
            return Err(Error::InvalidArguments);
        }
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT id,version,digest,effective_from_unix_ms,effective_until_unix_ms,\
             provider_calls,input_tokens,output_tokens,request_utf8_bytes,elapsed_monotonic_ms,\
             retry_dispatches,approved_by,approval_signature FROM advisory_budget_policies \
             WHERE tenant_id=$1 AND workspace_id=$2 AND effective_from_unix_ms<=$3 \
             AND effective_until_unix_ms>$3 ORDER BY version DESC LIMIT 1",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(now_unix_ms)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else { return Ok(None) };
        let ceilings = AdvisoryBudgetCeilings {
            provider_calls: row.try_get("provider_calls").map_err(storage_error)?,
            input_tokens: row.try_get("input_tokens").map_err(storage_error)?,
            output_tokens: row.try_get("output_tokens").map_err(storage_error)?,
            request_utf8_bytes: row.try_get("request_utf8_bytes").map_err(storage_error)?,
            elapsed_monotonic_ms: row.try_get("elapsed_monotonic_ms").map_err(storage_error)?,
            retry_dispatches: row.try_get("retry_dispatches").map_err(storage_error)?,
        };
        AdvisoryBudgetPolicy::new(
            row.try_get("id").map_err(storage_error)?,
            row.try_get("version").map_err(storage_error)?,
            row.try_get("digest").map_err(storage_error)?,
            row.try_get("effective_from_unix_ms")
                .map_err(storage_error)?,
            row.try_get("effective_until_unix_ms")
                .map_err(storage_error)?,
            ceilings,
            row.try_get("approved_by").map_err(storage_error)?,
            row.try_get("approval_signature").map_err(storage_error)?,
        )
        .map(Some)
    }

    async fn install_budget_policy(
        &mut self,
        workspace_id: Uuid,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<()> {
        policy.validate()?;
        if workspace_id.is_nil() || !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        if self.principal_id()? != policy.approved_by() {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        let c = policy.ceilings();
        sqlx::query(
            "INSERT INTO advisory_budget_policies \
             (tenant_id,workspace_id,id,version,digest,effective_from_unix_ms,\
             effective_until_unix_ms,provider_calls,input_tokens,output_tokens,\
             request_utf8_bytes,elapsed_monotonic_ms,retry_dispatches,approved_by,approval_signature) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)"
        ).bind(tenant).bind(workspace_id).bind(policy.id()).bind(policy.version())
            .bind(policy.digest()).bind(policy.effective_from_unix_ms())
            .bind(policy.effective_until_unix_ms()).bind(c.provider_calls)
            .bind(c.input_tokens).bind(c.output_tokens).bind(c.request_utf8_bytes)
            .bind(c.elapsed_monotonic_ms).bind(c.retry_dispatches)
            .bind(policy.approved_by()).bind(policy.approval_signature())
            .execute(&mut **self.transaction()?).await.map_err(|error| {
                match error.as_database_error().and_then(|db| db.code()) {
                    Some(code) if code == "42501" => Error::Forbidden,
                    Some(code) if code == "23505" || code == "23514" || code == "23503" => Error::InputConflict,
                    _ => storage_error(error),
                }
            })?;
        Ok(())
    }
}
