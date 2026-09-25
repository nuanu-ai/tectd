use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_application::{
    AntiBloatAttemptState, AntiBloatNoCall, AntiBloatPreparedRequest, AntiBloatSendPermit,
    AntiBloatStore, Sha256ScopeDigest, StoredAntiBloatReview,
};
use tect_domain::{
    AntiBloatApplyReceipt, AntiBloatDisposition, AntiBloatInput, AntiBloatObligationLink,
    AntiBloatPreservation, CandidateDeltaBatch, Error, ResolvedCandidateDraft, Result,
    ScopeConstructorManifest, WorkspaceAdvisoryMode, check_anti_bloat_delta, review_anti_bloat,
    scope_candidate_material_digest,
};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct SelectedBindingRow {
    manifest_payload: serde_json::Value,
    draft_payload: Option<serde_json::Value>,
    obligation_links: serde_json::Value,
    mandatory_policy_obligation_ids: serde_json::Value,
    dependency_digest: String,
    source_digest: String,
    provenance: String,
    set_revision: i64,
    current_snapshot_id: Option<Uuid>,
    selected_draft_revision: i64,
    selected_material_digest: String,
    selected_alternative_id: String,
    selected_caller_link_id: Uuid,
    selected_caller_request_id: Uuid,
    caller_request_id: Uuid,
    receipt_revision: i64,
    disposition_alternative_id: Option<String>,
}

#[derive(sqlx::FromRow)]
struct CurrentSourceAuthority {
    current_snapshot_id: Option<Uuid>,
    input_cursor: i64,
    candidate_latest_input: i64,
    program_id: Uuid,
    program_revision: i64,
    program_current_latest: i64,
    program_latest_input: i64,
    planning_latest_input: i64,
    selected_sources_digest: String,
    method_revision: String,
    method_digest: String,
    registry_revision: String,
    registry_digest: String,
}

fn digest(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

fn state_name(state: &AntiBloatAttemptState) -> &'static str {
    match state {
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::Disabled) => "disabled",
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::Skipped) => "skipped",
        AntiBloatAttemptState::NoCall(AntiBloatNoCall::NoEligibleFindings) => "no_eligible",
        AntiBloatAttemptState::Prepared => "prepared",
        AntiBloatAttemptState::Sending => "sending",
        AntiBloatAttemptState::Ranked(_) => "ranked",
        AntiBloatAttemptState::SendUnknown => "send_unknown",
    }
}

fn parse_state(name: &str, ranked: Option<serde_json::Value>) -> Result<AntiBloatAttemptState> {
    Ok(match name {
        "disabled" => AntiBloatAttemptState::NoCall(AntiBloatNoCall::Disabled),
        "skipped" => AntiBloatAttemptState::NoCall(AntiBloatNoCall::Skipped),
        "no_eligible" => AntiBloatAttemptState::NoCall(AntiBloatNoCall::NoEligibleFindings),
        "prepared" => AntiBloatAttemptState::Prepared,
        "sending" => AntiBloatAttemptState::Sending,
        "send_unknown" => AntiBloatAttemptState::SendUnknown,
        "ranked" => AntiBloatAttemptState::Ranked(
            serde_json::from_value(ranked.ok_or(Error::InternalInvariant)?)
                .map_err(storage_error)?,
        ),
        _ => return Err(Error::InternalInvariant),
    })
}

