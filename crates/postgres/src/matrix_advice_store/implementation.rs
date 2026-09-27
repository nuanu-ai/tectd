use super::*;

mod legacy;
mod read;

#[async_trait]
impl MatrixAdviceStore for PgUnitOfWork {
    async fn guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<StoredGuardedMatrixAdviceRecord>> {
        self.load_guarded_matrix_advice(workspace_id, opportunity_id)
            .await
    }

    async fn persist_guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
    ) -> Result<StoredGuardedMatrixAdviceRecord> {
        self.persist_matrix_advice(workspace_id, record, false)
            .await
    }

    async fn finalize_guarded_matrix_advice(
        &mut self,
        capability: &AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatch,
        record: Option<&GuardedMatrixAdviceRecord>,
        verification_stale: bool,
    ) -> Result<AdvisoryOpportunity> {
        let tenant = self.tenant_id()?;
        let previous: String = sqlx::query_scalar(
            "SELECT state FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(opportunity_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?
        .ok_or(Error::NotFound)?;
        let _ = capability;
        let result = finalize_matrix_response(
            self.transaction()?,
            tenant,
            workspace_id,
            opportunity_id,
            expected_config_revision,
            dispatch,
            verification_stale,
            record.is_some(),
        )
        .await?;
        if result.state == AdvisoryOpportunityState::Advised {
            let record = record.ok_or(Error::InternalInvariant)?;
            if record.opportunity_id != opportunity_id || record.dispatch_id != dispatch.id {
                return Err(Error::InputConflict);
            }
            // Only a newly terminalized opportunity can reuse the freshness
            // decision made by finalize under this transaction's task lock.
            // A replay of an already Advised row takes the current-time path.
            self.persist_matrix_advice(
                workspace_id,
                record,
                previous == AdvisoryOpportunityState::AwaitingResponse.as_str()
                    || previous == AdvisoryOpportunityState::Unresolved.as_str(),
            )
            .await?;
        }
        Ok(result)
    }
}

