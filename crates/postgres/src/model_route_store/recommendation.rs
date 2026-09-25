#[async_trait]
impl ModelRouteRecommendationStore for PgUnitOfWork {
    async fn validate_current(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<()> {
        let stored = self
            .by_request(prepared.workspace_id, &prepared.request_key)
            .await?
            .ok_or(Error::StaleContext)?;
        if stored != *prepared {
            return Err(Error::InputConflict);
        }
        current_preparation(self, prepared).await.map(|_| ())
    }
    async fn by_request(
        &mut self,
        workspace_id: Uuid,
        request_key: &str,
    ) -> Result<Option<PreparedModelRouteRecommendation>> {
        let tenant = self.tenant_id()?;
        let row: Option<Value> = sqlx::query_scalar(
            "SELECT prepared_payload FROM model_route_preparations \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_key=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(request_key)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode).transpose()
    }

    async fn load_basis(
        &mut self,
        workspace_id: Uuid,
        disposition_id: Uuid,
    ) -> Result<Option<ModelRouteRecommendationBasis>> {
        let tenant = self.tenant_id()?;
        let linked: Option<Uuid> = sqlx::query_scalar(
            "SELECT disposition_id FROM matrix_planning_selection_links \
             WHERE tenant_id=$1 AND workspace_id=$2 AND disposition_id=$3 LIMIT 1",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(disposition_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if linked.is_none() {
            return Ok(None);
        }
        config(self, workspace_id).await.map(Some)
    }

    async fn capture(
        &mut self,
        prepared: &PreparedModelRouteRecommendation,
    ) -> Result<PreparedModelRouteRecommendation> {
        if !self.is_read_write() || prepared.workspace_id.is_nil() {
            return Err(Error::Forbidden);
        }
        if let Some(saved) = self
            .by_request(prepared.workspace_id, &prepared.request_key)
            .await?
        {
            return if saved == *prepared {
                Ok(saved)
            } else {
                Err(Error::InputConflict)
            };
        }
        let (catalogue_digest, host_ref) = current_preparation(self, prepared).await?;
        let work = &prepared.work;
        let selection = &work.approved_matrix_selection;
        let link = &work.selection_link;
        let mode = config(self, prepared.workspace_id).await?;
        let tenant = self.tenant_id()?;
        sqlx::query(
            "INSERT INTO model_route_preparations \
             (tenant_id,workspace_id,request_key,disposition_id,candidate_set_id,caller_request_id, \
              work_node_id,work_node_revision,task_id,task_revision,advisory_mode, \
              advisory_config_revision,work_digest,catalogue_digest,host_capability_evidence_ref,prepared_payload) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(tenant)
        .bind(prepared.workspace_id)
        .bind(&prepared.request_key)
        .bind(selection.disposition_id)
        .bind(link.candidate_set_id)
        .bind(link.caller_request_id)
        .bind(link.mapped_work_node_id)
        .bind(link.mapped_work_node_revision)
        .bind(selection.task_id)
        .bind(selection.task_revision)
        .bind(mode.advisory_mode.as_str())
        .bind(mode.advisory_config_revision)
        .bind(work.digest()?)
        .bind(if catalogue_digest.is_empty() { None } else { Some(catalogue_digest) })
        .bind(host_ref)
        .bind(encode(prepared)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(write_error)?;
        Ok(prepared.clone())
    }
}
