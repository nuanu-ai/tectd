type MatrixObservationRow = (
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<String>,
    i64,
    bool,
);

async fn attach_matrix_observation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    saved: &mut tect_application::StoredMatrixDispatch,
) -> Result<()> {
    let row: Option<MatrixObservationRow> = sqlx::query_as(
        "SELECT response_payload,http_status,original_input_tokens,original_output_tokens,elapsed_ms,response_complete \
         FROM advisory_provider_observations WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3"
    ).bind(tenant).bind(workspace).bind(saved.dispatch.id).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    if let Some((raw, status, input, output, elapsed, complete)) = row {
        if saved.dispatch.state == AdvisoryDispatchState::Sealed && saved.response_payload != raw {
            return Err(Error::InputConflict);
        }
        saved.response_payload_sha256 = raw
            .as_ref()
            .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
        saved.response_payload = raw;
        saved.response_http_status = status
            .map(u16::try_from)
            .transpose()
            .map_err(|_| Error::StorageUnavailable)?;
        saved.original_input_tokens = input
            .map(|n| n.parse())
            .transpose()
            .map_err(|_| Error::StorageUnavailable)?;
        saved.original_output_tokens = output
            .map(|n| n.parse())
            .transpose()
            .map_err(|_| Error::StorageUnavailable)?;
        saved.original_elapsed_ms = Some(elapsed);
        saved.raw_observation_sealed = true;
        saved.response_complete = complete;
    }
    Ok(())
}

pub(crate) async fn matrix_continuation_saved(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    continuation: &tect_application::MatrixDispatchContinuation,
) -> Result<tect_application::StoredMatrixDispatch> {
    let _locked = dispatch_by_id(
        tx,
        tenant,
        continuation.workspace_id(),
        continuation.dispatch_id(),
        true,
    )
    .await?;
    let saved = matrix_dispatch_for_recovery(
        tx,
        tenant,
        continuation.workspace_id(),
        continuation.actor_id(),
        continuation.opportunity_id(),
        Some(continuation.dispatch_id()),
    )
    .await?;
    if saved.dispatch.configuration_digest != continuation.configuration_digest()
        || saved.request_payload_sha256 != continuation.request_sha256()
        || !matches!(
            saved.dispatch.state,
            AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed
        )
    {
        return Err(Error::InputConflict);
    }
    // The application-owned header must equal the actual committed reservation.
    let reservation = reservation_for_dispatch(
        tx,
        tenant,
        continuation.workspace_id(),
        continuation.dispatch_id(),
    )
    .await?
    .ok_or(Error::BudgetPolicyInvalid)?;
    if reservation.request_sha256 != saved.request_payload_sha256
        || reservation.request_utf8_bytes
            != i64::try_from(saved.request_payload.len()).unwrap_or(-1)
        || saved.configuration_snapshot.get("budget_policy_id")
            != Some(&serde_json::json!(reservation.policy_id.to_string()))
        || saved.configuration_snapshot.get("budget_policy")
            != Some(&serde_json::json!({
                "policy_id": reservation.policy_id.to_string(),
                "policy_version": reservation.policy_version,
                "policy_digest": reservation.policy_digest,
            }))
    {
        return Err(Error::BudgetPolicyInvalid);
    }
    Ok(saved)
}

pub(crate) async fn seal_matrix_raw_observation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    continuation: &tect_application::MatrixDispatchContinuation,
    observation: &tect_application::MatrixProviderObservation,
    elapsed: i64,
) -> Result<tect_application::StoredMatrixDispatch> {
    if elapsed < 0
        || observation.response_payload.is_none() && observation.response_complete
        || observation
            .http_status
            .is_some_and(|n| !(100..=599).contains(&n))
        || observation.response_payload.is_none()
            && (observation.http_status.is_some()
                || observation.input_tokens.is_some()
                || observation.output_tokens.is_some())
    {
        return Err(Error::InvalidArguments);
    }
    let saved = matrix_continuation_saved(tx, tenant, continuation).await?;
    if saved.raw_observation_sealed {
        if saved.response_payload != observation.response_payload
            || saved.response_http_status != observation.http_status
            || saved.original_input_tokens != observation.input_tokens
            || saved.original_output_tokens != observation.output_tokens
            || saved.original_elapsed_ms != Some(elapsed)
            || saved.response_complete != observation.response_complete
        {
            return Err(Error::InputConflict);
        }
        return Ok(saved);
    }
    if saved.dispatch.state != AdvisoryDispatchState::Sending {
        return Err(Error::InputConflict);
    }
    sqlx::query("INSERT INTO advisory_provider_observations (tenant_id,workspace_id,opportunity_id,dispatch_id,configuration_digest,request_sha256,response_payload,response_sha256,http_status,original_input_tokens,original_output_tokens,elapsed_ms,original_transport_outcome,response_complete) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14)")
        .bind(tenant).bind(continuation.workspace_id()).bind(continuation.opportunity_id()).bind(continuation.dispatch_id())
        .bind(continuation.configuration_digest()).bind(continuation.request_sha256()).bind(&observation.response_payload)
        .bind(observation.response_payload.as_ref().map(|bytes| format!("{:x}", Sha256::digest(bytes))))
        .bind(observation.http_status.map(i32::from)).bind(observation.input_tokens.map(|n| n.to_string()))
        .bind(observation.output_tokens.map(|n| n.to_string())).bind(elapsed)
        .bind(if observation.response_payload.is_none() { "transport_failure" } else if observation.response_complete { "received" } else { "partial_received" })
        .bind(observation.response_complete)
        .execute(&mut **tx).await.map_err(storage_error)?;
    matrix_continuation_saved(tx, tenant, continuation).await
}