impl PgUnitOfWork {
    async fn persist_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
        verified_fresh_under_lock: bool,
    ) -> Result<StoredGuardedMatrixAdviceRecord> {
        let tenant = self.tenant_id()?;
        // Lock the occurrence and dispatch before configuration and the Matrix head.
        let opportunity = sqlx::query(
            "SELECT work_item_id,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,material_digest,config_revision,capability,decision_point,state,primary_reason \
             FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE"
        ).bind(tenant).bind(workspace_id).bind(record.opportunity_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::NotFound)?;
        let dispatch = sqlx::query(
            "SELECT opportunity_id,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,(sealed_at IS NOT NULL) AS is_sealed \
             FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE"
        ).bind(tenant).bind(workspace_id).bind(record.dispatch_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::InputConflict)?;
        let task_id: Option<Uuid> = opportunity.try_get("work_item_id").map_err(storage_error)?;
        let revision: Option<i64> = opportunity
            .try_get("matrix_task_revision")
            .map_err(storage_error)?;
        let digest: Option<String> = opportunity
            .try_get("matrix_choice_set_digest")
            .map_err(storage_error)?;
        let verification_digest: Option<String> = opportunity
            .try_get("matrix_verification_digest")
            .map_err(storage_error)?;
        let material: String = opportunity
            .try_get("material_digest")
            .map_err(storage_error)?;
        let config_revision: i64 = opportunity
            .try_get("config_revision")
            .map_err(storage_error)?;
        let snapshot: serde_json::Value = dispatch
            .try_get("configuration_snapshot")
            .map_err(storage_error)?;
        let request_payload: Vec<u8> =
            dispatch.try_get("request_payload").map_err(storage_error)?;
        let response_payload: Option<Vec<u8>> = dispatch
            .try_get("response_payload")
            .map_err(storage_error)?;
        let response_sha = format!("{:x}", Sha256::digest(&record.raw_response_payload));
        let config_sha = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&snapshot).map_err(storage_error)?)
        );
        if task_id != Some(record.binding.task_id)
            || revision != Some(record.binding.task_revision)
            || digest.as_deref() != Some(record.binding.choice_set_digest.as_str())
            || verification_digest.as_deref() != record.binding.verification.digest()
            || material != record.binding.evaluation_digest
            || material != record.opportunity_material_digest
            || opportunity
                .try_get::<String, _>("capability")
                .map_err(storage_error)?
                != "engineering_profile"
            || opportunity
                .try_get::<String, _>("decision_point")
                .map_err(storage_error)?
                != "engineering.profile.before_selection"
            || dispatch
                .try_get::<Uuid, _>("opportunity_id")
                .map_err(storage_error)?
                != record.opportunity_id
            || dispatch
                .try_get::<String, _>("material_digest")
                .map_err(storage_error)?
                != material
            || dispatch
                .try_get::<String, _>("provider")
                .map_err(storage_error)?
                != record.provider_profile_ref.id
            || dispatch
                .try_get::<String, _>("model")
                .map_err(storage_error)?
                != record.model_configuration.model
            || dispatch
                .try_get::<String, _>("configuration_digest")
                .map_err(storage_error)?
                != config_sha
            || dispatch
                .try_get::<String, _>("payload_digest")
                .map_err(storage_error)?
                != format!("{:x}", Sha256::digest(&request_payload))
            || snapshot.get("provider_profile_ref")
                != Some(&serde_json::json!(record.provider_profile_ref))
            || snapshot.get("model_configuration")
                != Some(&serde_json::json!(record.model_configuration))
            || snapshot.get("request_body_length")
                != Some(&serde_json::json!(request_payload.len()))
            || snapshot.get("request_body_sha256")
                != Some(&serde_json::json!(format!(
                    "{:x}",
                    Sha256::digest(&request_payload)
                )))
            || dispatch
                .try_get::<String, _>("state")
                .map_err(storage_error)?
                != "sealed"
            || dispatch
                .try_get::<String, _>("send_certainty")
                .map_err(storage_error)?
                != "sent"
            || !dispatch
                .try_get::<bool, _>("is_sealed")
                .map_err(storage_error)?
            || response_payload.as_deref() != Some(record.raw_response_payload.as_slice())
            || record.response_payload_sha256 != response_sha
        {
            return Err(Error::InputConflict);
        }

        let usable = !matches!(record.outcome, GuardedMatrixAdviceOutcome::Rejected { .. });
        let expected_outcome = if usable {
            "provider_response"
        } else {
            "provider_failure"
        };
        let expected_state = if usable { "advised" } else { "failed" };
        let expected_reason = if usable {
            "provider_response"
        } else {
            "provider_failure"
        };
        if dispatch
            .try_get::<Option<String>, _>("outcome")
            .map_err(storage_error)?
            .as_deref()
            != Some(expected_outcome)
            || opportunity
                .try_get::<String, _>("state")
                .map_err(storage_error)?
                != expected_state
            || opportunity
                .try_get::<String, _>("primary_reason")
                .map_err(storage_error)?
                != expected_reason
        {
            return Err(Error::InputConflict);
        }

        // A sealed receipt already persisted for this opportunity is immutable.
        // Reconciliation must remain readable after the context or evidence
        // becomes stale; freshness gates only a new advice insert.
        if let Some(prior) = self
            .guarded_matrix_advice(workspace_id, record.opportunity_id)
            .await?
        {
            return if prior.record == *record {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }

        if let MatrixVerificationAuthority::LegacyV1 {
            digest: saved_digest,
        } = &record.binding.verification
        {
            return self
                .persist_legacy_matrix_advice(workspace_id, record, saved_digest)
                .await;
        }
        let config = sqlx::query("SELECT revision,mode,provider_profile_ref,model_configuration FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
            .bind(tenant).bind(workspace_id).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::StaleRevision)?;
        let current = self
            .lock_matrix_task(workspace_id, record.binding.task_id)
            .await?
            .ok_or(Error::StaleRevision)?;
        if current.revision != record.binding.task_revision
            || current.input_digest != record.binding.input_digest
            || current.choice_set_digest.as_deref()
                != Some(record.binding.choice_set_digest.as_str())
            || config
                .try_get::<i64, _>("revision")
                .map_err(storage_error)?
                != config_revision
            || config.try_get::<String, _>("mode").map_err(storage_error)? != "optional"
            || config
                .try_get::<Option<String>, _>("provider_profile_ref")
                .map_err(storage_error)?
                .as_deref()
                != Some(record.provider_profile_ref.id.as_str())
            || config
                .try_get::<Option<serde_json::Value>, _>("model_configuration")
                .map_err(storage_error)?
                != Some(serde_json::json!(record.model_configuration))
        {
            return Err(Error::StaleRevision);
        }
        // The head lock also serializes verifier inserts. A replacement header,
        // changed input, or expired evidence must reject direct store callers.
        let latest = sqlx::query(
            "SELECT id,input_digest,schema,owner_principal_id,verifier_principal_id,policy_version,record_digest,verification_reason, \
                    frozen_snapshot_id,requirements_semantic_digest,authority_schema, \
                    FLOOR(EXTRACT(EPOCH FROM verified_at))::bigint AS verified_epoch \
             FROM matrix_verifications \
             WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND task_revision=$4 \
             ORDER BY verified_at DESC,id DESC LIMIT 1",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(record.binding.task_id)
        .bind(record.binding.task_revision)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(latest) = latest else {
            return Err(Error::StaleRevision);
        };
        let verification_id: Uuid = latest.try_get("id").map_err(storage_error)?;
        let verified_epoch: i64 = latest.try_get("verified_epoch").map_err(storage_error)?;
        let latest_digest: String = latest.try_get("record_digest").map_err(storage_error)?;
        let verified_input_digest: String =
            latest.try_get("input_digest").map_err(storage_error)?;
        let authority = verification_authority(&latest)?;
        if record.binding.verification != authority
            || record.binding.verification.digest() != Some(latest_digest.as_str())
            || verification_digest.as_deref() != Some(latest_digest.as_str())
            || verified_input_digest != current.input_digest
        {
            return Err(Error::StaleRevision);
        }
        let bindings_valid: bool = sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM matrix_verification_bindings \
               WHERE tenant_id=$1 AND workspace_id=$2 AND verification_id=$3) \
             AND NOT EXISTS (SELECT 1 FROM matrix_verification_bindings \
               WHERE tenant_id=$1 AND workspace_id=$2 AND verification_id=$3 \
                 AND (validation_outcome <> 'accepted' OR \
                   (NOT $4 AND expires_at <= EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))))",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(verification_id)
        .bind(verified_fresh_under_lock)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if !bindings_valid {
            return Err(Error::StaleRevision);
        }
        let choice = current.choice_set.as_ref().ok_or(Error::InputConflict)?;
        let eligibility = choice.validate(&current.input)?;
        let canonical = serde_json::to_value(&current.input).map_err(storage_error)?;
        if canonical_matrix_input_digest(&canonical)? != record.binding.input_digest
            || choice.choice_set_id != record.binding.choice_set_id
            || choice.version != record.binding.choice_set_version
            || choice.canonical_digest(&current.input)? != record.binding.choice_set_digest
        {
            return Err(Error::InputConflict);
        }
        // Finalize checked expiry under this task lock before terminalizing
        // an already sent response. Direct writes use the current clock.
        let current_epoch: i64 = sqlx::query_scalar(
            "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
        )
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let evaluation_digest = match &record.binding.verification {
            MatrixVerificationAuthority::LegacyV1 { .. } => {
                // Only previously persisted V1 advice can be replayed.
                return Err(Error::StaleRevision);
            }
            MatrixVerificationAuthority::ContextV2 {
                snapshot_id,
                authority_schema,
                semantic_digest,
                ..
            } => {
                let binding = sqlx::query(
                    "SELECT snapshot_id,semantic_digest,authority_schema \
                     FROM matrix_task_requirements_bindings \
                     WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4",
                )
                .bind(tenant)
                .bind(workspace_id)
                .bind(current.task_id)
                .bind(current.revision)
                .fetch_optional(&mut **self.transaction()?)
                .await
                .map_err(storage_error)?
                .ok_or(Error::StaleRevision)?;
                if binding
                    .try_get::<Uuid, _>("snapshot_id")
                    .map_err(storage_error)?
                    != *snapshot_id
                    || binding
                        .try_get::<String, _>("semantic_digest")
                        .map_err(storage_error)?
                        != *semantic_digest
                    || binding
                        .try_get::<String, _>("authority_schema")
                        .map_err(storage_error)?
                        != *authority_schema
                {
                    return Err(Error::StaleRevision);
                }
                let frozen = MatrixRequirementsContextStore::frozen_matrix_requirements_by_id(
                    self,
                    workspace_id,
                    *snapshot_id,
                )
                .await?
                .ok_or(Error::StaleRevision)?;
                if frozen.effective.schema() != authority_schema
                    || frozen.effective.semantic_digest() != semantic_digest
                {
                    return Err(Error::StaleRevision);
                }
                let verification = self
                    .decode_context_verification(
                        current.task_id,
                        current.revision,
                        workspace_id,
                        latest,
                    )
                    .await?;
                let validation_epoch = if verified_fresh_under_lock {
                    verified_epoch
                } else {
                    current_epoch
                };
                let validated = evaluate_context_matrix_verification(
                    &current.task_id.to_string(),
                    &current.revision.to_string(),
                    &snapshot_id.to_string(),
                    &current.input,
                    &frozen.effective,
                    &verification,
                    validation_epoch,
                )
                .map_err(|_| Error::StaleRevision)?;
                let composition = compose_confirmed_requirements_matrix(
                    &current.task_id.to_string(),
                    &current.revision.to_string(),
                    &snapshot_id.to_string(),
                    &current.input,
                    &frozen.effective,
                    &validated,
                    validation_epoch,
                )
                .map_err(|_| Error::StaleRevision)?;
                context_matrix_verified_evaluation_digest(
                    &current.input,
                    &composition,
                    choice,
                    &verification,
                )?
            }
            MatrixVerificationAuthority::Unverified => return Err(Error::StaleRevision),
        };
        if evaluation_digest != record.binding.evaluation_digest {
            return Err(Error::InputConflict);
        }
        record.validate_for(
            record.opportunity_id,
            record.dispatch_id,
            &record.binding,
            &record.provider_profile_ref,
            &record.model_configuration,
            &eligibility,
        )?;
        self.insert_matrix_advice_receipt(workspace_id, record)
            .await
    }
}
