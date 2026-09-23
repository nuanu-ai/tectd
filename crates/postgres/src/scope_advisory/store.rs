#[async_trait]
impl ScopeAdvisoryStore for PgUnitOfWork {
    async fn prepare_scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        record: &ScopeManifestRecord,
    ) -> Result<ScopeConstructorManifest> {
        let tenant = self.tenant_id()?;
        prepare_manifest(self.transaction()?, tenant, workspace_id, record, None).await
    }

    async fn prepare_authored_scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        record: &ScopeManifestRecord,
        authored_request_digest: &str,
    ) -> Result<ScopeConstructorManifest> {
        let tenant = self.tenant_id()?;
        prepare_manifest(
            self.transaction()?,
            tenant,
            workspace_id,
            record,
            Some(authored_request_digest),
        )
        .await
    }

    async fn scope_advisory_manifest(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<ScopeConstructorManifest>> {
        let tenant = self.tenant_id()?;
        load_manifest(
            self.transaction()?,
            tenant,
            workspace_id,
            opportunity_id,
            None,
        )
        .await
    }

    async fn scope_advisory_manifest_by_request_key(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<StoredScopeManifestRecord>> {
        let tenant = self.tenant_id()?;
        load_manifest_by_request_key(self.transaction()?, tenant, workspace_id, request_key).await
    }

    async fn guarded_scope_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<GuardedScopeAdvice>> {
        let tenant = self.tenant_id()?;
        Ok(load_advice(
            self.transaction()?,
            tenant,
            workspace_id,
            opportunity_id,
            None,
        )
        .await?
        .map(|(_, advice)| advice))
    }

    async fn persist_guarded_scope_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedScopeAdviceRecord,
    ) -> Result<GuardedScopeAdvice> {
        let tenant = self.tenant_id()?;
        persist_advice(self.transaction()?, tenant, workspace_id, record).await
    }

    async fn finalize_guarded_scope_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedScopeAdviceRecord,
    ) -> Result<GuardedScopeAdvice> {
        let tenant = self.tenant_id()?;
        finalize_advice(self.transaction()?, tenant, workspace_id, record).await
    }

    async fn invalidate_scope_advisory(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<()> {
        let tenant = self.tenant_id()?;
        invalidate_advice_opportunity(self.transaction()?, tenant, workspace_id, opportunity_id)
            .await
    }

    async fn finalize_prepared_scope_advisory_without_dispatch(
        &mut self,
        workspace_id: Uuid,
        record: &ScopePreparedAdvisoryDisposition,
    ) -> Result<()> {
        let tenant = self.tenant_id()?;
        finish_prepared_scope_advisory_without_dispatch(
            self.transaction()?,
            tenant,
            workspace_id,
            record,
        )
        .await
    }

    async fn cas_scope_advisory_disposition(
        &mut self,
        workspace_id: Uuid,
        record: ScopeDispositionRecord,
    ) -> Result<ScopeDispositionRevision> {
        let tenant = self.tenant_id()?;
        cas_disposition(self.transaction()?, tenant, workspace_id, record).await
    }

    async fn persist_scope_preservation_receipt(
        &mut self,
        workspace_id: Uuid,
        input: &ScopePreservationReceiptInput,
    ) -> Result<Uuid> {
        let tenant = self.tenant_id()?;
        persist_preservation(self.transaction()?, tenant, workspace_id, input).await
    }

    async fn link_scope_advisory_caller(
        &mut self,
        workspace_id: Uuid,
        input: &ScopeCallerLinkInput,
    ) -> Result<Uuid> {
        let tenant = self.tenant_id()?;
        persist_caller_link(self.transaction()?, tenant, workspace_id, input).await
    }

    async fn persist_scope_verifier_receipt(
        &mut self,
        workspace_id: Uuid,
        input: &ScopeVerifierReceiptInput,
    ) -> Result<Uuid> {
        let tenant = self.tenant_id()?;
        persist_verifier(self.transaction()?, tenant, workspace_id, input).await
    }

    async fn observe_selected_scope_save(
        &mut self,
        workspace_id: Uuid,
        request: &SelectedSaveObservationRequest,
    ) -> Result<SelectedSaveObservation> {
        let tenant = self.tenant_id()?;
        let actor = self.principal_id()?;
        observe_selected_save(self.transaction()?, tenant, workspace_id, actor, request, false).await
    }

    async fn independently_observe_selected_scope_save(
        &mut self,
        workspace_id: Uuid,
        request: &SelectedSaveObservationRequest,
    ) -> Result<SelectedSaveObservation> {
        let tenant = self.tenant_id()?;
        if self.principal_role()? != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        let actor = self.principal_id()?;
        observe_selected_save(self.transaction()?, tenant, workspace_id, actor, request, true).await
    }
}