pub(crate) async fn seal_matrix_observation_usage(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    continuation: &tect_application::MatrixDispatchContinuation,
    usage: tect_application::MatrixProviderUsage,
) -> Result<()> {
    let saved = matrix_continuation_saved(tx, tenant, continuation).await?;
    if !saved.raw_observation_sealed {
        return Err(Error::InputConflict);
    }
    let seal = matrix_usage_seal(
        continuation.dispatch_id(),
        saved.response_payload,
        usage,
        saved.original_elapsed_ms,
        saved.response_complete,
    );
    seal_dispatch(tx, tenant, continuation.workspace_id(), &seal).await?;
    Ok(())
}

fn matrix_usage_seal(
    dispatch_id: Uuid,
    raw: Option<Vec<u8>>,
    usage: tect_application::MatrixProviderUsage,
    elapsed: Option<i64>,
    complete: bool,
) -> AdvisoryDispatchSeal {
    let received = raw.is_some();
    let usage = if complete {
        usage
    } else {
        tect_application::MatrixProviderUsage::default()
    };
    AdvisoryDispatchSeal {
        dispatch_id,
        send_certainty: if received {
            AdvisorySendCertainty::Sent
        } else {
            AdvisorySendCertainty::SentUnknown
        },
        outcome: if received {
            AdvisoryDispatchOutcome::ProviderResponse
        } else {
            AdvisoryDispatchOutcome::ProviderFailure
        },
        response_payload: raw,
        input_tokens: usage.input_tokens.and_then(|n| i64::try_from(n).ok()),
        output_tokens: usage.output_tokens.and_then(|n| i64::try_from(n).ok()),
        latency_ms: elapsed,
        raw_response_ref: None,
    }
}

#[cfg(test)]
mod matrix_observation_tests {
    use super::*;
    use tect_application::MatrixProviderUsage;

    #[test]
    fn received_empty_or_malformed_bytes_remain_known_received_with_original_elapsed() {
        for raw in [
            Vec::new(),
            b"provider HTTP error is not JSON".to_vec(),
            vec![0xff, 0],
        ] {
            let seal = matrix_usage_seal(
                Uuid::new_v4(),
                Some(raw.clone()),
                MatrixProviderUsage::default(),
                Some(23),
                true,
            );
            seal.validate().unwrap();
            assert_eq!(seal.send_certainty, AdvisorySendCertainty::Sent);
            assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderResponse);
            assert_eq!(seal.response_payload, Some(raw));
            assert_eq!(seal.latency_ms, Some(23));
        }
    }

    #[test]
    fn transport_failure_stays_unknown_and_unrepresentable_usage_is_never_known() {
        let seal = matrix_usage_seal(
            Uuid::new_v4(),
            None,
            MatrixProviderUsage::default(),
            Some(5),
            false,
        );
        assert_eq!(seal.send_certainty, AdvisorySendCertainty::SentUnknown);
        assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderFailure);
        assert!(seal.response_payload.is_none());
        let overflow = matrix_usage_seal(
            Uuid::new_v4(),
            Some(b"raw".to_vec()),
            MatrixProviderUsage {
                input_tokens: Some(u64::MAX),
                output_tokens: Some(4),
            },
            Some(5),
            true,
        );
        assert_eq!(overflow.input_tokens, None);
        assert_eq!(overflow.output_tokens, Some(4));
        let partial = matrix_usage_seal(
            Uuid::new_v4(),
            Some(b"prefix".to_vec()),
            MatrixProviderUsage {
                input_tokens: Some(1),
                output_tokens: Some(2),
            },
            Some(7),
            false,
        );
        assert_eq!(partial.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(
            partial.response_payload.as_deref(),
            Some(b"prefix".as_slice())
        );
        assert_eq!((partial.input_tokens, partial.output_tokens), (None, None));
    }
}

pub(crate) async fn consume_matrix_observation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    continuation: &tect_application::MatrixDispatchContinuation,
) -> Result<(
    tect_application::StoredMatrixDispatch,
    AdvisoryBudgetConsumption,
)> {
    let saved = matrix_continuation_saved(tx, tenant, continuation).await?;
    let consumption = consume_budget(
        tx,
        tenant,
        continuation.workspace_id(),
        continuation.dispatch_id(),
    )
    .await?;
    Ok((saved, consumption))
}
