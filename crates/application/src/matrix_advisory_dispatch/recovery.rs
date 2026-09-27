use super::*;

mod execute;

pub(crate) fn reconstruct_historical_legacy_request(
    revision: &crate::MatrixTaskRevision,
    record: &tect_domain::MatrixVerificationRecord,
    verified_epoch: i64,
    expected: &crate::MatrixProviderBinding,
    profile: tect_domain::AdvisoryProviderProfileRef,
    model: tect_domain::AdvisoryModelConfiguration,
) -> Result<Option<MatrixProviderRequest>> {
    if record.digest != expected.verification.digest().ok_or(Error::InputConflict)?
        || record.input_digest != revision.input_digest
        || record.owner_principal != revision.recorded_by_principal_id.to_string()
    {
        return Ok(None);
    }
    let Ok(validated) = tect_domain::evaluate_matrix_verification(
        &revision.task_id.to_string(),
        &revision.revision.to_string(),
        &revision.input,
        record,
        verified_epoch,
    ) else {
        return Ok(None);
    };
    let reported = tect_domain::OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        revision.task_id.to_string(),
        revision.revision.to_string(),
        revision.input.clone(),
    )?;
    let composition =
        tect_domain::compose_independently_verified_owner_matrix(&reported, &validated)?;
    let verification = crate::RevalidatedMatrixVerification::from_revalidated(validated);
    let request = MatrixProviderRequest::new_verified(
        revision.clone(),
        composition,
        &verification,
        profile,
        model,
    )?;
    Ok((request.binding() == expected).then_some(request))
}

impl WorkspaceService {
    pub(super) async fn current_verified_matrix_budget(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
    ) -> Result<Option<tect_domain::AdvisoryBudgetPolicy>> {
        let (mut read, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = read
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if Self::validate_binding(&mut *read, context, &identity, &session)
            .await?
            .id
            != workspace_id
        {
            return Err(Error::InputConflict);
        }
        let policy = crate::matrix_advisory_capture::lookup_verified_matrix_budget(
            read.advisory_budget_policy_store(),
            workspace_id,
        )
        .await?;
        read.commit().await?;
        Ok(policy)
    }

    pub(super) async fn cancel_stale_authorized_matrix_dispatch(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryOpportunity> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if Self::validate_binding(&mut *tx, context, &identity, &session)
            .await?
            .id
            != workspace_id
        {
            return Err(Error::InputConflict);
        }
        // The store rechecks the dispatch, configuration, and task under its
        // write locks; false can only cancel an Authorized, not-sent attempt.
        let started = tx
            .start_verified_matrix_dispatch(
                &AdvisoryLifecycleCapability::internal(),
                workspace_id,
                dispatch_id,
                false,
            )
            .await?;
        if started.should_send || started.dispatch.opportunity_id != opportunity_id {
            return Err(Error::InputConflict);
        }
        let terminal = tx
            .advisory_opportunity_for_dispatch(workspace_id, opportunity_id)
            .await?;
        tx.commit().await?;
        Ok(terminal)
    }

    pub(super) async fn current_saved_matrix_request(
        &self,
        context: &RequestContext,
        workspace_id: Uuid,
        saved: &StoredMatrixDispatch,
        allow_legacy_reconciliation: bool,
    ) -> Result<Option<MatrixProviderRequest>> {
        let (mut read, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = read
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        if Self::validate_binding(&mut *read, context, &identity, &session)
            .await?
            .id
            != workspace_id
        {
            return Err(Error::InputConflict);
        }
        let source = read
            .matrix_task_source(workspace_id, saved.binding.task_id)
            .await?;
        let Some(source) =
            source.filter(|source| source.revision.revision == saved.binding.task_revision)
        else {
            read.commit().await?;
            return Ok(None);
        };
        if allow_legacy_reconciliation
            && source.requirements_binding.is_none()
            && matches!(
                saved.binding.verification,
                crate::MatrixVerificationAuthority::LegacyV1 { .. }
            )
        {
            // Historical V1 may complete an already-sent operation. This path
            // is never used by the Authorized/unsent recovery window.
            let digest = saved
                .binding
                .verification
                .digest()
                .ok_or(Error::InputConflict)?;
            let historical = if let Some(store) = read.matrix_verification_store() {
                store
                    .historical_matrix_verification_by_digest(
                        workspace_id,
                        source.revision.task_id,
                        source.revision.revision,
                        digest,
                    )
                    .await?
            } else {
                None
            };
            read.commit().await?;
            let Some((record, verified_epoch)) = historical else {
                return Ok(None);
            };
            if record.digest != digest {
                return Ok(None);
            }
            return reconstruct_historical_legacy_request(
                &source.revision,
                &record,
                verified_epoch,
                &saved.binding,
                saved.provider_profile_ref.clone(),
                saved.model_configuration.clone(),
            );
        }
        let crate::MatrixVerificationAuthority::ContextV2 {
            snapshot_id,
            authority_schema,
            semantic_digest,
            ..
        } = &saved.binding.verification
        else {
            read.commit().await?;
            return Ok(None);
        };
        let Some(binding) = source.requirements_binding.as_ref().filter(|binding| {
            binding.snapshot_id == *snapshot_id
                && binding.authority_schema == *authority_schema
                && binding.semantic_digest == *semantic_digest
        }) else {
            read.commit().await?;
            return Ok(None);
        };
        let context = if let Some(store) = read.matrix_requirements_context_store() {
            crate::matrix_verification::load_bound_matrix_context(
                store,
                workspace_id,
                identity.principal_id,
                binding,
            )
            .await
            .ok()
        } else {
            None
        };
        let Some(context) = context else {
            read.commit().await?;
            return Ok(None);
        };
        let verified = crate::matrix_tasks::compose_bound_revision_with_verification(
            read.context_matrix_verification_store(),
            self.matrix_evidence_validator.as_ref(),
            workspace_id,
            &source.revision,
            *snapshot_id,
            &context,
            crate::matrix_verification::current_epoch_seconds()?,
        )
        .await?;
        read.commit().await?;
        let Some((composition, record)) = verified else {
            return Ok(None);
        };
        let Ok(request) = MatrixProviderRequest::new_context_verified(
            source.revision,
            &composition,
            &record,
            *snapshot_id,
            saved.provider_profile_ref.clone(),
            saved.model_configuration.clone(),
        ) else {
            return Ok(None);
        };
        Ok((request.binding() == &saved.binding).then_some(request))
    }
}
