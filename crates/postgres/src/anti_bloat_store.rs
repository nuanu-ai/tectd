use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_application::{
    AntiBloatAttemptState, AntiBloatNoCall, AntiBloatStore, Sha256ScopeDigest,
    StoredAntiBloatReview,
};
use tect_domain::{
    AntiBloatDisposition, AntiBloatInput, AntiBloatObligationLink, AntiBloatPreservation,
    CandidateDeltaBatch, CandidateDeltaReceipt, Error, Result, ScopeConstructorManifest,
    WorkspaceAdvisoryMode, review_anti_bloat,
};
use uuid::Uuid;

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
        type Bound = (
            serde_json::Value,
            serde_json::Value,
            serde_json::Value,
            String,
            String,
            i64,
            Uuid,
        );
        let row: Option<Bound> = sqlx::query_as(
            "SELECT m.aggregate_payload,b.obligation_links,b.mandatory_policy_obligation_ids, \
                    b.dependency_digest,b.source_digest,s.revision,s.current_snapshot_id \
             FROM scope_anti_bloat_bindings b \
             JOIN advisory_scope_manifest m ON (m.tenant_id,m.workspace_id,m.opportunity_id,m.candidate_set_id)= \
                 (b.tenant_id,b.workspace_id,b.opportunity_id,b.candidate_set_id) \
             JOIN scope_candidate_sets s ON (s.tenant_id,s.workspace_id,s.id)= \
                 (b.tenant_id,b.workspace_id,b.candidate_set_id) \
             WHERE b.tenant_id=$1 AND b.workspace_id=$2 AND b.candidate_set_id=$3 \
               AND b.candidate_set_revision=$4",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(expected_revision)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some((payload, links, policy, dependency_digest, source_digest, revision, snapshot)) =
            row
        else {
            return Ok(None);
        };
        if revision != expected_revision {
            return Ok(None);
        }
        let manifest: ScopeConstructorManifest =
            serde_json::from_value(payload).map_err(storage_error)?;
        if manifest.source.candidate_set_id != candidate_set_id
            || manifest.source.candidate_set_revision != revision
            || manifest.source.snapshot_id != snapshot
            || manifest.source.digest != source_digest
        {
            return Err(Error::InputConflict);
        }
        let input = AntiBloatInput {
            selected_id: manifest.baseline_id.clone(),
            manifest,
            dependency_digest,
            obligation_links: serde_json::from_value::<Vec<AntiBloatObligationLink>>(links)
                .map_err(storage_error)?,
            mandatory_policy_obligation_ids: serde_json::from_value(policy)
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

    async fn begin_send(&mut self, saved: &StoredAntiBloatReview) -> Result<bool> {
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
            return Ok(false);
        }
        let bytes = serde_json::to_vec(&serde_json::json!({
            "review": &saved.review, "eligible_ids": &eligible
        }))
        .map_err(storage_error)?;
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
        .bind(&bytes)
        .bind(digest(&bytes))
        .bind(serde_json::to_value(&saved.input).map_err(storage_error)?)
        .bind(serde_json::to_value(&saved.review).map_err(storage_error)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(result.rows_affected() == 1)
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

    async fn seal_ranked(&mut self, review_id: Uuid, ranked_ids: &[String]) -> Result<()> {
        let tenant = self.tenant_id()?;
        let actor = self.principal_id()?;
        let value = serde_json::to_value(ranked_ids).map_err(storage_error)?;
        let result = sqlx::query("UPDATE scope_anti_bloat_reviews SET state='ranked',ranked_ids=$4, \
             sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND review_id=$2 \
             AND actor_id=$3 AND state='sending' AND eligible_ids @> $4::jsonb AND $4::jsonb @> eligible_ids")
            .bind(tenant).bind(review_id).bind(actor).bind(value)
            .execute(&mut **self.transaction()?).await.map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(Error::InputConflict);
        }
        Ok(())
    }

    async fn apply_preserved_delta(
        &mut self,
        _review_id: Uuid,
        _input: &AntiBloatInput,
        _finding_id: &str,
        _disposition: AntiBloatDisposition,
        _preservation: &AntiBloatPreservation,
        _delta: &CandidateDeltaBatch,
    ) -> Result<CandidateDeltaReceipt> {
        // The current application port does not pass the caller's full after
        // graph into this transaction. Refuse until whole-plan recheck and the
        // candidate-delta CAS can be bound in one transaction.
        Err(Error::Forbidden)
    }
}
