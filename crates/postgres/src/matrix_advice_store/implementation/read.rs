use super::*;

impl PgUnitOfWork {
    pub(super) async fn load_guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<StoredGuardedMatrixAdviceRecord>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT a.advice_id,a.opportunity_id,a.dispatch_id,a.task_id,a.matrix_task_revision, \
                    a.matrix_choice_set_digest,a.kind,a.ranked_choice_ids,a.reason,a.advice_digest, \
                    a.provider_profile_ref,a.model_configuration,a.response_payload_sha256, \
                    a.ranking_policy_version,a.trial_uncertainty, \
                    o.material_digest,o.matrix_verification_digest,r.input_digest,r.canonical_input,r.choice_set,d.response_payload,d.configuration_snapshot,d.configuration_digest \
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
        let verification_digest: Option<String> = row
            .try_get("matrix_verification_digest")
            .map_err(storage_error)?;
        let task_id: Uuid = row.try_get("task_id").map_err(storage_error)?;
        let task_revision: i64 = row.try_get("matrix_task_revision").map_err(storage_error)?;
        let verification_row = if let Some(digest) = verification_digest.as_deref() {
            Some(sqlx::query(
                "SELECT id,input_digest,schema,owner_principal_id,verifier_principal_id,policy_version,record_digest,verification_reason, \
                        frozen_snapshot_id,requirements_semantic_digest,authority_schema, \
                        FLOOR(EXTRACT(EPOCH FROM verified_at))::bigint AS verified_epoch \
                 FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 \
                   AND task_revision=$4 AND record_digest=$5",
            )
            .bind(tenant)
            .bind(workspace_id)
            .bind(task_id)
            .bind(task_revision)
            .bind(digest)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?
            .ok_or(Error::InternalInvariant)?)
        } else {
            None
        };
        let verification = verification_row
            .as_ref()
            .map(verification_authority)
            .transpose()?
            .unwrap_or(MatrixVerificationAuthority::Unverified);
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
                verification,
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
        if let Some(verification_row) = verification_row {
            // Read historical advice against its immutable verification at the
            // instant it was recorded. Expiration after advice was saved must
            // not make an otherwise intact receipt unreadable.
            let verified_epoch: i64 = verification_row
                .try_get("verified_epoch")
                .map_err(storage_error)?;
            let actual_digest = match &advice.record.binding.verification {
                MatrixVerificationAuthority::LegacyV1 { .. } => {
                    let verification = self
                        .decode_verification(workspace_id, task_id, task_revision, verification_row)
                        .await?;
                    let validated = evaluate_matrix_verification(
                        &task_id.to_string(),
                        &task_revision.to_string(),
                        &input,
                        &verification,
                        verified_epoch,
                    )
                    .map_err(|_| Error::InternalInvariant)?;
                    let reported =
                        OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                            task_id.to_string(),
                            task_revision.to_string(),
                            input.clone(),
                        )
                        .map_err(|_| Error::InternalInvariant)?;
                    let composition =
                        compose_independently_verified_owner_matrix(&reported, &validated)
                            .map_err(|_| Error::InternalInvariant)?;
                    matrix_verified_evaluation_digest(&input, &composition, &choice, &validated)
                        .map_err(|_| Error::InternalInvariant)?
                }
                MatrixVerificationAuthority::ContextV2 {
                    snapshot_id,
                    authority_schema,
                    semantic_digest,
                    ..
                } => {
                    let verification = self
                        .decode_context_verification(
                            task_id,
                            task_revision,
                            workspace_id,
                            verification_row,
                        )
                        .await?;
                    if verification.frozen_snapshot_id != snapshot_id.to_string()
                        || verification.authority_schema != *authority_schema
                        || verification.requirements_semantic_digest != *semantic_digest
                    {
                        return Err(Error::InternalInvariant);
                    }
                    let binding = sqlx::query(
                        "SELECT snapshot_id,semantic_digest,authority_schema \
                         FROM matrix_task_requirements_bindings \
                         WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4",
                    )
                    .bind(tenant)
                    .bind(workspace_id)
                    .bind(task_id)
                    .bind(task_revision)
                    .fetch_optional(&mut **self.transaction()?)
                    .await
                    .map_err(storage_error)?
                    .ok_or(Error::InternalInvariant)?;
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
                        return Err(Error::InternalInvariant);
                    }
                    let frozen = MatrixRequirementsContextStore::frozen_matrix_requirements_by_id(
                        self,
                        workspace_id,
                        *snapshot_id,
                    )
                    .await?
                    .ok_or(Error::InternalInvariant)?;
                    let validated = evaluate_context_matrix_verification(
                        &task_id.to_string(),
                        &task_revision.to_string(),
                        &snapshot_id.to_string(),
                        &input,
                        &frozen.effective,
                        &verification,
                        verified_epoch,
                    )
                    .map_err(|_| Error::InternalInvariant)?;
                    let composition = compose_confirmed_requirements_matrix(
                        &task_id.to_string(),
                        &task_revision.to_string(),
                        &snapshot_id.to_string(),
                        &input,
                        &frozen.effective,
                        &validated,
                        verified_epoch,
                    )
                    .map_err(|_| Error::InternalInvariant)?;
                    context_matrix_verified_evaluation_digest(
                        &input,
                        &composition,
                        &choice,
                        &verification,
                    )
                    .map_err(|_| Error::InternalInvariant)?
                }
                MatrixVerificationAuthority::Unverified => return Err(Error::InternalInvariant),
            };
            if actual_digest != advice.record.binding.evaluation_digest {
                return Err(Error::InternalInvariant);
            }
        }
        Ok(Some(advice))
    }
}
