use super::*;

impl WorkspaceService {
    /// Read a saved Matrix advisory receipt, including historical no-call
    /// decisions, for an authenticated member of the receipt's workspace.
    pub async fn get_engineering_advisory(
        &self,
        context: &RequestContext,
        task_id: Uuid,
        request_key: &str,
    ) -> Result<EngineeringAdvisoryRead> {
        if task_id.is_nil() || !valid_advisory_request_key(request_key) {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let receipt = tx
            .advisory_opportunity_by_request(workspace.id, request_key)
            .await?
            .ok_or(Error::NotFound)?;
        if !matrix_advisory_receipt_matches(&receipt, workspace.id, task_id, request_key) {
            return Err(Error::NotFound);
        }
        let advice = if receipt.state == AdvisoryOpportunityState::Advised {
            tx.guarded_matrix_advice(workspace.id, receipt.id).await?
        } else {
            None
        };
        let current_advice = if let Some(advice) = advice {
            let source = tx.matrix_task_source(workspace.id, task_id).await?;
            let config = tx.advisory_config(workspace.id).await?;
            let fresh = if let Some(source) = source
                .filter(|source| source.revision.revision == advice.record.binding.task_revision)
            {
                if let (
                    Some(binding),
                    crate::MatrixVerificationAuthority::ContextV2 {
                        snapshot_id,
                        authority_schema,
                        semantic_digest,
                        ..
                    },
                ) = (
                    source.requirements_binding.as_ref(),
                    &advice.record.binding.verification,
                ) {
                    if binding.snapshot_id != *snapshot_id
                        || binding.authority_schema != *authority_schema
                        || binding.semantic_digest != *semantic_digest
                    {
                        None
                    } else {
                        let context = if let Some(store) = tx.matrix_requirements_context_store() {
                            crate::matrix_verification::load_bound_matrix_context(
                                store,
                                workspace.id,
                                identity.principal_id,
                                binding,
                            )
                            .await
                            .ok()
                        } else {
                            None
                        };
                        if let Some(context) = context {
                            let verified = binding::compose_bound_revision_with_verification(
                                tx.context_matrix_verification_store(),
                                self.matrix_evidence_validator.as_ref(),
                                workspace.id,
                                &source.revision,
                                *snapshot_id,
                                &context,
                                crate::matrix_verification::current_epoch_seconds()?,
                            )
                            .await
                            .ok()
                            .flatten();
                            verified.and_then(|(composition, record)| {
                                crate::MatrixProviderRequest::new_context_verified(
                                    source.revision,
                                    &composition,
                                    &record,
                                    *snapshot_id,
                                    advice.record.provider_profile_ref.clone(),
                                    advice.record.model_configuration.clone(),
                                )
                                .ok()
                            })
                        } else {
                            None
                        }
                    }
                } else {
                    None
                }
            } else {
                None
            };
            current_public_matrix_advice(
                &receipt,
                &advice,
                &config,
                fresh.as_ref().map(|request| request.binding()),
            )
        } else {
            None
        };
        tx.commit().await?;
        Ok(EngineeringAdvisoryRead {
            opportunity: receipt,
            current_advice,
        })
    }

    pub async fn request_engineering_advisory(
        &self,
        context: &RequestContext,
        request: &RequestEngineeringAdvisory,
    ) -> Result<AdvisoryOpportunity> {
        if request.task_id.is_nil()
            || request.expected_task_revision < 1
            || !valid_advisory_request_key(&request.request_key)
        {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let existing = tx
            .advisory_opportunity_by_request(workspace.id, &request.request_key)
            .await?;
        if let Some(existing) = existing {
            if !matrix_advisory_replay_matches(
                &existing,
                request,
                session.id,
                identity.principal_id,
            ) {
                return Err(Error::InputConflict);
            }
            let saved = if matches!(
                existing.state,
                AdvisoryOpportunityState::Prepared | AdvisoryOpportunityState::AwaitingResponse
            ) {
                Some(
                    tx.matrix_dispatch_for_recovery(
                        &crate::AdvisoryLifecycleCapability::internal(),
                        workspace.id,
                        identity.principal_id,
                        existing.id,
                        None,
                    )
                    .await?,
                )
            } else {
                None
            };
            tx.commit().await?;
            return match saved {
                Some(saved) => {
                    self.recover_matrix_advisory(context, workspace.id, existing, saved)
                        .await
                }
                None => Ok(existing),
            };
        }
        let source = tx
            .matrix_task_source(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        // Bound task recording locks requirements before appending the task;
        // keep that order here so concurrent edits cannot deadlock this gate.
        let bound_context = if let Some(binding) = source.requirements_binding.as_ref() {
            Some(
                if let Some(store) = tx.matrix_requirements_context_store() {
                    crate::matrix_verification::lock_and_load_bound_matrix_context(
                        store,
                        workspace.id,
                        identity.principal_id,
                        binding,
                    )
                    .await
                } else {
                    Err(crate::matrix_verification::BoundContextFailure::CurrentUnresolved)
                },
            )
        } else {
            None
        };
        let revision = tx
            .lock_matrix_task(workspace.id, request.task_id)
            .await?
            .ok_or(Error::NotFound)?;
        if source.revision != revision {
            return Err(Error::StaleRevision);
        }
        if revision.revision != request.expected_task_revision {
            return Err(Error::StaleRevision);
        }
        let config = tx.advisory_config(workspace.id).await?;
        let mut input = matrix_advisory_opportunity_input(
            &revision,
            request,
            &config,
            session.id,
            identity.principal_id,
        )?;
        let mut provider_request = None;
        if matches!(
            input.primary_reason,
            AdvisoryReason::MatrixSourceUnverified
                | AdvisoryReason::MatrixEvidenceUnresolved
                | AdvisoryReason::CapabilityUnavailable
        ) {
            if let (Some(binding), Some(resolved)) =
                (source.requirements_binding.as_ref(), bound_context)
            {
                match resolved {
                    Err(failure) => input.primary_reason = match failure {
                        crate::matrix_verification::BoundContextFailure::SnapshotMissing => AdvisoryReason::MatrixSnapshotMissing,
                        crate::matrix_verification::BoundContextFailure::BindingMismatch => AdvisoryReason::MatrixBindingMismatch,
                        crate::matrix_verification::BoundContextFailure::CurrentUnresolved => AdvisoryReason::MatrixContextUnresolved,
                        crate::matrix_verification::BoundContextFailure::CurrentStale => AdvisoryReason::MatrixContextStale,
                        crate::matrix_verification::BoundContextFailure::AuthoritySchemaUnsupported => AdvisoryReason::MatrixAuthoritySchemaUnsupported,
                    },
                    Ok(context) => {
                        let verified = binding::compose_bound_revision_with_verification(
                            tx.context_matrix_verification_store(),
                            self.matrix_evidence_validator.as_ref(),
                            workspace.id, &revision, binding.snapshot_id, &context,
                            crate::matrix_verification::current_epoch_seconds()?,
                        ).await?;
                        if let Some((composition, record)) = verified {
                            if let (Some(profile), Some(model)) = (
                                config.provider_profile_ref.clone(), config.model_configuration.clone(),
                            ) {
                                let prepared = crate::MatrixProviderRequest::new_context_verified(
                                    revision.clone(), &composition, &record, binding.snapshot_id,
                                    profile, model,
                                )?;
                                input.primary_reason = AdvisoryReason::CapabilityUnavailable;
                                provider_request = Some(prepared);
                            } else { input.primary_reason = AdvisoryReason::ProviderUnconfigured; }
                        } else { input.primary_reason = AdvisoryReason::MatrixOperatingEvidenceUnresolved; }
                    }
                }
            } else {
                input.primary_reason = AdvisoryReason::MatrixTaskUnbound;
            }
        }
        if let Some(binding) = source.requirements_binding.as_ref() {
            let no_call_material = serde_json::to_vec(&(
                "tect.context-matrix-advisory-opportunity/1",
                &input.material_digest,
                binding.snapshot_id,
                &binding.authority_schema,
                &binding.semantic_digest,
                input.primary_reason.as_str(),
            ))
            .map_err(|_| Error::InternalInvariant)?;
            input.material_digest = format!("{:x}", Sha256::digest(no_call_material));
        }
        input.validate()?;
        let verified_policy = if provider_request.is_some() {
            crate::matrix_advisory_capture::lookup_verified_matrix_budget(
                tx.advisory_budget_policy_store(),
                workspace.id,
            )
            .await?
        } else {
            None
        };
        let prepared = crate::matrix_advisory_capture::prepare_eligible_matrix_opportunity(
            &mut input,
            provider_request.as_ref(),
            workspace.id,
            identity.principal_id,
            self.matrix_advice_provider.as_ref(),
            self.matrix_budget.as_ref(),
            verified_policy.as_ref(),
        )
        .await?;
        let opportunity = tx
            .capture_advisory_opportunity(workspace.id, &input)
            .await?;
        let crate::matrix_advisory_capture::PreparedMatrixOpportunity::Authorized {
            prepared,
            authorization: budget,
        } = prepared
        else {
            tx.commit().await?;
            return Ok(opportunity);
        };
        let provider_request = provider_request.ok_or(Error::InternalInvariant)?;
        let prepared = *prepared;
        let authorization = crate::matrix_advisory_dispatch::authorize_prepared_matrix(
            Uuid::new_v4(),
            opportunity.id,
            &opportunity,
            &prepared,
            &budget,
            verified_policy.as_ref().ok_or(Error::InternalInvariant)?,
        )?;
        let lifecycle = crate::AdvisoryLifecycleCapability::internal();
        tx.authorize_advisory_dispatch(&lifecycle, workspace.id, config.revision, &authorization)
            .await?;
        tx.commit().await?;
        self.dispatch_prepared_matrix_advisory(
            context,
            workspace.id,
            crate::matrix_advisory_dispatch::PreparedMatrixDispatch {
                opportunity,
                config_revision: config.revision,
                authorization,
                provider_request,
                prepared,
            },
        )
        .await
    }
}
