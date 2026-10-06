/// Matrix's typed view is assembled only after the common raw receipt commits.
async fn attach_matrix_observation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    saved: &mut tect_application::StoredMatrixDispatch,
) -> Result<()> {
    if let Some((observation, elapsed)) =
        read_provider_observation(tx, tenant, workspace, saved.dispatch.id).await?
    {
        apply_matrix_observation(saved, observation, elapsed)?;
    }
    Ok(())
}

fn apply_matrix_observation(
    saved: &mut tect_application::StoredMatrixDispatch,
    observation: tect_application::AdvisoryProviderReceiptObservation,
    elapsed: i64,
) -> Result<()> {
    if saved.dispatch.state == AdvisoryDispatchState::Sealed
        && saved.response_payload != observation.response_payload
    {
        return Err(Error::InputConflict);
    }
    saved.response_payload_sha256 = observation
        .response_payload
        .as_ref()
        .map(|raw| format!("{:x}", Sha256::digest(raw)));
    saved.response_payload = observation.response_payload;
    saved.response_http_status = observation.http_status;
    saved.original_input_tokens = observation.input_tokens;
    saved.original_output_tokens = observation.output_tokens;
    saved.original_elapsed_ms = Some(elapsed);
    saved.raw_observation_sealed = true;
    saved.response_complete = observation.response_complete;
    saved.original_transport_context = observation.original_transport_context;
    Ok(())
}

pub(crate) async fn matrix_receipt_view(
    tx: &mut Transaction<'_, Postgres>,
    continuation: &tect_application::AdvisoryDispatchContinuation,
) -> Result<tect_application::StoredMatrixDispatch> {
    if continuation.capability() != AdvisoryCapability::EngineeringProfile {
        return Err(Error::InputConflict);
    }
    matrix_dispatch_for_recovery(
        tx,
        continuation.tenant_id(),
        continuation.workspace_id(),
        continuation.actor_id(),
        continuation.opportunity_id(),
        Some(continuation.dispatch_id()),
    )
    .await
}
