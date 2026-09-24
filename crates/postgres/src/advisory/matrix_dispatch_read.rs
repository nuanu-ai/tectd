use sha2::{Digest, Sha256};
use sqlx::Row;
use tect_application::{
    MAX_PREPARED_MATRIX_BODY_BYTES, MatrixProviderBinding, StoredMatrixDispatch,
};

const MAX_RECOVERED_RESPONSE_BYTES: usize = 4 * 1024 * 1024;

/// Read only: the caller supplies the exact occurrence and attempt IDs. The
/// actor predicate and tenant RLS apply before either byte payload is returned.
pub(crate) async fn matrix_dispatch_for_recovery(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
    opportunity_id: Uuid,
    dispatch_id: Option<Uuid>,
) -> Result<StoredMatrixDispatch> {
    if workspace.is_nil()
        || actor.is_nil()
        || opportunity_id.is_nil()
        || dispatch_id.is_some_and(|id| id.is_nil())
    {
        return Err(Error::InvalidArguments);
    }
    let row = sqlx::query(
        "SELECT o.work_item_id,o.matrix_task_revision,o.matrix_choice_set_digest,\
                o.matrix_verification_digest,o.material_digest,r.input_digest,r.canonical_input,\
                r.choice_set,r.choice_set_digest AS revision_choice_digest \
         FROM advisory_opportunity o \
         JOIN matrix_task_revisions r ON \
           (r.tenant_id,r.workspace_id,r.task_id,r.revision)=\
           (o.tenant_id,o.workspace_id,o.work_item_id,o.matrix_task_revision) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.authorized_actor_id=$3 \
           AND o.id=$4 AND o.scope_id IS NULL AND o.work_item_kind='matrix_task' \
           AND o.capability='engineering_profile' \
           AND o.decision_point='engineering.profile.before_selection'",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(actor)
    .bind(opportunity_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    let choice: EngineeringChoiceSet =
        serde_json::from_value(row.try_get("choice_set").map_err(storage_error)?)
            .map_err(|_| Error::StorageUnavailable)?;
    let canonical_input: serde_json::Value =
        row.try_get("canonical_input").map_err(storage_error)?;
    let input: EngineeringMatrixInput =
        serde_json::from_value(canonical_input.clone()).map_err(|_| Error::StorageUnavailable)?;
    let binding = MatrixProviderBinding {
        task_id: row.try_get("work_item_id").map_err(storage_error)?,
        task_revision: row.try_get("matrix_task_revision").map_err(storage_error)?,
        input_digest: row.try_get("input_digest").map_err(storage_error)?,
        choice_set_id: choice.choice_set_id.clone(),
        choice_set_version: choice.version,
        choice_set_digest: row
            .try_get("matrix_choice_set_digest")
            .map_err(storage_error)?,
        evaluation_digest: row.try_get("material_digest").map_err(storage_error)?,
        verification_digest: row
            .try_get("matrix_verification_digest")
            .map_err(storage_error)?,
    };
    if binding.task_id.is_nil()
        || binding.task_revision < 1
        || binding.input_digest
            != tect_application::canonical_matrix_input_digest(&canonical_input)?
        || binding.choice_set_digest != choice.canonical_digest(&input)?
        || binding.choice_set_digest
            != row
                .try_get::<String, _>("revision_choice_digest")
                .map_err(storage_error)?
    {
        return Err(Error::StorageUnavailable);
    }
    let sizes: Option<(Uuid, i32, Option<i32>)> = sqlx::query_as(
        "SELECT id,octet_length(request_payload),octet_length(response_payload) \
         FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 \
           AND opportunity_id=$3 AND (($4::uuid IS NULL AND attempt_number=1) OR id=$4)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity_id)
    .bind(dispatch_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?;
    let (found_dispatch_id, request_size, response_size) = sizes.ok_or(Error::NotFound)?;
    if request_size < 1
        || request_size as usize > MAX_PREPARED_MATRIX_BODY_BYTES
        || response_size
            .is_some_and(|size| size < 0 || size as usize > MAX_RECOVERED_RESPONSE_BYTES)
    {
        return Err(Error::StorageUnavailable);
    }
    let dispatch = dispatch_by_id(tx, tenant, workspace, found_dispatch_id, false).await?;
    if dispatch.opportunity_id != opportunity_id
        || dispatch.material_digest != binding.evaluation_digest
    {
        return Err(Error::NotFound);
    }
    stored_matrix_dispatch(dispatch, binding)
}

fn stored_matrix_dispatch(
    row: DispatchRow,
    binding: MatrixProviderBinding,
) -> Result<StoredMatrixDispatch> {
    let dispatch = dispatch_from_row(&row)?;
    let request_sha = format!("{:x}", Sha256::digest(&row.request_payload));
    if row.request_payload.is_empty()
        || row.request_payload.len() > MAX_PREPARED_MATRIX_BODY_BYTES
        || row
            .response_payload
            .as_ref()
            .is_some_and(|bytes| bytes.len() > MAX_RECOVERED_RESPONSE_BYTES)
        || request_sha != row.payload_digest
        || format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&row.configuration_snapshot).map_err(storage_error)?)
        ) != row.configuration_digest
    {
        return Err(Error::StorageUnavailable);
    }
    let snapshot = row
        .configuration_snapshot
        .as_object()
        .ok_or(Error::StorageUnavailable)?;
    let profile: AdvisoryProviderProfileRef = serde_json::from_value(
        snapshot
            .get("provider_profile_ref")
            .cloned()
            .ok_or(Error::StorageUnavailable)?,
    )
    .map_err(|_| Error::StorageUnavailable)?;
    let model: AdvisoryModelConfiguration = serde_json::from_value(
        snapshot
            .get("model_configuration")
            .cloned()
            .ok_or(Error::StorageUnavailable)?,
    )
    .map_err(|_| Error::StorageUnavailable)?;
    let destination = snapshot
        .get("destination")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::StorageUnavailable)?
        .to_owned();
    let wire_version = snapshot
        .get("wire_version")
        .and_then(serde_json::Value::as_str)
        .ok_or(Error::StorageUnavailable)?
        .to_owned();
    if profile.validate().is_err()
        || model.validate().is_err()
        || profile.id != row.provider
        || model.model != row.model
        || destination.is_empty()
        || destination.contains('\0')
        || wire_version.is_empty()
        || wire_version.contains('\0')
        || snapshot.get("request_body_length")
            != Some(&serde_json::json!(row.request_payload.len()))
        || snapshot.get("request_body_sha256") != Some(&serde_json::json!(request_sha))
    {
        return Err(Error::StorageUnavailable);
    }
    let body: serde_json::Value =
        serde_json::from_slice(&row.request_payload).map_err(|_| Error::StorageUnavailable)?;
    let request_binding = body
        .pointer("/state/binding")
        .ok_or(Error::StorageUnavailable)?;
    let mut expected_binding = serde_json::json!({
        "task_id": binding.task_id.to_string(),
        "task_revision": binding.task_revision.to_string(),
        "input_digest": binding.input_digest,
        "choice_set_id": binding.choice_set_id,
        "choice_set_version": binding.choice_set_version,
        "choice_set_digest": binding.choice_set_digest,
        "evaluation_digest": binding.evaluation_digest,
        "verification_digest": binding.verification_digest,
    });
    if binding.verification_digest.is_none() {
        expected_binding
            .as_object_mut()
            .unwrap()
            .remove("verification_digest");
    }
    if body.get("model") != Some(&serde_json::json!(model.model))
        || request_binding != &expected_binding
    {
        return Err(Error::StorageUnavailable);
    }
    let response_sha = row
        .response_payload
        .as_ref()
        .map(|bytes| format!("{:x}", Sha256::digest(bytes)));
    if matches!(
        dispatch.state,
        AdvisoryDispatchState::Authorized
            | AdvisoryDispatchState::Sending
            | AdvisoryDispatchState::Cancelled
    ) && row.response_payload.is_some()
        || dispatch.outcome == Some(AdvisoryDispatchOutcome::ProviderResponse)
            && row.response_payload.is_none()
    {
        return Err(Error::StorageUnavailable);
    }
    Ok(StoredMatrixDispatch {
        dispatch,
        binding,
        provider_profile_ref: profile,
        model_configuration: model,
        configuration_snapshot: row.configuration_snapshot,
        destination,
        wire_version,
        request_payload: row.request_payload,
        request_payload_sha256: request_sha,
        response_payload: row.response_payload,
        response_payload_sha256: response_sha,
    })
}

