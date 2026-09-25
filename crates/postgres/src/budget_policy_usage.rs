//! Exact-policy workspace ledger shared by advisory dispatches and Anti-Bloat.
//! Reservation calls acquire the workspace policy lock after their route lock.
use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{AdvisoryBudgetPolicy, Error, Result};
use uuid::Uuid;

pub(crate) struct PolicyUsage {
    pub calls: i64,
    pub request_bytes: i64,
    pub retries: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub elapsed_ms: i64,
    pub pending: i64,
    pub invalid: i64,
}

/// Serialize installation with the active-policy choice made by reservations.
/// The lock is held until commit and must precede both the highest-active
/// policy SELECT and the selected policy row lock.
pub(crate) async fn lock_workspace_policy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
) -> Result<()> {
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(pg_catalog.hashtextextended(\
         'tect-budget-policy:' || $1::text || ':' || $2::text, 0))",
    )
    .bind(tenant)
    .bind(workspace)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(())
}

/// Serializes every route on the same immutable policy row. A pending attempt
/// holds the entire remaining token envelope, so no sibling may start yet.
pub(crate) async fn policy_usage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    policy_id: Uuid,
    version: i64,
    digest: &str,
) -> Result<PolicyUsage> {
    let locked: Option<(i64, String)> = sqlx::query_as(
        "SELECT version,digest FROM advisory_budget_policies WHERE tenant_id=$1 \
         AND workspace_id=$2 AND id=$3 FOR UPDATE",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(policy_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if locked
        .as_ref()
        .is_none_or(|row| row.0 != version || row.1 != digest)
    {
        return Err(Error::BudgetPolicyInvalid);
    }
    let totals: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        r#"SELECT COUNT(*)::bigint,COALESCE(SUM(request_bytes),0)::bigint,
         COALESCE(SUM(retries),0)::bigint,COALESCE(SUM(input_tokens),0)::bigint,
         COALESCE(SUM(output_tokens),0)::bigint,COALESCE(SUM(elapsed_ms),0)::bigint,
         COUNT(*) FILTER (WHERE NOT consumed)::bigint,
         COUNT(*) FILTER (WHERE unknown_usage OR exhausted)::bigint FROM (
         SELECT r.request_utf8_bytes AS request_bytes,r.reserved_retry_dispatches AS retries,
           c.input_tokens,c.output_tokens,c.monotonic_elapsed_ms AS elapsed_ms,
           c.dispatch_id IS NOT NULL AS consumed,COALESCE(c.unknown_usage,false) AS unknown_usage,
           COALESCE(c.exhausted_after_response,false) AS exhausted
         FROM advisory_budget_reservations r LEFT JOIN advisory_budget_consumptions c
           ON (c.tenant_id,c.workspace_id,c.dispatch_id)=(r.tenant_id,r.workspace_id,r.dispatch_id)
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.policy_id=$3
           AND r.policy_version=$4 AND r.policy_digest=$5
         UNION ALL
         SELECT r.request_utf8_bytes,0,c.input_tokens,c.output_tokens,c.elapsed_monotonic_ms,
           c.review_id IS NOT NULL,COALESCE(c.unknown_usage,false),
           COALESCE(c.exhausted_after_response,false)
         FROM scope_anti_bloat_budget_reservations r
         LEFT JOIN scope_anti_bloat_budget_consumptions c
           ON (c.tenant_id,c.workspace_id,c.review_id)=(r.tenant_id,r.workspace_id,r.review_id)
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.policy_id=$3
           AND r.policy_version=$4 AND r.policy_digest=$5
         UNION ALL
         SELECT r.request_utf8_bytes,0,c.input_tokens,c.output_tokens,c.elapsed_monotonic_ms,
           c.attempt_id IS NOT NULL,COALESCE(c.unknown_usage,false),
           COALESCE(c.exhausted_after_response,false)
         FROM model_route_budget_reservations r
         LEFT JOIN model_route_budget_consumptions c
           ON (c.tenant_id,c.workspace_id,c.attempt_id)=(r.tenant_id,r.workspace_id,r.attempt_id)
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.policy_id=$3
           AND r.policy_version=$4 AND r.policy_digest=$5) attempts"#,
    )
    .bind(tenant)
    .bind(workspace)
    .bind(policy_id)
    .bind(version)
    .bind(digest)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(PolicyUsage {
        calls: totals.0,
        request_bytes: totals.1,
        retries: totals.2,
        input_tokens: totals.3,
        output_tokens: totals.4,
        elapsed_ms: totals.5,
        pending: totals.6,
        invalid: totals.7,
    })
}

/// Recheck the winner after the route's own lock and the policy ledger lock.
/// A previously authorized policy may have been superseded before reservation.
pub(crate) async fn require_current_policy(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    policy: &AdvisoryBudgetPolicy,
) -> Result<()> {
    let current: Option<(Uuid, i64, String, i64, i64)> = sqlx::query_as(
        "WITH instant AS MATERIALIZED (SELECT \
         (EXTRACT(EPOCH FROM pg_catalog.clock_timestamp())*1000)::bigint AS now) \
         SELECT id,version,digest,effective_from_unix_ms,effective_until_unix_ms \
         FROM advisory_budget_policies CROSS JOIN instant \
         WHERE tenant_id=$1 AND workspace_id=$2 \
           AND effective_from_unix_ms <= instant.now \
           AND effective_until_unix_ms > instant.now \
         ORDER BY version DESC LIMIT 1",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    if current.as_ref().is_none_or(|row| {
        row.0 != policy.id()
            || row.1 != policy.version()
            || row.2 != policy.digest()
            || row.3 != policy.effective_from_unix_ms()
            || row.4 != policy.effective_until_unix_ms()
    }) {
        return Err(Error::BudgetPolicyInvalid);
    }
    Ok(())
}
