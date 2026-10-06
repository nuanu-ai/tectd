#[async_trait]
impl AdvisoryStore for PgUnitOfWork {
    async fn finalize_pipeline_advisory_without_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatch,
    ) -> Result<AdvisoryOpportunity> {
        let tenant = self.tenant_id()?;
        let actor = self.principal_id()?;
        let opportunity = opportunity_by_id(
            self.transaction()?, tenant, workspace_id, opportunity_id, true,
        ).await?;
        if !pipeline_receipt_family(
            opportunity.capability, opportunity.decision_point, &opportunity.target_kind,
        ) || opportunity.target_id.is_none()
            || opportunity.authorized_actor_id != actor
        {
            return Err(Error::Forbidden);
        }
        finalize_interpreted_advisory_response(
            self.transaction()?, tenant, workspace_id, opportunity_id,
            expected_config_revision, dispatch, false, false,
        ).await
    }

    async fn advisory_dispatch_receipt(
        &mut self,
        workspace: Uuid,
        opportunity: Uuid,
    ) -> Result<Option<tect_application::StoredAdvisoryProviderReceipt>> {
        let tenant = self.tenant_id()?;
        let actor = self.principal_id()?;
        load_provider_receipt_for_actor(self.transaction()?, tenant, workspace, actor, opportunity)
            .await
    }

    async fn consume_advisory_budget(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryBudgetConsumption> {
        let tenant = self.tenant_id()?;
        consume_budget(self.transaction()?, tenant, workspace_id, dispatch_id).await
    }

    async fn start_verified_matrix_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
        verification_current: bool,
    ) -> Result<AdvisoryDispatchStart> {
        let tenant = self.tenant_id()?;
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| Error::BudgetPolicyInvalid)?
                .as_millis(),
        )
        .map_err(|_| Error::BudgetPolicyInvalid)?;
        let policy = self.authorized_budget_policy(workspace_id, now).await?;
        start_matrix_dispatch(
            self.transaction()?,
            tenant,
            workspace_id,
            dispatch_id,
            Some(verification_current),
            policy.as_ref(),
            None,
        )
        .await
    }

    async fn matrix_dispatch_for_recovery(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        actor_id: Uuid,
        opportunity_id: Uuid,
        dispatch_id: Option<Uuid>,
    ) -> Result<tect_application::StoredMatrixDispatch> {
        let tenant = self.tenant_id()?;
        matrix_dispatch_for_recovery(
            self.transaction()?,
            tenant,
            workspace_id,
            actor_id,
            opportunity_id,
            dispatch_id,
        )
        .await
    }

    async fn advisory_config(&mut self, workspace_id: Uuid) -> Result<WorkspaceAdvisoryConfig> {
        let tenant = self.tenant_id()?;
        config(self.transaction()?, tenant, workspace_id).await
    }

    async fn materialize_advisory_config(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
    ) -> Result<WorkspaceAdvisoryConfig> {
        let tenant = self.tenant_id()?;
        materialize_config(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            session_id,
        )
        .await
    }

    async fn configure_advisory(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &ConfigureWorkspaceAdvisory,
    ) -> Result<WorkspaceAdvisoryConfig> {
        let tenant = self.tenant_id()?;
        configure(
            self.transaction()?,
            tenant,
            workspace_id,
            principal_id,
            session_id,
            request,
        )
        .await
    }

    async fn capture_advisory_opportunity(
        &mut self,
        workspace_id: Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<AdvisoryOpportunity> {
        let tenant = self.tenant_id()?;
        capture_opportunity(self.transaction()?, tenant, workspace_id, input).await
    }

    async fn advisory_opportunity_for_dispatch(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunity> {
        let tenant = self.tenant_id()?;
        opportunity_by_id(
            self.transaction()?,
            tenant,
            workspace_id,
            opportunity_id,
            true,
        )
        .await
    }

    async fn advisory_opportunity_by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<AdvisoryOpportunity>> {
        let tenant = self.tenant_id()?;
        opportunity_by_request_key(self.transaction()?, tenant, workspace_id, request_key).await
    }

    async fn authorize_advisory_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        expected_config_revision: i64,
        input: &AdvisoryDispatchAuthorization,
    ) -> Result<AdvisoryDispatch> {
        let tenant = self.tenant_id()?;
        authorize_dispatch(
            self.transaction()?,
            tenant,
            workspace_id,
            expected_config_revision,
            input,
        )
        .await
    }

    async fn start_signed_scope_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchStart> {
        let tenant = self.tenant_id()?;
        let prepared =
            prepare_bound_scope_dispatch(self.transaction()?, tenant, workspace_id, dispatch_id)
                .await?;
        if matches!(prepared.disposition, DispatchStartDisposition::NonSend) {
            let reservation = if matches!(
                dispatch_state(&prepared.row.state)?,
                AdvisoryDispatchState::Sending | AdvisoryDispatchState::Sealed
            ) {
                // Historical None remains non-send, not a successful signed new start.
                reservation_for_dispatch(self.transaction()?, tenant, workspace_id, dispatch_id)
                    .await?
            } else {
                None
            };
            return finish_dispatch_start(
                self.transaction()?,
                tenant,
                workspace_id,
                dispatch_id,
                prepared,
                reservation,
            )
            .await;
        }
        // Route/currentness locks remain held while the row is Authorized.
        // Reserve before Sending; every failure precedes the App start commit.
        let now = i64::try_from(
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map_err(|_| Error::BudgetPolicyInvalid)?
                .as_millis(),
        )
        .map_err(|_| Error::BudgetPolicyInvalid)?;
        let policy = self.authorized_budget_policy(workspace_id, now).await?;
        let reservation = reserve_before_dispatch(
            self.transaction()?,
            tenant,
            workspace_id,
            &prepared.row,
            policy.as_ref(),
            Some(&prepared.row.configuration_snapshot),
            None,
        )
        .await?;
        finish_dispatch_start(
            self.transaction()?,
            tenant,
            workspace_id,
            dispatch_id,
            prepared,
            Some(reservation),
        )
        .await
    }

    async fn start_advisory_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchStart> {
        let tenant = self.tenant_id()?;
        start_dispatch(self.transaction()?, tenant, workspace_id, dispatch_id).await
    }

    async fn seal_advisory_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        seal: &AdvisoryDispatchSeal,
    ) -> Result<AdvisoryDispatch> {
        let tenant = self.tenant_id()?;
        seal_dispatch(self.transaction()?, tenant, workspace_id, seal).await
    }

    async fn cancel_advisory_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        dispatch_id: Uuid,
    ) -> Result<AdvisoryDispatchCancellation> {
        let tenant = self.tenant_id()?;
        cancel_dispatch(self.transaction()?, tenant, workspace_id, dispatch_id).await
    }

    async fn reconcile_advisory_dispatch(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        evidence: &AdvisoryReconciliationEvidence,
    ) -> Result<AdvisoryDispatch> {
        let tenant = self.tenant_id()?;
        reconcile_dispatch(self.transaction()?, tenant, workspace_id, evidence).await
    }

    async fn finalize_advisory_opportunity(
        &mut self,
        _capability: &tect_application::AdvisoryLifecycleCapability,
        workspace_id: Uuid,
        opportunity_id: Uuid,
        expected_config_revision: i64,
        dispatch: &AdvisoryDispatch,
    ) -> Result<AdvisoryOpportunity> {
        let tenant = self.tenant_id()?;
        let opportunity = opportunity_by_id(
            self.transaction()?, tenant, workspace_id, opportunity_id, false,
        ).await?;
        if pipeline_receipt_family(
            opportunity.capability, opportunity.decision_point, &opportunity.target_kind,
        ) && opportunity.target_id.is_some()
        {
            let actor = self.principal_id()?;
            if opportunity.authorized_actor_id != actor {
                return Err(Error::Forbidden);
            }
            return finalize_interpreted_advisory_response(
                self.transaction()?, tenant, workspace_id, opportunity_id,
                expected_config_revision, dispatch, false, true,
            ).await;
        }
        if opportunity.capability == AdvisoryCapability::PipelineRecommendation {
            return Err(Error::Forbidden);
        }
        finalize_opportunity(
            self.transaction()?,
            tenant,
            workspace_id,
            opportunity_id,
            expected_config_revision,
            dispatch,
        )
        .await
    }

    async fn advisory_audit(
        &mut self,
        workspace_id: Uuid,
        scope_id: Option<Uuid>,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage> {
        let tenant = self.tenant_id()?;
        audit(
            self.transaction()?,
            tenant,
            workspace_id,
            scope_id,
            None,
            query,
        )
        .await
    }

    async fn advisory_opportunity_detail(
        &mut self,
        workspace_id: Uuid,
        scope_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunityDetail> {
        let tenant = self.tenant_id()?;
        opportunity_detail(
            self.transaction()?,
            tenant,
            workspace_id,
            Some(scope_id),
            None,
            opportunity_id,
        )
        .await
    }

    async fn advisory_candidate_set_exists(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<bool> {
        let tenant = self.tenant_id()?;
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)")
            .bind(tenant).bind(workspace_id).bind(candidate_set_id)
            .fetch_one(&mut **self.transaction()?).await.map_err(storage_error)
    }

    async fn candidate_advisory_audit(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        query: &AdvisoryAuditQuery,
    ) -> Result<AdvisoryAuditPage> {
        let tenant = self.tenant_id()?;
        audit(
            self.transaction()?,
            tenant,
            workspace_id,
            None,
            Some(candidate_set_id),
            query,
        )
        .await
    }

    async fn candidate_advisory_opportunity_detail(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<AdvisoryOpportunityDetail> {
        let tenant = self.tenant_id()?;
        opportunity_detail(
            self.transaction()?,
            tenant,
            workspace_id,
            None,
            Some(candidate_set_id),
            opportunity_id,
        )
        .await
    }
}