#[cfg(test)]
mod matrix_dispatch_read_tests {
    use super::*;

    fn fixture(
        state: &str,
        certainty: &str,
        outcome: Option<&str>,
    ) -> (DispatchRow, MatrixProviderBinding) {
        let binding = MatrixProviderBinding {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            input_digest: "a".repeat(64),
            choice_set_id: "choice".into(),
            choice_set_version: 1,
            choice_set_digest: "b".repeat(64),
            evaluation_digest: "c".repeat(64),
            verification_digest: Some("d".repeat(64)),
        };
        let request_payload = serde_json::to_vec(&serde_json::json!({
            "model": "model", "state": {"binding": {
                "task_id": binding.task_id.to_string(),
                "task_revision": "1",
                "input_digest": binding.input_digest,
                "choice_set_id": binding.choice_set_id,
                "choice_set_version": 1,
                "choice_set_digest": binding.choice_set_digest,
                "evaluation_digest": binding.evaluation_digest,
                "verification_digest": binding.verification_digest,
            }}
        }))
        .unwrap();
        let payload_digest = format!("{:x}", Sha256::digest(&request_payload));
        let configuration_snapshot = serde_json::json!({
            "provider_profile_ref": {"id":"profile"},
            "model_configuration": {"model":"model"},
            "destination":"test", "wire_version":"matrix-ranking/2",
            "request_body_length":request_payload.len(),
            "request_body_sha256":payload_digest,
        });
        let configuration_digest = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&configuration_snapshot).unwrap())
        );
        let row = DispatchRow {
            id: Uuid::new_v4(),
            opportunity_id: Uuid::new_v4(),
            predecessor_dispatch_id: None,
            attempt_number: 1,
            provider: "profile".into(),
            model: "model".into(),
            configuration_snapshot,
            configuration_digest,
            material_digest: binding.evaluation_digest.clone(),
            payload_digest,
            request_payload,
            response_payload: (outcome == Some("provider_response"))
                .then(|| b"private-raw-response".to_vec()),
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: state.into(),
            send_certainty: certainty.into(),
            outcome: outcome.map(str::to_owned),
            retry_basis: "initial".into(),
            raw_response_ref: None,
        };
        (row, binding)
    }

    #[test]
    fn recovery_preserves_each_attempt_state_and_exact_bytes() {
        for (state, certainty, outcome) in [
            ("authorized", "not_sent", None),
            ("sending", "sent_unknown", None),
            ("sealed", "sent", Some("provider_response")),
            ("cancelled", "not_sent", None),
        ] {
            let (row, binding) = fixture(state, certainty, outcome);
            let exact_request = row.request_payload.clone();
            let exact_response = row.response_payload.clone();
            let saved = stored_matrix_dispatch(row, binding.clone()).unwrap();
            assert_eq!(saved.binding, binding);
            assert_eq!(saved.request_payload, exact_request);
            assert_eq!(saved.response_payload, exact_response);
            assert_eq!(saved.dispatch.state, dispatch_state(state).unwrap());
            assert_eq!(
                saved.dispatch.send_certainty,
                send_certainty(certainty).unwrap()
            );
            assert!(!format!("{saved:?}").contains("private-raw-response"));
        }
    }

    #[test]
    fn recovery_rejects_corrupt_and_oversized_payloads() {
        let (mut row, binding) = fixture("authorized", "not_sent", None);
        row.request_payload.push(b' ');
        assert!(matches!(
            stored_matrix_dispatch(row, binding),
            Err(Error::StorageUnavailable)
        ));
        let (mut row, binding) = fixture("sealed", "sent", Some("provider_response"));
        row.response_payload = Some(vec![0; MAX_RECOVERED_RESPONSE_BYTES + 1]);
        assert!(matches!(
            stored_matrix_dispatch(row, binding),
            Err(Error::StorageUnavailable)
        ));
    }

    #[test]
    fn recovery_rejects_cancelled_dispatch_with_response_bytes() {
        // The migration's lifecycle check does not prohibit these bytes, but
        // they cannot be trusted as evidence of a cancelled, unsent attempt.
        let (mut row, binding) = fixture("cancelled", "not_sent", None);
        row.response_payload = Some(b"unexpected provider response".to_vec());
        assert!(matches!(
            stored_matrix_dispatch(row, binding),
            Err(Error::StorageUnavailable)
        ));
    }
}
