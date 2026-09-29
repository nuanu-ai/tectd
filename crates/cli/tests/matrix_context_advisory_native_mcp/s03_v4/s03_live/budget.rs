use super::*;
use tect_domain::AdvisoryBudgetPolicy;

type MatrixBudgetRow = (Uuid, i64, i64, i64, i64, bool, bool, String, String, String);

pub(crate) struct Headroom {
    pub(crate) calls: i64,
    pub(crate) request_bytes: i64,
    pub(crate) input_tokens: i64,
    pub(crate) output_tokens: i64,
    pub(crate) elapsed_ms: i64,
}

/// Read the same immutable policy and policy-wide ledger used by the dispatch
/// guard. The synthetic Matrix attempt must be the sole prior reservation.
#[allow(clippy::too_many_arguments)]
pub(crate) async fn remaining_for_pipeline(
    pool: &PgPool,
    store: &PgStore,
    enrolled: &tect_postgres::admin::Enrollment,
    workspace: Uuid,
    task: Uuid,
    pipeline_opportunity: Uuid,
    expected: &AdvisoryBudgetPolicy,
    exact_request_bytes: usize,
    timeout_ms: i64,
) -> Option<Headroom> {
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .expect("clock before epoch")
            .as_millis(),
    )
    .expect("clock overflow");
    let mut tx = store
        .begin(TransactionMode::ReadOnly)
        .await
        .expect("budget read transaction");
    tx.authenticate(&enrolled.auth)
        .await
        .expect("budget actor authentication");
    tx.set_tenant(enrolled.tenant_id)
        .await
        .expect("budget tenant binding");
    let current = tx
        .advisory_budget_policy_store()
        .expect("budget policy store")
        .authorized_budget_policy(workspace, now)
        .await
        .expect("budget policy read")
        .expect("signed current policy missing");
    tx.commit().await.expect("budget read commit");
    if current != *expected
        || !current.is_effective_at(now)
        || current.effective_until_unix_ms().checked_sub(now)? < timeout_ms
        || exact_request_bytes == 0
    {
        return None;
    }
    let matrix: Vec<MatrixBudgetRow> = sqlx::query_as(
        r#"SELECT r.policy_id,r.reserved_calls,r.reserved_retry_dispatches,c.calls,c.retry_dispatches,
                  c.unknown_usage,c.exhausted_after_response,d.state,d.send_certainty,d.outcome
           FROM advisory_opportunity o JOIN advisory_dispatch d ON
             (d.tenant_id,d.workspace_id,d.opportunity_id)=(o.tenant_id,o.workspace_id,o.id)
           JOIN advisory_budget_reservations r ON
             (r.tenant_id,r.workspace_id,r.dispatch_id)=(d.tenant_id,d.workspace_id,d.id)
           JOIN advisory_budget_consumptions c ON
             (c.tenant_id,c.workspace_id,c.dispatch_id)=(d.tenant_id,d.workspace_id,d.id)
           WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.capability='engineering_profile'
             AND o.work_item_id=$3"#,
    ).bind(enrolled.tenant_id).bind(workspace).bind(task)
     .fetch_all(pool).await.expect("Matrix budget ledger query");
    if matrix.len() != 1 {
        return None;
    }
    let matrix = &matrix[0];
    if matrix.0 != current.id()
        || (matrix.1, matrix.2, matrix.3, matrix.4) != (1, 0, 1, 0)
        || matrix.5
        || matrix.6
        || matrix.7 != "sealed"
        || matrix.8 != "sent"
        || matrix.9 != "provider_response"
    {
        return None;
    }
    let pipeline_dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(enrolled.tenant_id)
    .bind(workspace)
    .bind(pipeline_opportunity)
    .fetch_one(pool)
    .await
    .expect("Pipeline dispatch count query");
    if pipeline_dispatches != 0 {
        return None;
    }
    let usage: (i64, i64, i64, i64, i64, i64, i64, i64) = sqlx::query_as(
        "SELECT calls,request_bytes,retries,input_tokens,output_tokens,elapsed_ms,pending,invalid FROM advisory_budget_policy_usage_totals($1,$2,$3,$4,$5)",
    )
    .bind(enrolled.tenant_id)
    .bind(workspace)
    .bind(current.id())
    .bind(current.version())
    .bind(current.digest())
    .fetch_one(pool)
    .await
    .expect("policy-wide usage query");
    let ceiling = current.ceilings();
    let headroom = Headroom {
        calls: ceiling.provider_calls.checked_sub(usage.0)?,
        request_bytes: ceiling.request_utf8_bytes.checked_sub(usage.1)?,
        input_tokens: ceiling.input_tokens.checked_sub(usage.3)?,
        output_tokens: ceiling.output_tokens.checked_sub(usage.4)?,
        elapsed_ms: ceiling.elapsed_monotonic_ms.checked_sub(usage.5)?,
    };
    // Provider token use cannot be predicted before dispatch. Require the
    // exact frozen request bytes to fit, plus positive signed token headroom.
    let exact_request_bytes = i64::try_from(exact_request_bytes).ok()?;
    if usage.0 != 1
        || usage.2 != 0
        || usage.6 != 0
        || usage.7 != 0
        || headroom.calls != 1
        || headroom.request_bytes < exact_request_bytes
        || headroom.input_tokens <= 0
        || headroom.output_tokens <= 0
        || headroom.elapsed_ms < timeout_ms
    {
        return None;
    }
    Some(headroom)
}
