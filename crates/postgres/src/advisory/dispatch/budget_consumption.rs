type BudgetConsumptionDbRow = (
    Uuid,
    Uuid,
    i64,
    String,
    Option<i64>,
    Option<i64>,
    Option<i64>,
    bool,
    bool,
);

#[derive(Clone, Copy)]
struct ConsumptionLimits {
    input_tokens: i64,
    output_tokens: i64,
    elapsed_ms: i64,
}

#[derive(Clone, Copy)]
struct ConsumptionPriorUsage {
    input_tokens: i64,
    output_tokens: i64,
    elapsed_ms: i64,
    invalid: bool,
}

#[derive(Clone, Copy)]
struct ConsumptionObservation {
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    elapsed_ms: Option<i64>,
}

async fn consumed_budget(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<Option<AdvisoryBudgetConsumption>> {
    let row: Option<BudgetConsumptionDbRow> = sqlx::query_as(
            "SELECT dispatch_id,policy_id,policy_version,policy_digest,input_tokens,\
             output_tokens,monotonic_elapsed_ms,unknown_usage,exhausted_after_response \
             FROM advisory_budget_consumptions WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3"
        ).bind(tenant).bind(workspace).bind(dispatch_id)
            .fetch_optional(&mut **tx).await.map_err(storage_error)?;
    Ok(row.map(|r| AdvisoryBudgetConsumption {
        dispatch_id: r.0,
        policy_id: r.1,
        policy_version: r.2,
        policy_digest: r.3,
        input_tokens: r.4,
        output_tokens: r.5,
        monotonic_elapsed_ms: r.6,
        unknown_usage: r.7,
        exhausted_after_response: r.8,
    }))
}

fn consumption_exhausted(
    limits: ConsumptionLimits,
    prior: ConsumptionPriorUsage,
    observation: ConsumptionObservation,
) -> Result<(bool, bool)> {
    let unknown = observation.input_tokens.is_none()
        || observation.output_tokens.is_none()
        || observation.elapsed_ms.is_none();
    let total_input = prior
        .input_tokens
        .checked_add(observation.input_tokens.unwrap_or(0));
    let total_output = prior
        .output_tokens
        .checked_add(observation.output_tokens.unwrap_or(0));
    let total_elapsed = prior
        .elapsed_ms
        .checked_add(observation.elapsed_ms.unwrap_or(0));
    let over = total_input.is_none_or(|value| value > limits.input_tokens)
        || total_output.is_none_or(|value| value > limits.output_tokens)
        || total_elapsed.is_none_or(|value| value > limits.elapsed_ms);
    Ok((unknown, unknown || over || prior.invalid))
}

/// The caller must have committed the raw seal in a previous transaction.
/// This method is idempotent and locks the same dispatch/opportunity order as
/// start, so a concurrent replay cannot insert a second consumption.
async fn consume_budget(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch_id: Uuid,
) -> Result<AdvisoryBudgetConsumption> {
    let dispatch = dispatch_by_id(tx, tenant, workspace, dispatch_id, true).await?;
    if dispatch_state(&dispatch.state)? != AdvisoryDispatchState::Sealed {
        return Err(Error::InputConflict);
    }
    let _opportunity =
        opportunity_by_id(tx, tenant, workspace, dispatch.opportunity_id, true).await?;
    let reservation = reservation_for_dispatch(tx, tenant, workspace, dispatch_id)
        .await?
        .ok_or(Error::BudgetPolicyInvalid)?;
    if let Some(saved) = consumed_budget(tx, tenant, workspace, dispatch_id).await? {
        if saved.policy_id != reservation.policy_id
            || saved.policy_version != reservation.policy_version
            || saved.policy_digest != reservation.policy_digest
            || saved.input_tokens != dispatch.input_tokens
            || saved.output_tokens != dispatch.output_tokens
            || saved.monotonic_elapsed_ms != dispatch.latency_ms
        {
            return Err(Error::InputConflict);
        }
        return Ok(saved);
    }
    let ceilings: Option<(i64, i64, i64)> = sqlx::query_as(
        "SELECT input_tokens,output_tokens,elapsed_monotonic_ms FROM advisory_budget_policies \
         WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND version=$4 AND digest=$5",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(reservation.policy_id)
    .bind(reservation.policy_version)
    .bind(&reservation.policy_digest)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (input_limit, output_limit, elapsed_limit) = ceilings.ok_or(Error::BudgetPolicyInvalid)?;
    let usage = crate::budget_policy_usage::policy_usage(
        tx,
        tenant,
        workspace,
        reservation.policy_id,
        reservation.policy_version,
        &reservation.policy_digest,
    )
    .await?;
    if usage.pending != 1 {
        return Err(Error::BudgetPolicyInvalid);
    }
    let (unknown, exhausted) = consumption_exhausted(
        ConsumptionLimits {
            input_tokens: input_limit,
            output_tokens: output_limit,
            elapsed_ms: elapsed_limit,
        },
        ConsumptionPriorUsage {
            input_tokens: usage.input_tokens,
            output_tokens: usage.output_tokens,
            elapsed_ms: usage.elapsed_ms,
            invalid: usage.invalid != 0,
        },
        ConsumptionObservation {
            input_tokens: dispatch.input_tokens,
            output_tokens: dispatch.output_tokens,
            elapsed_ms: dispatch.latency_ms,
        },
    )?;
    let inserted = sqlx::query(
        "INSERT INTO advisory_budget_consumptions \
         (tenant_id,workspace_id,opportunity_id,dispatch_id,policy_id,policy_version,\
          policy_digest,request_sha256,request_utf8_bytes,response_sha256,raw_response_ref,\
          send_certainty,outcome,input_tokens,output_tokens,input_tokens_known,\
          output_tokens_known,monotonic_elapsed_ms,elapsed_known,calls,retry_dispatches,\
          unknown_usage,exhausted_after_response,sealed_at) \
         SELECT d.tenant_id,d.workspace_id,d.opportunity_id,d.id,$4,$5,$6,$7,$8,\
          CASE WHEN d.response_payload IS NULL THEN NULL ELSE \
            pg_catalog.encode(pg_catalog.sha256(d.response_payload),'hex') END,\
          d.raw_response_ref,d.send_certainty,d.outcome,d.input_tokens,d.output_tokens,\
          d.input_tokens IS NOT NULL,d.output_tokens IS NOT NULL,d.latency_ms,\
          d.latency_ms IS NOT NULL,1,$9,$10,$11,d.sealed_at \
         FROM advisory_dispatch d WHERE d.tenant_id=$1 AND d.workspace_id=$2 \
           AND d.id=$3 AND d.state='sealed'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(dispatch_id)
    .bind(reservation.policy_id)
    .bind(reservation.policy_version)
    .bind(&reservation.policy_digest)
    .bind(&reservation.request_sha256)
    .bind(reservation.request_utf8_bytes)
    .bind(reservation.reserved_retry_dispatches)
    .bind(unknown)
    .bind(exhausted)
    .execute(&mut **tx)
    .await
    .map_err(storage_error)?;
    if inserted.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(AdvisoryBudgetConsumption {
        dispatch_id,
        policy_id: reservation.policy_id,
        policy_version: reservation.policy_version,
        policy_digest: reservation.policy_digest,
        input_tokens: dispatch.input_tokens,
        output_tokens: dispatch.output_tokens,
        monotonic_elapsed_ms: dispatch.latency_ms,
        unknown_usage: unknown,
        exhausted_after_response: exhausted,
    })
}

#[cfg(test)]
mod budget_consumption_tests {
    use super::*;
    #[test]
    fn exact_limits_overrun_and_unknown_are_distinct() {
        let limits = ConsumptionLimits {
            input_tokens: 10,
            output_tokens: 20,
            elapsed_ms: 100,
        };
        let prior = ConsumptionPriorUsage {
            input_tokens: 0,
            output_tokens: 0,
            elapsed_ms: 0,
            invalid: false,
        };
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: Some(10),
                    output_tokens: Some(20),
                    elapsed_ms: Some(100),
                }
            ),
            Ok((false, false))
        );
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: Some(11),
                    output_tokens: Some(20),
                    elapsed_ms: Some(100),
                }
            ),
            Ok((false, true))
        );
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: Some(10),
                    output_tokens: Some(21),
                    elapsed_ms: Some(100),
                }
            ),
            Ok((false, true))
        );
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: Some(10),
                    output_tokens: Some(20),
                    elapsed_ms: Some(101),
                }
            ),
            Ok((false, true))
        );
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: None,
                    output_tokens: Some(1),
                    elapsed_ms: Some(1),
                }
            ),
            Ok((true, true))
        );
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: Some(1),
                    output_tokens: None,
                    elapsed_ms: Some(1),
                }
            ),
            Ok((true, true))
        );
        assert_eq!(
            consumption_exhausted(
                limits,
                prior,
                ConsumptionObservation {
                    input_tokens: Some(1),
                    output_tokens: Some(1),
                    elapsed_ms: None,
                }
            ),
            Ok((true, true))
        );
    }
}
