use super::*;

impl PgUnitOfWork {
    pub(super) async fn persist_legacy_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
        saved_digest: &str,
    ) -> Result<StoredGuardedMatrixAdviceRecord> {
        let tenant = self.tenant_id()?;
        // A first V1 receipt is reconciliation only when 0108 captured the
        // exact sealed response before cutover. No current-head or config
        // check belongs here: they can change after the provider send.
        let cutover: Option<String> = sqlx::query_scalar(
            "SELECT fingerprint FROM public.matrix_v1_dispatch_cutover_allowlist \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 \
                   AND dispatch_id=$4 AND task_id=$5 AND task_revision=$6 \
                   AND verification_digest=$7",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(record.opportunity_id)
        .bind(record.dispatch_id)
        .bind(record.binding.task_id)
        .bind(record.binding.task_revision)
        .bind(saved_digest)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(cutover) = cutover else {
            return Err(Error::StaleRevision);
        };
        let current_fingerprint: Option<String> =
            sqlx::query_scalar("SELECT public.matrix_v1_cutover_fingerprint($1,$2,$3,$4)")
                .bind(tenant)
                .bind(workspace_id)
                .bind(record.opportunity_id)
                .bind(record.dispatch_id)
                .fetch_one(&mut **self.transaction()?)
                .await
                .map_err(storage_error)?;
        if current_fingerprint.as_deref() != Some(cutover.as_str()) {
            return Err(Error::InputConflict);
        }
        let historical = sqlx::query(
            "SELECT canonical_input,input_digest,choice_set,choice_set_digest \
                 FROM matrix_task_revisions WHERE tenant_id=$1 AND workspace_id=$2 \
                   AND task_id=$3 AND revision=$4",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(record.binding.task_id)
        .bind(record.binding.task_revision)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?
        .ok_or(Error::InputConflict)?;
        let input_json: serde_json::Value = historical
            .try_get("canonical_input")
            .map_err(storage_error)?;
        let input: EngineeringMatrixInput =
            serde_json::from_value(input_json.clone()).map_err(|_| Error::InputConflict)?;
        let choice_json: serde_json::Value =
            historical.try_get("choice_set").map_err(storage_error)?;
        let choice: EngineeringChoiceSet =
            serde_json::from_value(choice_json).map_err(|_| Error::InputConflict)?;
        let (verification, verified_epoch) = self
            .historical_matrix_verification_by_digest(
                workspace_id,
                record.binding.task_id,
                record.binding.task_revision,
                saved_digest,
            )
            .await?
            .ok_or(Error::InputConflict)?;
        let validated = evaluate_matrix_verification(
            &record.binding.task_id.to_string(),
            &record.binding.task_revision.to_string(),
            &input,
            &verification,
            verified_epoch,
        )
        .map_err(|_| Error::InputConflict)?;
        let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            record.binding.task_id.to_string(),
            record.binding.task_revision.to_string(),
            input.clone(),
        )
        .map_err(|_| Error::InputConflict)?;
        let composition = compose_independently_verified_owner_matrix(&reported, &validated)
            .map_err(|_| Error::InputConflict)?;
        let evaluation_digest =
            matrix_verified_evaluation_digest(&input, &composition, &choice, &validated)
                .map_err(|_| Error::InputConflict)?;
        if canonical_matrix_input_digest(&input_json)? != record.binding.input_digest
            || historical
                .try_get::<String, _>("input_digest")
                .map_err(storage_error)?
                != record.binding.input_digest
            || historical
                .try_get::<String, _>("choice_set_digest")
                .map_err(storage_error)?
                != record.binding.choice_set_digest
            || choice.choice_set_id != record.binding.choice_set_id
            || choice.version != record.binding.choice_set_version
            || choice.canonical_digest(&input)? != record.binding.choice_set_digest
            || evaluation_digest != record.binding.evaluation_digest
        {
            return Err(Error::InputConflict);
        }
        let eligibility = choice.validate(&input)?;
        record.validate_for(
            record.opportunity_id,
            record.dispatch_id,
            &record.binding,
            &record.provider_profile_ref,
            &record.model_configuration,
            &eligibility,
        )?;
        return self
            .insert_matrix_advice_receipt(workspace_id, record)
            .await;
    }
}
