use super::*;

#[async_trait]
impl MatrixAdviceStore for PgUnitOfWork {
    async fn guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<StoredGuardedMatrixAdviceRecord>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT a.advice_id,a.opportunity_id,a.dispatch_id,a.task_id,a.matrix_task_revision, \
                    a.matrix_choice_set_digest,a.kind,a.ranked_choice_ids,a.reason,a.advice_digest, \
                    a.provider_profile_ref,a.model_configuration,a.response_payload_sha256, \
                    o.material_digest,o.matrix_verification_digest,r.input_digest,r.canonical_input,r.choice_set,d.response_payload \
             FROM advisory_matrix_advice a \
             JOIN advisory_opportunity o ON (o.tenant_id,o.workspace_id,o.id)=(a.tenant_id,a.workspace_id,a.opportunity_id) \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=(a.tenant_id,a.workspace_id,a.task_id,a.matrix_task_revision) \
             JOIN advisory_dispatch d ON (d.tenant_id,d.workspace_id,d.id)=(a.tenant_id,a.workspace_id,a.dispatch_id) \
             WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.opportunity_id=$3"
        )
            .bind(tenant)
            .bind(workspace_id)
            .bind(opportunity_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let (advice, input, choice) = {
            let choice_json: serde_json::Value =
                row.try_get("choice_set").map_err(storage_error)?;
            let choice: EngineeringChoiceSet =
                serde_json::from_value(choice_json).map_err(|_| Error::InternalInvariant)?;
            let input_json: serde_json::Value =
                row.try_get("canonical_input").map_err(storage_error)?;
            let input: EngineeringMatrixInput =
                serde_json::from_value(input_json.clone()).map_err(|_| Error::InternalInvariant)?;
            let raw: Vec<u8> = row
                .try_get::<Option<Vec<u8>>, _>("response_payload")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?;
            let binding = MatrixProviderBinding {
                task_id: row.try_get("task_id").map_err(storage_error)?,
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
            if canonical_matrix_input_digest(&input_json)? != binding.input_digest
                || choice.canonical_digest(&input)? != binding.choice_set_digest
            {
                return Err(Error::InternalInvariant);
            }
            let eligibility = choice
                .validate(&input)
                .map_err(|_| Error::InternalInvariant)?;
            let advice = decode_advice(row, binding, raw)?;
            advice
                .record
                .validate_for(
                    advice.record.opportunity_id,
                    advice.record.dispatch_id,
                    &advice.record.binding,
                    &advice.record.provider_profile_ref,
                    &advice.record.model_configuration,
                    &eligibility,
                )
                .map_err(|_| Error::InternalInvariant)?;
            (advice, input, choice)
        };
        if let Some(verification_digest) = advice.record.binding.verification_digest.as_deref() {
            // Read historical advice against its immutable verification at the
            // instant it was recorded. Expiration after advice was saved must
            // not make an otherwise intact receipt unreadable.
            let verification_row = sqlx::query(
                "SELECT id,input_digest,schema,owner_principal_id,verifier_principal_id,policy_version,record_digest,verification_reason, \
                        FLOOR(EXTRACT(EPOCH FROM verified_at))::bigint AS verified_epoch \
                 FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 \
                   AND task_revision=$4 AND record_digest=$5",
            )
            .bind(tenant)
            .bind(workspace_id)
            .bind(advice.record.binding.task_id)
            .bind(advice.record.binding.task_revision)
            .bind(verification_digest)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?
            .ok_or(Error::InternalInvariant)?;
            let verified_epoch: i64 = verification_row
                .try_get("verified_epoch")
                .map_err(storage_error)?;
            let verification = self
                .decode_verification(
                    workspace_id,
                    advice.record.binding.task_id,
                    advice.record.binding.task_revision,
                    verification_row,
                )
                .await?;
            let validated = evaluate_matrix_verification(
                &advice.record.binding.task_id.to_string(),
                &advice.record.binding.task_revision.to_string(),
                &input,
                &verification,
                verified_epoch,
            )
            .map_err(|_| Error::InternalInvariant)?;
            let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                advice.record.binding.task_id.to_string(),
                advice.record.binding.task_revision.to_string(),
                input.clone(),
            )
            .map_err(|_| Error::InternalInvariant)?;
            let composition = compose_independently_verified_owner_matrix(&reported, &validated)
                .map_err(|_| Error::InternalInvariant)?;
            if matrix_verified_evaluation_digest(&input, &composition, &choice, &validated)
                .map_err(|_| Error::InternalInvariant)?
                != advice.record.binding.evaluation_digest
            {
                return Err(Error::InternalInvariant);
            }
        }
        Ok(Some(advice))
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
        let result = finalize_opportunity(
            self.transaction()?,
            tenant,
            workspace_id,
            opportunity_id,
            expected_config_revision,
            dispatch,
            verification_stale,
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
            || verification_digest != record.binding.verification_digest
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
        if record.binding.verification_digest.as_deref() != Some(latest_digest.as_str())
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
        let verification = self
            .decode_verification(workspace_id, current.task_id, current.revision, latest)
            .await?;
        // Finalize already checked expiry under this task lock before it
        // terminalized the opportunity. A second clock read can cross the
        // expiry boundary and roll that transition back after a sent reply.
        // The immutable verified_at checks canonical record structure in
        // that path; direct writes and later replays still require fresh time.
        let current_epoch: i64 = sqlx::query_scalar(
            "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
        )
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let validated = validated_for_guarded_advice(
            current.task_id,
            current.revision,
            &current.input,
            &verification,
            verified_fresh_under_lock,
            verified_epoch,
            current_epoch,
        )?;
        let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            current.task_id.to_string(),
            current.revision.to_string(),
            current.input.clone(),
        )?;
        let composition = compose_independently_verified_owner_matrix(&reported, &validated)?;
        if matrix_verified_evaluation_digest(&current.input, &composition, choice, &validated)?
            != record.binding.evaluation_digest
        {
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
        let (kind, ranks, reason) = outcome_columns(&record.outcome);
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO advisory_matrix_advice (tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision,matrix_choice_set_digest,dispatch_id,kind,ranked_choice_ids,reason,advice_digest,provider_profile_ref,model_configuration,response_payload_sha256) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT DO NOTHING RETURNING advice_id"
        ).bind(tenant).bind(workspace_id).bind(record.opportunity_id).bind(record.binding.task_id)
            .bind(record.binding.task_revision).bind(&record.binding.choice_set_digest)
            .bind(record.dispatch_id).bind(kind).bind(ranks).bind(reason)
            .bind(&record.advice_digest).bind(&record.provider_profile_ref.id)
            .bind(serde_json::json!(record.model_configuration)).bind(&record.response_payload_sha256)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let advice_id = inserted.ok_or(Error::InputConflict)?;
        Ok(StoredGuardedMatrixAdviceRecord {
            advice_id,
            record: record.clone(),
        })
    }
}
