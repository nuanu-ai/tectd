async fn audit_links(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    opportunities: &mut [AdvisoryAuditOpportunity],
) -> Result<()> {
    if opportunities.is_empty() {
        return Ok(());
    }
    let ids = opportunities.iter().map(|item| item.id).collect::<Vec<_>>();
    let links: Vec<AuditLinksRow> = sqlx::query_as(
        "SELECT o.id AS opportunity_id,a.advice_id AS guarded_advice_digest,d.disposition_id,\
                p.receipt_id AS preservation_receipt_id,p.status AS preservation_status,\
                c.caller_request_id AS caller_receipt_id,c.link_id AS caller_link_id,\
                v.receipt_id AS verifier_receipt_id,\
                obs.observation_id,obs.target_revision AS observation_target_revision,\
                obs.status AS observation_status,obs.reason_codes AS observation_reason_codes,\
                obs.evidence_digest AS observation_evidence_digest,obs.qualification AS observation_qualification \
         FROM advisory_opportunity o \
         LEFT JOIN advisory_scope_advice a ON a.tenant_id=o.tenant_id AND a.workspace_id=o.workspace_id AND a.opportunity_id=o.id \
         LEFT JOIN LATERAL (SELECT disposition_id FROM advisory_scope_disposition \
             WHERE tenant_id=o.tenant_id AND workspace_id=o.workspace_id AND opportunity_id=o.id \
             ORDER BY revision DESC,disposition_id DESC LIMIT 1) d ON true \
         LEFT JOIN LATERAL (SELECT link_id,caller_request_id,preservation_receipt_id FROM advisory_scope_caller_link \
             WHERE tenant_id=o.tenant_id AND workspace_id=o.workspace_id AND opportunity_id=o.id \
               AND disposition_id=d.disposition_id \
             ORDER BY created_at DESC,link_id DESC LIMIT 1) c ON true \
         LEFT JOIN LATERAL (SELECT receipt_id,status FROM advisory_scope_preservation_receipt \
             WHERE tenant_id=o.tenant_id AND workspace_id=o.workspace_id AND opportunity_id=o.id \
               AND disposition_id=d.disposition_id \
             ORDER BY (receipt_id=c.preservation_receipt_id) DESC,created_at DESC,receipt_id DESC LIMIT 1) p ON true \
         LEFT JOIN LATERAL (SELECT receipt_id FROM advisory_scope_verifier_receipt \
             WHERE tenant_id=o.tenant_id AND workspace_id=o.workspace_id AND opportunity_id=o.id \
               AND caller_link_id=c.link_id \
             ORDER BY created_at DESC,receipt_id DESC LIMIT 1) v ON true \
         LEFT JOIN LATERAL (SELECT observation_id,target_revision,status,reason_codes,evidence_digest,qualification \
             FROM advisory_scope_selected_save_observation \
             WHERE tenant_id=o.tenant_id AND workspace_id=o.workspace_id AND opportunity_id=o.id \
               AND candidate_set_id=o.work_item_id AND o.work_item_kind='scope_candidate_set' \
             ORDER BY created_at DESC,observation_id DESC LIMIT 1) obs ON true \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=ANY($3)",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(&ids)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let links_by_id = links
        .into_iter()
        .map(|row| (row.opportunity_id, row))
        .collect::<std::collections::HashMap<_, _>>();
    for opportunity in opportunities {
        if let Some(links) = links_by_id.get(&opportunity.id) {
            apply_audit_links(opportunity, links);
        }
    }
    Ok(())
}

async fn audit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope_id: Option<Uuid>,
    candidate_set_id: Option<Uuid>,
    query: &AdvisoryAuditQuery,
) -> Result<AdvisoryAuditPage> {
    query.validate()?;
    let config = config(tx, tenant, workspace).await?;
    let capability_filter = query.capability.map(AdvisoryCapability::as_str);
    let decision_filter = query.decision_point.map(AdvisoryDecisionPoint::as_str);
    let reason_filter = query.reason.map(AdvisoryReason::as_str);
    let state_filter = query.state.map(AdvisoryOpportunityState::as_str);
    let page_limit = i64::from(query.limit) + 1;
    let mut rows: Vec<OpportunityAuditRow> = sqlx::query_as(
        "SELECT o.id,o.workspace_id,o.scope_id,o.session_id,o.authorized_actor_id,o.work_item_kind,o.work_item_id,o.source_revision,o.run_id,o.phase,o.step,o.capability,o.decision_point,o.config_revision,o.session_preference,o.request_preference,o.policy_version,o.request_key,o.material_digest,o.deterministic_baseline_ref,o.eligible_material_ref,o.state,o.primary_reason,o.parent_opportunity_id,to_char(o.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at,to_char(o.updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS updated_at FROM advisory_opportunity o WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND ($3::uuid IS NULL OR o.scope_id=$3) AND ($10::uuid IS NULL OR (o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$10)) AND ($4::text IS NULL OR o.capability=$4) AND ($5::text IS NULL OR o.decision_point=$5) AND ($6::text IS NULL OR o.primary_reason=$6) AND ($7::text IS NULL OR o.state=$7) AND ($8::uuid IS NULL OR (o.created_at,o.id)<(SELECT c.created_at,c.id FROM advisory_opportunity c WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$8 AND ($3::uuid IS NULL OR c.scope_id=$3) AND ($10::uuid IS NULL OR (c.scope_id IS NULL AND c.work_item_kind='scope_candidate_set' AND c.work_item_id=$10)))) ORDER BY o.created_at DESC,o.id DESC LIMIT $9",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .bind(capability_filter)
    .bind(decision_filter)
    .bind(reason_filter)
    .bind(state_filter)
    .bind(query.after)
    .bind(page_limit)
    .bind(candidate_set_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let has_more = rows.len() > query.limit as usize;
    if has_more {
        rows.truncate(query.limit as usize);
    }
    let next_after = has_more.then(|| rows.last().expect("non-empty bounded page").id);
    let mut opportunities = rows
        .into_iter()
        .map(opportunity_audit_from_row)
        .collect::<Result<Vec<_>>>()?;
    audit_links(tx, tenant, workspace, &mut opportunities).await?;
    let opportunity_ids = opportunities.iter().map(|row| row.id).collect::<Vec<_>>();
    let dispatch_rows: Vec<DispatchAuditRow> = if opportunity_ids.is_empty() {
        Vec::new()
    } else {
        sqlx::query_as(
            "SELECT d.id,d.opportunity_id,d.predecessor_dispatch_id,d.attempt_number,d.provider,d.model,d.configuration_digest,d.material_digest,d.payload_digest,octet_length(d.request_payload)::bigint AS request_bytes,CASE WHEN d.response_payload IS NULL THEN NULL ELSE octet_length(d.response_payload)::bigint END AS response_bytes,d.input_tokens,d.output_tokens,d.latency_ms,d.state,d.send_certainty,d.outcome,d.retry_basis,d.raw_response_ref,to_char(d.authorized_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS authorized_at,CASE WHEN d.send_started_at IS NULL THEN NULL ELSE to_char(d.send_started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END AS send_started_at,CASE WHEN d.sealed_at IS NULL THEN NULL ELSE to_char(d.sealed_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END AS sealed_at FROM advisory_dispatch d WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=ANY($3) ORDER BY d.opportunity_id,d.attempt_number,d.id",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(&opportunity_ids)
        .fetch_all(&mut **tx)
        .await
        .map_err(storage_error)?
    };
    let dispatches = dispatch_rows
        .into_iter()
        .map(dispatch_audit_from_row)
        .collect::<Result<Vec<_>>>()?;
    let aggregate_row: AuditAggregateRow = sqlx::query_as(
        "WITH filtered AS (SELECT o.id,o.state FROM advisory_opportunity o WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND ($3::uuid IS NULL OR o.scope_id=$3) AND ($8::uuid IS NULL OR (o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$8)) AND ($4::text IS NULL OR o.capability=$4) AND ($5::text IS NULL OR o.decision_point=$5) AND ($6::text IS NULL OR o.primary_reason=$6) AND ($7::text IS NULL OR o.state=$7)), opportunity_counts AS (SELECT count(*) AS opportunities,count(*) FILTER (WHERE EXISTS(SELECT 1 FROM advisory_dispatch d WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=filtered.id)) AS opportunities_with_attempts,count(*) FILTER (WHERE state='no_call') AS no_call_opportunities FROM filtered), attempt_counts AS (SELECT count(d.id) AS authorized_attempts,count(d.id) FILTER (WHERE d.send_certainty='sent') AS confirmed_sent_attempts,count(d.id) FILTER (WHERE d.send_certainty='sent_unknown') AS send_unknown_attempts,count(d.id) FILTER (WHERE d.send_certainty='not_sent' AND d.state IN ('sealed','cancelled')) AS proven_unsent_attempts,COALESCE(sum(d.input_tokens) FILTER (WHERE d.input_tokens IS NOT NULL),0)::bigint AS known_input_tokens,COALESCE(sum(d.output_tokens) FILTER (WHERE d.output_tokens IS NOT NULL),0)::bigint AS known_output_tokens,count(d.id) FILTER (WHERE d.input_tokens IS NULL OR d.output_tokens IS NULL) AS attempts_with_unknown_token_usage FROM filtered LEFT JOIN advisory_dispatch d ON d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=filtered.id) SELECT o.opportunities,o.opportunities_with_attempts,o.no_call_opportunities,a.authorized_attempts,a.confirmed_sent_attempts,a.send_unknown_attempts,a.proven_unsent_attempts,a.known_input_tokens,a.known_output_tokens,a.attempts_with_unknown_token_usage FROM opportunity_counts o CROSS JOIN attempt_counts a",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .bind(capability_filter)
    .bind(decision_filter)
    .bind(reason_filter)
    .bind(state_filter)
    .bind(candidate_set_id)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    let reason_rows: Vec<ReasonCountRow> = sqlx::query_as(
        "SELECT o.primary_reason AS reason,count(*)::bigint AS count FROM advisory_opportunity o WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND ($3::uuid IS NULL OR o.scope_id=$3) AND ($8::uuid IS NULL OR (o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$8)) AND ($4::text IS NULL OR o.capability=$4) AND ($5::text IS NULL OR o.decision_point=$5) AND ($6::text IS NULL OR o.primary_reason=$6) AND ($7::text IS NULL OR o.state=$7) AND o.state='no_call' GROUP BY o.primary_reason ORDER BY o.primary_reason",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .bind(capability_filter)
    .bind(decision_filter)
    .bind(reason_filter)
    .bind(state_filter)
    .bind(candidate_set_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    Ok(AdvisoryAuditPage {
        config,
        opportunities,
        dispatches,
        aggregate: audit_aggregate_from_rows(aggregate_row, reason_rows)?,
        next_after,
    })
}

async fn opportunity_detail(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    scope_id: Option<Uuid>,
    candidate_set_id: Option<Uuid>,
    opportunity_id: Uuid,
) -> Result<AdvisoryOpportunityDetail> {
    let row: OpportunityAuditRow = sqlx::query_as(
        "SELECT o.id,o.workspace_id,o.scope_id,o.session_id,o.authorized_actor_id,o.work_item_kind,o.work_item_id,o.source_revision,o.run_id,o.phase,o.step,o.capability,o.decision_point,o.config_revision,o.session_preference,o.request_preference,o.policy_version,o.request_key,o.material_digest,o.deterministic_baseline_ref,o.eligible_material_ref,o.state,o.primary_reason,o.parent_opportunity_id,to_char(o.created_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS created_at,to_char(o.updated_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS updated_at FROM advisory_opportunity o WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND ($3::uuid IS NULL OR o.scope_id=$3) AND ($5::uuid IS NULL OR (o.scope_id IS NULL AND o.work_item_kind='scope_candidate_set' AND o.work_item_id=$5)) AND o.id=$4",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(scope_id)
    .bind(opportunity_id)
    .bind(candidate_set_id)
    .fetch_optional(&mut **tx)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    let dispatch_rows: Vec<DispatchAuditRow> = sqlx::query_as(
        "SELECT d.id,d.opportunity_id,d.predecessor_dispatch_id,d.attempt_number,d.provider,d.model,d.configuration_digest,d.material_digest,d.payload_digest,octet_length(d.request_payload)::bigint AS request_bytes,CASE WHEN d.response_payload IS NULL THEN NULL ELSE octet_length(d.response_payload)::bigint END AS response_bytes,d.input_tokens,d.output_tokens,d.latency_ms,d.state,d.send_certainty,d.outcome,d.retry_basis,d.raw_response_ref,to_char(d.authorized_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') AS authorized_at,CASE WHEN d.send_started_at IS NULL THEN NULL ELSE to_char(d.send_started_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END AS send_started_at,CASE WHEN d.sealed_at IS NULL THEN NULL ELSE to_char(d.sealed_at AT TIME ZONE 'UTC','YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') END AS sealed_at FROM advisory_dispatch d WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=$3 ORDER BY d.attempt_number,d.id",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(opportunity_id)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    let mut opportunities = vec![opportunity_audit_from_row(row)?];
    audit_links(tx, tenant, workspace, &mut opportunities).await?;
    Ok(AdvisoryOpportunityDetail {
        opportunity: opportunities.remove(0),
        dispatches: dispatch_rows
            .into_iter()
            .map(dispatch_audit_from_row)
            .collect::<Result<Vec<_>>>()?,
    })
}

#[async_trait]
impl AdvisoryStore for PgUnitOfWork {
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
        audit(self.transaction()?, tenant, workspace_id, scope_id, None, query).await
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

    async fn advisory_candidate_set_exists(&mut self, workspace_id: Uuid, candidate_set_id: Uuid) -> Result<bool> {
        let tenant = self.tenant_id()?;
        sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)")
            .bind(tenant).bind(workspace_id).bind(candidate_set_id)
            .fetch_one(&mut **self.transaction()?).await.map_err(storage_error)
    }

    async fn candidate_advisory_audit(&mut self, workspace_id: Uuid, candidate_set_id: Uuid, query: &AdvisoryAuditQuery) -> Result<AdvisoryAuditPage> {
        let tenant = self.tenant_id()?;
        audit(self.transaction()?, tenant, workspace_id, None, Some(candidate_set_id), query).await
    }

    async fn candidate_advisory_opportunity_detail(&mut self, workspace_id: Uuid, candidate_set_id: Uuid, opportunity_id: Uuid) -> Result<AdvisoryOpportunityDetail> {
        let tenant = self.tenant_id()?;
        opportunity_detail(self.transaction()?, tenant, workspace_id, None, Some(candidate_set_id), opportunity_id).await
    }
}
