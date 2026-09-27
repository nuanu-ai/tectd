use super::*;

mod advice_validation;

#[async_trait]
impl MatrixDispositionStore for PgUnitOfWork {
    async fn matrix_disposition_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixDispositionRecord>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT d.disposition_id,d.request_id,d.actor_id,d.session_id,d.opportunity_id, \
                    d.task_id,d.matrix_task_revision,d.matrix_choice_set_digest,d.basis, \
                    d.advice_id,d.outcome,d.selected_choice_id,d.blocked_reason, \
                    r.input_digest,a.advice_digest \
             FROM advisory_matrix_disposition d \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)= \
               (d.tenant_id,d.workspace_id,d.task_id,d.matrix_task_revision) \
             LEFT JOIN advisory_matrix_advice a ON (a.tenant_id,a.workspace_id,a.advice_id)= \
               (d.tenant_id,d.workspace_id,d.advice_id) \
             WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.request_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode_disposition).transpose()
    }

    async fn record_matrix_disposition(
        &mut self,
        workspace_id: Uuid,
        actor_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixDisposition,
        current_advice: Option<&CurrentMatrixAdvice>,
        current_verification: Option<&MatrixDispositionVerification>,
    ) -> Result<MatrixDispositionRecord> {
        request.validate()?;
        if workspace_id.is_nil()
            || actor_id.is_nil()
            || session_id.is_nil()
            || self.principal_id()? != actor_id
        {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        // The runtime role cannot lock private host/principal rows. These
        // existing security-definer reads provide preflight; the INSERT trigger
        // repeats the full check with row locks until transaction commit.
        let active: bool = sqlx::query_scalar(
            "SELECT COALESCE(public.tect_dk_session_principal($1)=$2,false) \
                    AND public.tect_dk_is_owner($2) \
                    AND EXISTS(SELECT 1 FROM memberships m \
                      WHERE m.tenant_id=$3 AND m.workspace_id=$4 AND m.principal_id=$2)",
        )
        .bind(session_id)
        .bind(actor_id)
        .bind(tenant)
        .bind(workspace_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if !active {
            return Err(Error::Forbidden);
        }
        if let Some(prior) = self
            .matrix_disposition_by_request(workspace_id, request.request_id)
            .await?
        {
            return if same_request(&prior, actor_id, session_id, request) {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }

        let opportunity = sqlx::query(
            "SELECT work_item_kind,work_item_id,source_revision,matrix_task_revision, \
                    matrix_choice_set_digest,matrix_verification_digest,session_id,authorized_actor_id, \
                    capability,decision_point,config_revision,material_digest,state,primary_reason \
             FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
        )
        .bind(tenant).bind(workspace_id).bind(request.opportunity_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
        .ok_or(Error::NotFound)?;
        let revision = request.expected_task_revision;
        let state: String = opportunity.try_get("state").map_err(storage_error)?;
        if opportunity
            .try_get::<String, _>("work_item_kind")
            .map_err(storage_error)?
            != "matrix_task"
            || opportunity
                .try_get::<Option<Uuid>, _>("work_item_id")
                .map_err(storage_error)?
                != Some(request.task_id)
            || opportunity
                .try_get::<Option<String>, _>("source_revision")
                .map_err(storage_error)?
                .as_deref()
                != Some(revision.to_string().as_str())
            || opportunity
                .try_get::<Option<i64>, _>("matrix_task_revision")
                .map_err(storage_error)?
                != Some(revision)
            || opportunity
                .try_get::<Option<String>, _>("matrix_choice_set_digest")
                .map_err(storage_error)?
                != request.expected_choice_set_digest
            || opportunity
                .try_get::<Uuid, _>("session_id")
                .map_err(storage_error)?
                != session_id
            || opportunity
                .try_get::<Uuid, _>("authorized_actor_id")
                .map_err(storage_error)?
                != actor_id
            || opportunity
                .try_get::<String, _>("capability")
                .map_err(storage_error)?
                != "engineering_profile"
            || opportunity
                .try_get::<String, _>("decision_point")
                .map_err(storage_error)?
                != "engineering.profile.before_selection"
        {
            return Err(Error::StaleContext);
        }
        match request.basis {
            MatrixDispositionBasis::NoCall if state == "no_call" => {}
            MatrixDispositionBasis::Manual
                if matches!(state.as_str(), "no_call" | "failed" | "invalidated") => {}
            MatrixDispositionBasis::AfterAdvice if state == "advised" => {}
            _ => return Err(Error::StaleContext),
        }

        let current = self
            .lock_matrix_task(workspace_id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if current.revision != revision
            || current.input_digest != request.expected_input_digest
            || current.choice_set_digest != request.expected_choice_set_digest
        {
            return Err(Error::StaleRevision);
        }
        if let MatrixDispositionDecision::Selected { selected_choice_id } = &request.decision {
            let set = current.choice_set.as_ref().ok_or(Error::InvalidArguments)?;
            set.validate(&current.input)?;
            if !set
                .candidates
                .iter()
                .any(|candidate| candidate.candidate_id == *selected_choice_id)
            {
                return Err(Error::InvalidArguments);
            }
            let token = current_verification.ok_or(Error::StaleContext)?;
            let now: i64 = sqlx::query_scalar(
                "SELECT FLOOR(EXTRACT(EPOCH FROM pg_catalog.clock_timestamp()))::bigint",
            )
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            let evaluation = match token {
                MatrixDispositionVerification::LegacyV1 { verification, .. } => {
                    // Preserve the historical V1 preimage and its saved-record authority.
                    let source = self
                        .matrix_task_source(workspace_id, request.task_id)
                        .await?
                        .ok_or(Error::StaleContext)?;
                    if source.revision != current || source.requirements_binding.is_some() {
                        return Err(Error::StaleContext);
                    }
                    let saved = self
                        .matrix_verification_for_revision(
                            workspace_id,
                            request.task_id,
                            revision,
                            &request.expected_input_digest,
                        )
                        .await?
                        .ok_or(Error::StaleContext)?;
                    if saved.digest != verification.record_digest()
                        || saved.owner_principal != current.recorded_by_principal_id.to_string()
                        || saved.verifier_principal == saved.owner_principal
                    {
                        return Err(Error::StaleContext);
                    }
                    let validated = evaluate_matrix_verification(
                        &request.task_id.to_string(),
                        &revision.to_string(),
                        &current.input,
                        &saved,
                        now,
                    )
                    .map_err(|_| Error::StaleContext)?;
                    let reported =
                        OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
                            request.task_id.to_string(),
                            revision.to_string(),
                            current.input.clone(),
                        )
                        .map_err(|_| Error::StaleContext)?;
                    let composition =
                        compose_independently_verified_owner_matrix(&reported, &validated)
                            .map_err(|_| Error::StaleContext)?;
                    matrix_verified_disposition_digest(
                        &current.input,
                        &composition,
                        set,
                        &validated,
                    )
                    .map_err(|_| Error::StaleContext)?
                }
                MatrixDispositionVerification::ContextV2 {
                    binding,
                    composition: expected,
                    record,
                } => {
                    let source = self
                        .matrix_task_source(workspace_id, request.task_id)
                        .await?
                        .ok_or(Error::StaleContext)?;
                    if source.revision != current
                        || source.requirements_binding.as_ref() != Some(binding)
                        || binding.authority_schema != MATRIX_REQUIREMENTS_SCHEMA
                    {
                        return Err(Error::StaleContext);
                    }
                    let lineage = self
                        .matrix_requirements_lineage(
                            workspace_id,
                            actor_id,
                            &binding.locator,
                            false,
                        )
                        .await
                        .map_err(|_| Error::StaleContext)?;
                    for anchor in &lineage {
                        self.lock_matrix_requirements_head(workspace_id, *anchor)
                            .await
                            .map_err(|_| Error::StaleContext)?;
                    }
                    let frozen = self
                        .frozen_matrix_requirements_by_id(workspace_id, binding.snapshot_id)
                        .await?
                        .ok_or(Error::StaleContext)?;
                    if lineage.last().copied() != Some(frozen.anchor)
                        || frozen.effective.schema() != binding.authority_schema
                        || frozen.effective.semantic_digest() != binding.semantic_digest
                    {
                        return Err(Error::StaleContext);
                    }
                    let revisions = self
                        .matrix_requirements_revisions(workspace_id, &lineage)
                        .await
                        .map_err(|_| Error::StaleContext)?;
                    let effective = resolve_matrix_requirements(
                        &lineage,
                        &revisions,
                        MATRIX_REQUIREMENTS_SCHEMA,
                    )
                    .map_err(|_| Error::StaleContext)?;
                    if effective.semantic_digest() != binding.semantic_digest
                        || effective != frozen.effective
                    {
                        return Err(Error::StaleContext);
                    }
                    let saved = self
                        .context_matrix_verification_for_revision(
                            workspace_id,
                            request.task_id,
                            revision,
                            &current.input_digest,
                            binding.snapshot_id,
                        )
                        .await?
                        .ok_or(Error::StaleContext)?;
                    if saved != **record
                        || saved.owner_principal != current.recorded_by_principal_id.to_string()
                        || saved.verifier_principal == saved.owner_principal
                        || saved.input_digest != current.input_digest
                        || saved.frozen_snapshot_id != binding.snapshot_id.to_string()
                        || saved.authority_schema != binding.authority_schema
                        || saved.requirements_semantic_digest != binding.semantic_digest
                    {
                        return Err(Error::StaleContext);
                    }
                    let validated = evaluate_context_matrix_verification(
                        &request.task_id.to_string(),
                        &revision.to_string(),
                        &binding.snapshot_id.to_string(),
                        &current.input,
                        &effective,
                        &saved,
                        now,
                    )
                    .map_err(|_| Error::StaleContext)?;
                    let composition = compose_confirmed_requirements_matrix(
                        &request.task_id.to_string(),
                        &revision.to_string(),
                        &binding.snapshot_id.to_string(),
                        &current.input,
                        &effective,
                        &validated,
                        now,
                    )
                    .map_err(|_| Error::StaleContext)?;
                    if &composition != expected.as_ref() {
                        return Err(Error::StaleContext);
                    }
                    context_matrix_verified_evaluation_digest(
                        &current.input,
                        &composition,
                        set,
                        &saved,
                    )
                    .map_err(|_| Error::StaleContext)?
                }
            };
            let reason: String = opportunity
                .try_get("primary_reason")
                .map_err(storage_error)?;
            let captured_verification: Option<String> = opportunity
                .try_get("matrix_verification_digest")
                .map_err(storage_error)?;
            let captured_material: String = opportunity
                .try_get("material_digest")
                .map_err(storage_error)?;
            if !selected_receipt_matches(
                &reason,
                captured_verification.as_deref(),
                token.record_digest(),
                &captured_material,
                &evaluation,
            ) {
                return Err(Error::StaleContext);
            }
        } else if current_verification.is_some() {
            return Err(Error::StaleContext);
        }

        self.validate_disposition_advice(
            workspace_id,
            request,
            current_advice,
            current_verification,
            &opportunity,
            &current,
        )
        .await?;
        let (outcome, selected, blocked) = match &request.decision {
            MatrixDispositionDecision::Selected { selected_choice_id } => {
                ("selected", Some(selected_choice_id.as_str()), None)
            }
            MatrixDispositionDecision::Blocked { blocked_reason } => {
                ("blocked", None, Some(blocked_reason.as_str()))
            }
        };
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO advisory_matrix_disposition \
               (tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision, \
                matrix_choice_set_digest,request_id,actor_id,session_id,basis,advice_id, \
                outcome,selected_choice_id,blocked_reason) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) \
             ON CONFLICT DO NOTHING RETURNING disposition_id",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request.opportunity_id)
        .bind(request.task_id)
        .bind(revision)
        .bind(&request.expected_choice_set_digest)
        .bind(request.request_id)
        .bind(actor_id)
        .bind(session_id)
        .bind(request.basis.as_str())
        .bind(request.advice_id)
        .bind(outcome)
        .bind(selected)
        .bind(blocked)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(disposition_write_error)?;
        match inserted {
            Some(disposition_id) => Ok(MatrixDispositionRecord {
                disposition_id,
                request: request.clone(),
                recorded_by_principal_id: actor_id,
                recorded_by_session_id: session_id,
                material_digest: request.material_digest()?,
            }),
            None => {
                self.disposition_retry_or_error(
                    workspace_id,
                    actor_id,
                    session_id,
                    request,
                    Error::InputConflict,
                )
                .await
            }
        }
    }
}