#[async_trait]
impl AntiBloatStore for PgUnitOfWork {
    async fn advisory_mode(&mut self, workspace_id: Uuid) -> Result<WorkspaceAdvisoryMode> {
        let tenant = self.tenant_id()?;
        let mode: Option<String> = sqlx::query_scalar(
            "SELECT mode FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2",
        )
        .bind(tenant)
        .bind(workspace_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        match mode.as_deref() {
            None | Some("disabled") => Ok(WorkspaceAdvisoryMode::Disabled),
            Some("optional") => Ok(WorkspaceAdvisoryMode::Optional),
            _ => Err(Error::InternalInvariant),
        }
    }

    async fn authoritative_input(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        expected_revision: i64,
    ) -> Result<Option<AntiBloatInput>> {
        let tenant = self.tenant_id()?;
        let row: Option<SelectedBindingRow> = sqlx::query_as(
            "SELECT m.aggregate_payload AS manifest_payload,dr.payload AS draft_payload, \
                    b.obligation_links,b.mandatory_policy_obligation_ids, \
                    b.dependency_digest,b.source_digest,b.provenance,s.revision AS set_revision, \
                    s.current_snapshot_id,b.selected_draft_revision,b.selected_material_digest, \
                    b.selected_alternative_id,b.selected_caller_link_id,b.selected_caller_request_id, \
                    c.caller_request_id,r.result_revision AS receipt_revision, \
                    d.selected_alternative_id AS disposition_alternative_id \
             FROM scope_anti_bloat_bindings b \
             JOIN advisory_scope_manifest m ON (m.tenant_id,m.workspace_id,m.opportunity_id,m.candidate_set_id)= \
                 (b.tenant_id,b.workspace_id,b.opportunity_id,b.candidate_set_id) \
             JOIN scope_candidate_sets s ON (s.tenant_id,s.workspace_id,s.id)= \
                 (b.tenant_id,b.workspace_id,b.candidate_set_id) \
             JOIN scope_candidate_drafts dr ON (dr.tenant_id,dr.workspace_id,dr.candidate_set_id,dr.set_revision)= \
                 (b.tenant_id,b.workspace_id,b.candidate_set_id,b.selected_draft_revision) \
             JOIN advisory_scope_caller_link c ON (c.tenant_id,c.workspace_id,c.link_id)= \
                 (b.tenant_id,b.workspace_id,b.selected_caller_link_id) \
             JOIN advisory_scope_disposition d ON (d.tenant_id,d.workspace_id,d.disposition_id)= \
                 (c.tenant_id,c.workspace_id,c.disposition_id) \
             JOIN scope_candidate_receipts r ON (r.tenant_id,r.workspace_id,r.candidate_set_id,r.operation,r.request_id)= \
                 (c.tenant_id,c.workspace_id,c.candidate_set_id,'save_draft',c.caller_request_id) \
             WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.candidate_set_id=$3 \
               AND b.candidate_set_revision=$4 AND b.selected_draft_revision IS NOT NULL \
               AND c.caller_operation='save_draft'",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(expected_revision)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        if row.set_revision != expected_revision
            || row.selected_draft_revision != expected_revision
            || row.caller_request_id != row.selected_caller_request_id
            || row.receipt_revision != expected_revision
            || row.disposition_alternative_id.as_deref()
                != Some(row.selected_alternative_id.as_str())
        {
            return Ok(None);
        }
        let manifest: ScopeConstructorManifest =
            serde_json::from_value(row.manifest_payload).map_err(storage_error)?;
        let selected_id = tect_domain::ScopeAlternativeId(row.selected_alternative_id);
        let selected = manifest
            .eligible(&selected_id)
            .ok_or(Error::InputConflict)?;
        let saved: ResolvedCandidateDraft =
            serde_json::from_value(row.draft_payload.ok_or(Error::InputConflict)?)
                .map_err(storage_error)?;
        if manifest.source.candidate_set_id != candidate_set_id
            || manifest.source.candidate_set_revision >= expected_revision
            || manifest.source.snapshot_id != row.current_snapshot_id.ok_or(Error::InputConflict)?
            || manifest.source.digest != row.source_digest
            || saved != selected.material
            || selected.material_digest != row.selected_material_digest
        {
            return Err(Error::InputConflict);
        }
        let (expected_links, expected_dependency, base_provenance) =
            crate::scope_advisory::authored_graph_binding_for(
                &manifest,
                &selected_id,
                expected_revision,
            )?;
        let expected_provenance = format!(
            "{base_provenance}:selected={}:caller={}:receipt={}",
            selected_id.0, row.selected_caller_link_id, row.selected_caller_request_id
        );
        if row.obligation_links != serde_json::to_value(expected_links).map_err(storage_error)?
            || row.dependency_digest != expected_dependency
            || row.provenance != expected_provenance
            || row.mandatory_policy_obligation_ids != serde_json::json!([])
        {
            return Err(Error::InputConflict);
        }
        let input = AntiBloatInput {
            selected_id,
            selected_revision: expected_revision,
            manifest,
            graph_provenance: row.provenance,
            dependency_digest: row.dependency_digest,
            obligation_links: serde_json::from_value::<Vec<AntiBloatObligationLink>>(
                row.obligation_links,
            )
            .map_err(storage_error)?,
            mandatory_policy_obligation_ids: serde_json::from_value(
                row.mandatory_policy_obligation_ids,
            )
            .map_err(storage_error)?,
        };
        review_anti_bloat(&Sha256ScopeDigest, &input)?;
        Ok(Some(input))
    }

    async fn save_review(
        &mut self,
        record: StoredAntiBloatReview,
    ) -> Result<StoredAntiBloatReview> {
        if !self.is_read_write() || self.principal_id()? != record.actor_id {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        let eligible = record
            .review
            .findings
            .iter()
            .filter(|finding| finding.rankable)
            .map(|finding| finding.id.clone())
            .collect::<Vec<_>>();
        let current = self
            .authoritative_input(
                record.workspace_id,
                record.review.candidate_set_id,
                record.review.plan_revision,
            )
            .await?;
        if current.as_ref() != Some(&record.input)
            || review_anti_bloat(&Sha256ScopeDigest, &record.input)? != record.review
        {
            return Err(Error::InputConflict);
        }
        sqlx::query(
            "INSERT INTO scope_anti_bloat_reviews \
             (tenant_id,workspace_id,review_id,candidate_set_id,candidate_set_revision,actor_id, \
              input_payload,review_payload,state,eligible_ids) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)",
        )
        .bind(tenant)
        .bind(record.workspace_id)
        .bind(record.review_id)
        .bind(record.review.candidate_set_id)
        .bind(record.review.plan_revision)
        .bind(record.actor_id)
        .bind(serde_json::to_value(&record.input).map_err(storage_error)?)
        .bind(serde_json::to_value(&record.review).map_err(storage_error)?)
        .bind(state_name(&record.state))
        .bind(serde_json::to_value(eligible).map_err(storage_error)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(record)
    }

    async fn review(&mut self, review_id: Uuid) -> Result<Option<StoredAntiBloatReview>> {
        let tenant = self.tenant_id()?;
        type Row = (
            Uuid,
            Uuid,
            serde_json::Value,
            serde_json::Value,
            String,
            Option<serde_json::Value>,
        );
        let row: Option<Row> = sqlx::query_as(
            "SELECT workspace_id,actor_id,input_payload,review_payload,state,ranked_ids \
             FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND review_id=$2",
        )
        .bind(tenant)
        .bind(review_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(|(workspace_id, actor_id, input, review, state, ranked)| {
            if self.principal_id()? != actor_id {
                return Err(Error::Forbidden);
            }
            Ok(StoredAntiBloatReview {
                review_id,
                workspace_id,
                actor_id,
                input: serde_json::from_value(input).map_err(storage_error)?,
                review: serde_json::from_value(review).map_err(storage_error)?,
                state: parse_state(&state, ranked)?,
            })
        })
        .transpose()
    }

    async fn begin_send(
        &mut self,
        saved: &StoredAntiBloatReview,
        prepared: &AntiBloatPreparedRequest,
    ) -> Result<Option<AntiBloatSendPermit>> {
        if !self.is_read_write() || self.principal_id()? != saved.actor_id {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        let locked_revision: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 \
             AND workspace_id=$2 AND id=$3 FOR SHARE",
        )
        .bind(tenant)
        .bind(saved.workspace_id)
        .bind(saved.review.candidate_set_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if locked_revision != Some(saved.review.plan_revision) {
            return Err(Error::InputConflict);
        }
        if self
            .authoritative_input(
                saved.workspace_id,
                saved.review.candidate_set_id,
                saved.review.plan_revision,
            )
            .await?
            .as_ref()
            != Some(&saved.input)
        {
            return Err(Error::InputConflict);
        }
        let eligible = saved
            .review
            .findings
            .iter()
            .filter(|item| item.rankable)
            .map(|item| item.id.clone())
            .collect::<Vec<_>>();
        if eligible.is_empty() || saved.state != AntiBloatAttemptState::Prepared {
            return Ok(None);
        }
        if digest(&prepared.bytes) != prepared.sha256 {
            return Err(Error::InputConflict);
        }
        let expected = serde_json::to_vec(&serde_json::json!({
            "review": &saved.review, "eligible_ids": &eligible
        }))
        .map_err(storage_error)?;
        if prepared.bytes != expected {
            return Err(Error::InputConflict);
        }
        let result = sqlx::query(
            "UPDATE scope_anti_bloat_reviews SET state='sending',request_bytes=$5, \
             request_sha256=$6,send_started_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3 AND actor_id=$4 \
               AND state='prepared' AND input_payload=$7 AND review_payload=$8",
        )
        .bind(tenant)
        .bind(saved.workspace_id)
        .bind(saved.review_id)
        .bind(saved.actor_id)
        .bind(&prepared.bytes)
        .bind(&prepared.sha256)
        .bind(serde_json::to_value(&saved.input).map_err(storage_error)?)
        .bind(serde_json::to_value(&saved.review).map_err(storage_error)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok((result.rows_affected() == 1).then(|| AntiBloatSendPermit {
            review_id: saved.review_id,
            request: prepared.clone(),
        }))
    }

    async fn mark_send_unknown(&mut self, review_id: Uuid) -> Result<()> {
        let tenant = self.tenant_id()?;
        let actor = self.principal_id()?;
        sqlx::query(
            "UPDATE scope_anti_bloat_reviews SET state='send_unknown' \
                     WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 AND state='sending'",
        )
        .bind(tenant)
        .bind(review_id)
        .bind(actor)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(())
    }

    async fn seal_response(
        &mut self,
        permit: &AntiBloatSendPermit,
        raw_response: &[u8],
        response_sha256: &str,
    ) -> Result<()> {
        if !self.is_read_write() || digest(raw_response) != response_sha256 {
            return Err(Error::InputConflict);
        }
        let result = sqlx::query(
            "UPDATE scope_anti_bloat_reviews SET raw_response=$5,response_sha256=$6, \
             response_sealed_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 AND state='sending' \
               AND request_bytes=$4 AND request_sha256=$7 AND raw_response IS NULL",
        )
        .bind(self.tenant_id()?)
        .bind(permit.review_id)
        .bind(self.principal_id()?)
        .bind(&permit.request.bytes)
        .bind(raw_response)
        .bind(response_sha256)
        .bind(&permit.request.sha256)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    async fn seal_ranked(&mut self, review_id: Uuid, ranked_ids: &[String]) -> Result<()> {
        let tenant = self.tenant_id()?;
        let actor = self.principal_id()?;
        let value = serde_json::to_value(ranked_ids).map_err(storage_error)?;
        let result = sqlx::query(
            "UPDATE scope_anti_bloat_reviews SET state='ranked',ranked_ids=$4, \
             sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND review_id=$2 \
             AND actor_id=$3 AND state='sending' AND raw_response IS NOT NULL \
             AND eligible_ids @> $4::jsonb AND $4::jsonb @> eligible_ids",
        )
        .bind(tenant)
        .bind(review_id)
        .bind(actor)
        .bind(value)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    async fn apply_preserved_delta(
        &mut self,
        review_id: Uuid,
        input: &AntiBloatInput,
        finding_id: &str,
        disposition: AntiBloatDisposition,
        preservation: &AntiBloatPreservation,
        delta: &CandidateDeltaBatch,
        after: &ResolvedCandidateDraft,
    ) -> Result<AntiBloatApplyReceipt> {
        if !self.is_read_write() || disposition != AntiBloatDisposition::Narrow {
            return Err(Error::Forbidden);
        }
        let saved = self.review(review_id).await?.ok_or(Error::NotFound)?;
        if &saved.input != input || saved.review.candidate_set_id != delta.candidate_set_id {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        let workspace = saved.workspace_id;
        // Serialize both new writes and replay with native candidate writers.
        let revision: Option<i64> = sqlx::query_scalar(
            "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 \
             AND id=$3 FOR UPDATE",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(delta.candidate_set_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        type Existing = (
            String,
            serde_json::Value,
            serde_json::Value,
            serde_json::Value,
            String,
            String,
            Option<String>,
            Option<Uuid>,
            Option<i64>,
            Option<i64>,
            Option<String>,
            Option<String>,
            serde_json::Value,
        );
        let existing: Option<Existing> = sqlx::query_as(
            "SELECT finding_id,preservation_payload,delta_payload,after_payload, \
             after_material_digest,caller_idempotency_key,caller_operation,caller_request_id, \
             from_revision,to_revision,source_digest,before_material_digest,caller_receipt \
             FROM scope_anti_bloat_caller_links \
             WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(review_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let preservation_json = serde_json::to_value(preservation).map_err(storage_error)?;
        let delta_json = serde_json::to_value(delta).map_err(storage_error)?;
        let after_json = serde_json::to_value(after).map_err(storage_error)?;
        if let Some((
            stored_finding,
            stored_preservation,
            stored_delta,
            stored_after,
            stored_after_digest,
            stored_key,
            stored_operation,
            stored_request_id,
            stored_from_revision,
            stored_to_revision,
            stored_source_digest,
            stored_before_digest,
            stored_receipt,
        )) = existing
        {
            if stored_finding != finding_id
                || stored_preservation != preservation_json
                || stored_delta != delta_json
                || stored_after != after_json
                || stored_after_digest != preservation.after_material_digest
                || stored_key != delta.idempotency_key
                || stored_operation.as_deref() != Some("anti_bloat_narrow")
                || stored_from_revision != Some(delta.expected_revision)
                || stored_source_digest.as_deref() != Some(preservation.source_digest.as_str())
                || stored_before_digest.as_deref()
                    != Some(preservation.before_material_digest.as_str())
            {
                return Err(Error::InputConflict);
            }
            let receipt: AntiBloatApplyReceipt =
                serde_json::from_value(stored_receipt.clone()).map_err(storage_error)?;
            if receipt.review_id != review_id
                || receipt.candidate_set_id != delta.candidate_set_id
                || receipt.idempotency_key != delta.idempotency_key
                || Some(receipt.caller_request_id) != stored_request_id
                || Some(receipt.from_revision) != stored_from_revision
                || Some(receipt.to_revision) != stored_to_revision
                || receipt.source_digest != preservation.source_digest
                || receipt.before_material_digest != preservation.before_material_digest
                || receipt.after_material_digest != preservation.after_material_digest
            {
                return Err(Error::InputConflict);
            }
            let native: Option<(serde_json::Value, serde_json::Value)> = sqlx::query_as(
                "SELECT r.result_payload,d.payload FROM scope_candidate_receipts r \
                 JOIN scope_candidate_drafts d ON (d.tenant_id,d.workspace_id,d.candidate_set_id,d.set_revision)= \
                     (r.tenant_id,r.workspace_id,r.candidate_set_id,r.result_revision) \
                 WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.candidate_set_id=$3 \
                   AND r.operation='anti_bloat_narrow' AND r.request_id=$4 AND r.result_revision=$5",
            )
            .bind(tenant)
            .bind(workspace)
            .bind(delta.candidate_set_id)
            .bind(receipt.caller_request_id)
            .bind(receipt.to_revision)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if native != Some((stored_receipt, after_json)) {
                return Err(Error::InputConflict);
            }
            return Ok(receipt);
        }
        if revision != Some(delta.expected_revision)
            || self
                .authoritative_input(workspace, delta.candidate_set_id, delta.expected_revision)
                .await?
                .as_ref()
                != Some(input)
            || review_anti_bloat(&Sha256ScopeDigest, input)? != saved.review
        {
            return Err(Error::InputConflict);
        }
        let checked = check_anti_bloat_delta(
            &Sha256ScopeDigest,
            input,
            &saved.review,
            finding_id,
            disposition,
            delta,
            after,
        )
        .map_err(|_| Error::InputConflict)?;
        if &checked != preservation {
            return Err(Error::InputConflict);
        }
        let source = &input.manifest.source;
        let authority: Option<CurrentSourceAuthority> = sqlx::query_as(
            "SELECT c.current_snapshot_id,c.input_cursor,c.latest_input AS candidate_latest_input, \
                    c.program_id,p.revision AS program_revision,p.latest_input AS program_current_latest, \
                    s.program_latest_input,s.planning_latest_input,s.selected_sources_digest, \
                    s.method_revision,s.method_digest,s.registry_revision,s.registry_digest \
             FROM scope_candidate_sets c JOIN programs p \
               ON (p.tenant_id,p.workspace_id,p.id)=(c.tenant_id,c.workspace_id,c.program_id) \
             JOIN scope_candidate_snapshots s \
               ON (s.tenant_id,s.workspace_id,s.candidate_set_id,s.id)= \
                  (c.tenant_id,c.workspace_id,c.id,c.current_snapshot_id) \
             WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.id=$3 FOR UPDATE OF p",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(delta.candidate_set_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(authority) = authority else {
            return Err(Error::InputConflict);
        };
        if authority.current_snapshot_id != Some(source.snapshot_id)
            || authority.input_cursor != source.input_cursor
            || authority.candidate_latest_input != source.planning_latest_input
            || authority.program_id != source.program_id
            || authority.program_revision != source.program_revision
            || authority.program_current_latest != source.program_latest_input
            || authority.program_latest_input != source.program_latest_input
            || authority.planning_latest_input != source.planning_latest_input
            || authority.selected_sources_digest != source.selected_sources_digest
            || authority.method_revision != source.method_revision
            || authority.method_digest != source.method_digest
            || authority.registry_revision != source.registry_revision
            || authority.registry_digest != source.registry_digest
        {
            return Err(Error::StaleRevision);
        }
        crate::scope_advisory::require_persisted_fragments(
            self.transaction()?,
            tenant,
            workspace,
            source,
            &input.manifest.obligations,
        )
        .await?;
        let before = &input
            .manifest
            .eligible(&input.selected_id)
            .ok_or(Error::InputConflict)?
            .material;
        if scope_candidate_material_digest(&Sha256ScopeDigest, before)?
            != preservation.before_material_digest
            || scope_candidate_material_digest(&Sha256ScopeDigest, after)?
                != preservation.after_material_digest
        {
            return Err(Error::InputConflict);
        }
        let receipt = AntiBloatApplyReceipt {
            review_id,
            candidate_set_id: delta.candidate_set_id,
            idempotency_key: delta.idempotency_key.clone(),
            caller_request_id: Uuid::new_v4(),
            from_revision: delta.expected_revision,
            to_revision: delta
                .expected_revision
                .checked_add(1)
                .ok_or(Error::StorageUnavailable)?,
            source_digest: preservation.source_digest.clone(),
            before_material_digest: preservation.before_material_digest.clone(),
            after_material_digest: preservation.after_material_digest.clone(),
        };
        let request_payload = serde_json::json!({
            "review_id": review_id,
            "finding_id": finding_id,
            "disposition": disposition,
            "preservation": preservation,
            "delta": delta,
            "after": after,
        });
        crate::scope_candidates::save_preserved_anti_bloat_draft(
            self.transaction()?,
            tenant,
            workspace,
            &receipt,
            source.snapshot_id,
            source.input_cursor,
            before,
            after,
            request_payload,
        )
        .await?;
        sqlx::query(
            "INSERT INTO scope_anti_bloat_caller_links \
             (tenant_id,workspace_id,review_id,candidate_set_id,finding_id,disposition, \
              preservation_payload,delta_payload,after_payload,after_material_digest, \
              caller_idempotency_key,caller_receipt,caller_operation,caller_request_id, \
              from_revision,to_revision,source_digest,before_material_digest) \
             VALUES ($1,$2,$3,$4,$5,'narrow',$6,$7,$8,$9,$10,$11,'anti_bloat_narrow',$12,$13,$14,$15,$16)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(review_id)
        .bind(delta.candidate_set_id)
        .bind(finding_id)
        .bind(preservation_json)
        .bind(delta_json)
        .bind(after_json)
        .bind(&preservation.after_material_digest)
        .bind(&delta.idempotency_key)
        .bind(serde_json::to_value(&receipt).map_err(storage_error)?)
        .bind(receipt.caller_request_id)
        .bind(receipt.from_revision)
        .bind(receipt.to_revision)
        .bind(&receipt.source_digest)
        .bind(&receipt.before_material_digest)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(receipt)
    }
}
