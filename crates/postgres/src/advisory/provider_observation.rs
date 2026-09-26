use sha2::{Digest, Sha256};
use tect_application::{
    AdvisoryDispatchContinuation, AdvisoryProviderReceiptObservation, AdvisoryProviderReceiptUsage,
    AdvisoryProviderTransportContext, StoredAdvisoryProviderReceipt,
};

type ProviderObservationRow = (
    Option<Vec<u8>>,
    Option<i32>,
    Option<String>,
    Option<String>,
    i64,
    bool,
    Option<serde_json::Value>,
);

async fn read_provider_observation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    dispatch: Uuid,
) -> Result<Option<(AdvisoryProviderReceiptObservation, i64)>> {
    let row: Option<ProviderObservationRow> = sqlx::query_as(
        "SELECT response_payload,http_status,original_input_tokens,original_output_tokens,elapsed_ms,response_complete,original_transport_context \
         FROM advisory_provider_observations WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3"
    ).bind(tenant).bind(workspace).bind(dispatch).fetch_optional(&mut **tx).await.map_err(storage_error)?;
    row.map(|(raw, status, input, output, elapsed, complete, context)| {
        Ok((
            AdvisoryProviderReceiptObservation {
                response_payload: raw,
                http_status: status
                    .map(u16::try_from)
                    .transpose()
                    .map_err(|_| Error::StorageUnavailable)?,
                input_tokens: input
                    .map(|n| n.parse())
                    .transpose()
                    .map_err(|_| Error::StorageUnavailable)?,
                output_tokens: output
                    .map(|n| n.parse())
                    .transpose()
                    .map_err(|_| Error::StorageUnavailable)?,
                response_complete: complete,
                original_transport_context: context
                    .map(decode_transport_context)
                    .transpose()
                    .map_err(storage_error)?,
            },
            elapsed,
        ))
    })
    .transpose()
}

/// Locks and validates only frozen committed dispatch material. Current source,
/// configuration, session authority, and Matrix revisions are not read here.
pub(crate) async fn provider_receipt_for_continuation(
    tx: &mut Transaction<'_, Postgres>,
    continuation: &AdvisoryDispatchContinuation,
) -> Result<StoredAdvisoryProviderReceipt> {
    let tenant = continuation.tenant_id();
    let workspace = continuation.workspace_id();
    let row = dispatch_by_id(tx, tenant, workspace, continuation.dispatch_id(), true).await?;
    let opportunity =
        opportunity_by_id(tx, tenant, workspace, continuation.opportunity_id(), true).await?;
    let request_sha = format!("{:x}", Sha256::digest(&row.request_payload));
    let config_sha = format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(&row.configuration_snapshot).map_err(storage_error)?)
    );
    if row.opportunity_id != opportunity.id
        || opportunity.authorized_actor_id != continuation.actor_id()
        || opportunity.capability != continuation.capability()
        || opportunity.decision_point != continuation.decision_point()
        || opportunity.target_kind != continuation.target_kind()
        || opportunity.target_id != continuation.target_id()
        || opportunity.work_revision != continuation.work_revision()
        || opportunity.material_digest != continuation.material_digest()
        || row.material_digest != opportunity.material_digest
        || row.configuration_digest != continuation.configuration_digest()
        || config_sha != row.configuration_digest
        || row.payload_digest != continuation.request_sha256()
        || request_sha != row.payload_digest
        || !matches!(
            dispatch_state(&row.state)?,
            AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed
        )
        || row.configuration_snapshot.get("request_body_sha256")
            != Some(&serde_json::json!(request_sha))
    {
        return Err(Error::InputConflict);
    }
    let reservation = reservation_for_dispatch(tx, tenant, workspace, row.id)
        .await?
        .ok_or(Error::BudgetPolicyInvalid)?;
    if reservation.request_sha256 != request_sha
        || reservation.request_utf8_bytes != i64::try_from(row.request_payload.len()).unwrap_or(-1)
        || reservation.reserved_calls != 1
    {
        return Err(Error::BudgetPolicyInvalid);
    }
    let policy_exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM advisory_budget_policies WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND version=$4 AND digest=$5)"
    ).bind(tenant).bind(workspace).bind(reservation.policy_id).bind(reservation.policy_version)
        .bind(&reservation.policy_digest).fetch_one(&mut **tx).await.map_err(storage_error)?;
    if !policy_exists {
        return Err(Error::BudgetPolicyInvalid);
    }
    if matches!(
        opportunity.capability,
        AdvisoryCapability::EngineeringProfile | AdvisoryCapability::ScopeDecomposition
    ) && (row.configuration_snapshot.get("request_body_length")
        != Some(&serde_json::json!(row.request_payload.len()))
        || row.configuration_snapshot.get("budget_policy_id")
            != Some(&serde_json::json!(reservation.policy_id.to_string()))
        || row.configuration_snapshot.get("budget_policy")
            != Some(&serde_json::json!({
                "policy_id": reservation.policy_id.to_string(), "policy_version": reservation.policy_version,
                "policy_digest": reservation.policy_digest,
            })))
    {
        return Err(Error::BudgetPolicyInvalid);
    }
    let observation = read_provider_observation(tx, tenant, workspace, row.id).await?;
    if let Some((raw, _)) = observation.as_ref()
        && dispatch_state(&row.state)? == AdvisoryDispatchState::Sealed
        && row.response_payload != raw.response_payload
    {
        return Err(Error::InputConflict);
    }
    Ok(StoredAdvisoryProviderReceipt {
        opportunity,
        dispatch: dispatch_from_row(&row)?,
        configuration_snapshot: row.configuration_snapshot,
        request_payload: row.request_payload,
        request_payload_sha256: request_sha,
        original_elapsed_ms: observation.as_ref().map(|(_, elapsed)| *elapsed),
        observation: observation.map(|(raw, _)| raw),
    })
}

pub(crate) async fn seal_provider_raw_observation(
    tx: &mut Transaction<'_, Postgres>,
    continuation: &AdvisoryDispatchContinuation,
    observation: &AdvisoryProviderReceiptObservation,
    elapsed: i64,
) -> Result<StoredAdvisoryProviderReceipt> {
    if elapsed < 0
        || observation
            .http_status
            .is_some_and(|n| !(100..=599).contains(&n))
        || observation.response_payload.is_none()
            && (observation.response_complete
                || observation.http_status.is_some()
                || observation.input_tokens.is_some()
                || observation.output_tokens.is_some())
    {
        return Err(Error::InvalidArguments);
    }
    if let Some(context) = observation.original_transport_context.as_ref() {
        context.validate_for(&observation.response_payload)?;
    }
    let saved = provider_receipt_for_continuation(tx, continuation).await?;
    if let Some(existing) = saved.observation.as_ref() {
        if existing != observation || saved.original_elapsed_ms != Some(elapsed) {
            return Err(Error::InputConflict);
        }
        return Ok(saved);
    }
    if saved.dispatch.state != AdvisoryDispatchState::Sending {
        return Err(Error::InputConflict);
    }
    sqlx::query("INSERT INTO advisory_provider_observations (tenant_id,workspace_id,opportunity_id,dispatch_id,configuration_digest,request_sha256,response_payload,response_sha256,http_status,original_input_tokens,original_output_tokens,elapsed_ms,original_transport_outcome,response_complete,original_transport_context) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
        .bind(continuation.tenant_id()).bind(continuation.workspace_id()).bind(continuation.opportunity_id()).bind(continuation.dispatch_id())
        .bind(continuation.configuration_digest()).bind(continuation.request_sha256()).bind(&observation.response_payload)
        .bind(observation.response_payload.as_ref().map(|bytes| format!("{:x}", Sha256::digest(bytes))))
        .bind(observation.http_status.map(i32::from)).bind(observation.input_tokens.map(|n| n.to_string()))
        .bind(observation.output_tokens.map(|n| n.to_string())).bind(elapsed)
        .bind(if observation.response_payload.is_none() { "transport_failure" } else if observation.response_complete { "received" } else { "partial_received" })
        .bind(observation.response_complete)
        .bind(observation.original_transport_context.as_ref().map(encode_transport_context).transpose()?)
        .execute(&mut **tx).await.map_err(storage_error)?;
    provider_receipt_for_continuation(tx, continuation).await
}

pub(crate) async fn seal_provider_observation_usage(
    tx: &mut Transaction<'_, Postgres>,
    continuation: &AdvisoryDispatchContinuation,
    usage: AdvisoryProviderReceiptUsage,
) -> Result<()> {
    let saved = provider_receipt_for_continuation(tx, continuation).await?;
    // Pipeline's seal guard awaits its own vertical integration.
    if saved.opportunity.capability == AdvisoryCapability::PipelineRecommendation {
        return Err(Error::TransportUnavailable);
    }
    let observation = saved.observation.ok_or(Error::InputConflict)?;
    let mut seal = provider_usage_seal(
        continuation.dispatch_id(),
        observation.response_payload,
        usage,
        saved.original_elapsed_ms,
        observation.response_complete,
    );
    if saved.opportunity.capability == AdvisoryCapability::ScopeDecomposition {
        apply_scope_transport_context(&mut seal, observation.original_transport_context.as_ref())?;
    }
    seal_dispatch(
        tx,
        continuation.tenant_id(),
        continuation.workspace_id(),
        &seal,
    )
    .await?;
    Ok(())
}

fn apply_scope_transport_context(
    seal: &mut AdvisoryDispatchSeal,
    context: Option<&AdvisoryProviderTransportContext>,
) -> Result<()> {
    let context = context.ok_or(Error::InputConflict)?;
    context.validate_for(&seal.response_payload)?;
    seal.send_certainty = context.send_certainty;
    seal.outcome = context.outcome;
    seal.raw_response_ref = context.raw_response_ref.clone();
    seal.validate()
}

#[cfg(test)]
mod provider_observation_tests {
    use super::*;

    #[test]
    fn scope_transport_failure_preserves_classification_ref_and_partial_unknown_usage() {
        let context = AdvisoryProviderTransportContext {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            raw_response_ref: Some("exact-original-ref".to_owned()),
            provider_failure_code: Some("opaque-provider-code".to_owned()),
        };
        let mut seal = provider_usage_seal(
            Uuid::new_v4(),
            Some(b"prefix".to_vec()),
            AdvisoryProviderReceiptUsage {
                input_tokens: Some(3),
                output_tokens: Some(4),
            },
            Some(19),
            false,
        );
        apply_scope_transport_context(&mut seal, Some(&context)).unwrap();
        assert_eq!(seal.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderFailure);
        assert_eq!(seal.raw_response_ref, context.raw_response_ref);
        assert_eq!((seal.input_tokens, seal.output_tokens), (None, None));
        assert_eq!(seal.latency_ms, Some(19));
        assert_eq!(
            apply_scope_transport_context(&mut seal, None),
            Err(Error::InputConflict)
        );
        let mut contradictory = context;
        contradictory.send_certainty = AdvisorySendCertainty::SentUnknown;
        assert_eq!(
            apply_scope_transport_context(&mut seal, Some(&contradictory)),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn received_empty_or_malformed_bytes_remain_known_received_with_original_elapsed() {
        for raw in [
            Vec::new(),
            b"provider HTTP error is not JSON".to_vec(),
            vec![0xff, 0],
        ] {
            let seal = provider_usage_seal(
                Uuid::new_v4(),
                Some(raw.clone()),
                AdvisoryProviderReceiptUsage::default(),
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
    fn transport_failure_and_partial_cannot_claim_known_usage() {
        let seal = provider_usage_seal(
            Uuid::new_v4(),
            None,
            AdvisoryProviderReceiptUsage::default(),
            Some(5),
            false,
        );
        assert_eq!(seal.send_certainty, AdvisorySendCertainty::SentUnknown);
        assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderFailure);
        let supplied = AdvisoryProviderReceiptUsage {
            input_tokens: Some(1),
            output_tokens: Some(2),
        };
        let absent = provider_usage_seal(Uuid::new_v4(), None, supplied, Some(7), true);
        assert_eq!((absent.input_tokens, absent.output_tokens), (None, None));
        let partial = provider_usage_seal(
            Uuid::new_v4(),
            Some(b"prefix".to_vec()),
            supplied,
            Some(7),
            false,
        );
        assert_eq!(partial.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(
            partial.response_payload.as_deref(),
            Some(b"prefix".as_slice())
        );
        assert_eq!((partial.input_tokens, partial.output_tokens), (None, None));
        let overflow = provider_usage_seal(
            Uuid::new_v4(),
            Some(b"raw".to_vec()),
            AdvisoryProviderReceiptUsage {
                input_tokens: Some(u64::MAX),
                output_tokens: Some(4),
            },
            Some(5),
            true,
        );
        assert_eq!(
            (overflow.input_tokens, overflow.output_tokens),
            (None, Some(4))
        );
    }
}

fn provider_usage_seal(
    dispatch_id: Uuid,
    raw: Option<Vec<u8>>,
    usage: AdvisoryProviderReceiptUsage,
    elapsed: Option<i64>,
    complete: bool,
) -> AdvisoryDispatchSeal {
    let received = raw.is_some();
    let usage = if complete && received {
        usage
    } else {
        AdvisoryProviderReceiptUsage::default()
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

pub(crate) async fn consume_provider_observation(
    tx: &mut Transaction<'_, Postgres>,
    continuation: &AdvisoryDispatchContinuation,
) -> Result<(StoredAdvisoryProviderReceipt, AdvisoryBudgetConsumption)> {
    let saved = provider_receipt_for_continuation(tx, continuation).await?;
    let consumption = consume_budget(
        tx,
        continuation.tenant_id(),
        continuation.workspace_id(),
        continuation.dispatch_id(),
    )
    .await?;
    Ok((saved, consumption))
}
