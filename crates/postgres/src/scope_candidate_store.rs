use crate::{scope_candidates, store::PgUnitOfWork};
use async_trait::async_trait;
use tect_application::{CandidateDeltaStore, ScopeCandidateStore};
use tect_domain::{
    BeginCandidateSet, BeginCandidateSetOutcome, CandidateHistoryEntry, CandidateInputSummary,
    CandidateReceiptRequest, CandidateSetSummary, CandidateSnapshotMaterial, CandidateTextFragment,
    RecordCandidateInput, RefreshCandidateSet, Result, ReviewCandidateSet, SaveCandidateDraft,
    StoredCandidateContext, StoredHistoricalCandidateDraft,
};
use uuid::Uuid;

#[async_trait]
impl ScopeCandidateStore for PgUnitOfWork {
    async fn candidate_begin_replay(
        &mut self,
        workspace_id: Uuid,
        request: &BeginCandidateSet,
    ) -> Result<Option<BeginCandidateSetOutcome>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::begin_replay(self.transaction()?, tenant_id, workspace_id, request).await
    }

    async fn candidate_receipt(
        &mut self,
        workspace_id: Uuid,
        request: &CandidateReceiptRequest,
    ) -> Result<Option<StoredCandidateContext>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::replay(self.transaction()?, tenant_id, workspace_id, request).await
    }
    async fn ensure_candidate_set(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &BeginCandidateSet,
        input_bytes: i64,
        material: &CandidateSnapshotMaterial,
    ) -> Result<BeginCandidateSetOutcome> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::ensure(
            self.transaction()?,
            tenant_id,
            workspace_id,
            session_id,
            request,
            input_bytes,
            material,
        )
        .await
    }

    async fn candidate_context(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
    ) -> Result<Option<StoredCandidateContext>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::load(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
        )
        .await
    }

    async fn candidate_history(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CandidateHistoryEntry>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::history(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            after,
            limit,
        )
        .await
    }

    async fn historical_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        draft_revision: i64,
    ) -> Result<Option<StoredHistoricalCandidateDraft>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::historical(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            draft_revision,
        )
        .await
    }

    async fn candidate_heads(
        &mut self,
        workspace_id: Uuid,
        limit: u32,
    ) -> Result<Vec<CandidateSetSummary>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::heads(self.transaction()?, tenant_id, workspace_id, limit).await
    }

    async fn candidate_inputs(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        after: i64,
        limit: u32,
    ) -> Result<Vec<CandidateInputSummary>> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::input_summaries(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            after,
            limit,
        )
        .await
    }

    async fn candidate_fragment(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        snapshot_id: Option<Uuid>,
        source_ref_id: Uuid,
        cursor: usize,
        max_bytes: usize,
    ) -> Result<CandidateTextFragment> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::fragment(
            self.transaction()?,
            tenant_id,
            workspace_id,
            candidate_set_id,
            snapshot_id,
            source_ref_id,
            cursor,
            max_bytes,
        )
        .await
    }

    async fn save_candidate_draft(
        &mut self,
        workspace_id: Uuid,
        request: &SaveCandidateDraft,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::save_draft(self.transaction()?, tenant_id, workspace_id, request).await
    }

    async fn save_candidate_review(
        &mut self,
        workspace_id: Uuid,
        request: &ReviewCandidateSet,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::save_review(self.transaction()?, tenant_id, workspace_id, request).await
    }

    async fn record_candidate_input(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &RecordCandidateInput,
        input_bytes: i64,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::record_input(
            self.transaction()?,
            tenant_id,
            workspace_id,
            session_id,
            request,
            input_bytes,
        )
        .await
    }

    async fn refresh_candidate_set(
        &mut self,
        workspace_id: Uuid,
        request: &RefreshCandidateSet,
        material: &CandidateSnapshotMaterial,
    ) -> Result<StoredCandidateContext> {
        let tenant_id = self.tenant_id()?;
        scope_candidates::refresh(
            self.transaction()?,
            tenant_id,
            workspace_id,
            request,
            material,
        )
        .await
    }
}

#[async_trait]
impl CandidateDeltaStore for PgUnitOfWork {
    async fn apply_candidate_delta(
        &mut self,
        workspace_id: Uuid,
        request: &tect_domain::CandidateDeltaBatch,
    ) -> Result<tect_domain::CandidateDeltaReceipt> {
        request.validate()?;
        let tenant_id = self.tenant_id()?;
        let tx = self.transaction()?;
        let payload = serde_json::to_value(request).map_err(crate::storage_error)?;
        if let Some((from_revision, to_revision, stale_json, stored_payload)) =
            sqlx::query_as::<_, (i64, i64, serde_json::Value, serde_json::Value)>(
                "SELECT from_revision,to_revision,stale_reasons,request_payload \
                 FROM scope_candidate_delta_receipts WHERE tenant_id=$1 AND workspace_id=$2 \
                 AND candidate_set_id=$3 AND idempotency_key=$4",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.candidate_set_id)
            .bind(&request.idempotency_key)
            .fetch_optional(&mut **tx)
            .await
            .map_err(crate::storage_error)?
        {
            if stored_payload != payload {
                return Err(tect_domain::Error::InputConflict);
            }
            return Ok(tect_domain::CandidateDeltaReceipt {
                candidate_set_id: request.candidate_set_id,
                idempotency_key: request.idempotency_key.clone(),
                from_revision,
                to_revision,
                stale_reasons: serde_json::from_value(stale_json).map_err(crate::storage_error)?,
            });
        }
        let current: i64 = sqlx::query_scalar(
            "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE"
        ).bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id)
        .fetch_optional(&mut **tx).await.map_err(crate::storage_error)?
        .ok_or(tect_domain::Error::NotFound)?;
        if current != request.expected_revision {
            return Err(tect_domain::Error::StaleRevision);
        }
        let next = current
            .checked_add(1)
            .ok_or(tect_domain::Error::StorageUnavailable)?;
        let stale = tect_domain::candidate_delta_stale_reasons(&request.operations);
        let stale_json = serde_json::to_value(&stale).map_err(crate::storage_error)?;
        sqlx::query("UPDATE scope_candidate_sets SET revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(next)
            .execute(&mut **tx).await.map_err(crate::storage_error)?;
        sqlx::query("INSERT INTO scope_candidate_delta_receipts (tenant_id,workspace_id,candidate_set_id,idempotency_key,request_payload,expected_revision,from_revision,to_revision,stale_reasons) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9)")
            .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(&request.idempotency_key).bind(&payload).bind(request.expected_revision).bind(current).bind(next).bind(&stale_json)
            .execute(&mut **tx).await.map_err(crate::storage_error)?;
        for (index, operation) in request.operations.iter().enumerate() {
            sqlx::query("INSERT INTO scope_candidate_delta_operations (tenant_id,workspace_id,candidate_set_id,idempotency_key,operation_index,operation) VALUES ($1,$2,$3,$4,$5,$6)")
                .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(&request.idempotency_key).bind(index as i32)
                .bind(serde_json::to_value(operation).map_err(crate::storage_error)?)
                .execute(&mut **tx).await.map_err(crate::storage_error)?;
            use tect_domain::CandidateDeltaOperation as Op;
            match operation {
                Op::GoalAdd { goal_id, value } => {
                    ensure_source_ref(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        value.source_ref_id,
                    )
                    .await?;
                    let payload = serde_json::to_value(value).map_err(crate::storage_error)?;
                    sqlx::query("INSERT INTO scope_candidate_delta_goals (tenant_id,workspace_id,candidate_set_id,goal_id,payload,text,finite,source_ref_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(goal_id)
                        .bind(payload).bind(&value.text).bind(value.finite).bind(value.source_ref_id)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                }
                Op::GoalResolve {
                    goal_id,
                    expected_revision,
                } => {
                    let result = sqlx::query("UPDATE scope_candidate_delta_goals SET revision=revision+1,resolved=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND goal_id=$4 AND revision=$5 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(goal_id).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
                Op::CandidateAdd {
                    candidate_id,
                    value,
                } => {
                    let payload = serde_json::to_value(value).map_err(crate::storage_error)?;
                    sqlx::query("INSERT INTO scope_candidate_delta_candidates (tenant_id,workspace_id,candidate_set_id,candidate_id,payload,title,outcome) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id)
                        .bind(payload).bind(&value.title).bind(&value.outcome)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                }
                Op::CandidateUpdate {
                    candidate_id,
                    expected_revision,
                    value,
                } => {
                    let payload = serde_json::to_value(value).map_err(crate::storage_error)?;
                    let result = sqlx::query("UPDATE scope_candidate_delta_candidates SET revision=revision+1,payload=$5,title=$6,outcome=$7 WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND revision=$8 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id)
                        .bind(payload).bind(&value.title).bind(&value.outcome).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
                Op::CandidateRemove {
                    candidate_id,
                    expected_revision,
                } => {
                    let referenced: bool = sqlx::query_scalar(
                        "SELECT EXISTS(SELECT 1 FROM scope_candidate_delta_coverage WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4
                         UNION ALL SELECT 1 FROM scope_candidate_delta_evidence WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND NOT deleted
                         UNION ALL SELECT 1 FROM scope_candidate_delta_supersessions WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND (candidate_id=$4 OR replacement_candidate_id=$4))")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id)
                        .fetch_one(&mut **tx).await.map_err(crate::storage_error)?;
                    if referenced {
                        return Err(tect_domain::Error::InvalidArguments);
                    }
                    let result = sqlx::query("UPDATE scope_candidate_delta_candidates SET revision=revision+1,deleted=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND revision=$5 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
                Op::CandidateSupersede {
                    candidate_id,
                    replacement_candidate_id,
                    expected_revision,
                } => {
                    let cycle: bool = sqlx::query_scalar(
                        "WITH RECURSIVE successors(id) AS (
                           SELECT replacement_candidate_id FROM scope_candidate_delta_supersessions
                           WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4
                           UNION SELECT s.replacement_candidate_id FROM scope_candidate_delta_supersessions s
                           JOIN successors p ON s.candidate_id=p.id
                           WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.candidate_set_id=$3)
                         SELECT EXISTS(SELECT 1 FROM successors WHERE id=$5)")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id)
                        .bind(replacement_candidate_id).bind(candidate_id)
                        .fetch_one(&mut **tx).await.map_err(crate::storage_error)?;
                    if cycle {
                        return Err(tect_domain::Error::InvalidArguments);
                    }
                    ensure_live_candidate(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        *candidate_id,
                    )
                    .await?;
                    ensure_live_candidate(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        *replacement_candidate_id,
                    )
                    .await?;
                    let result = sqlx::query("UPDATE scope_candidate_delta_candidates SET revision=revision+1,deleted=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND revision=$5 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                    sqlx::query("INSERT INTO scope_candidate_delta_supersessions (tenant_id,workspace_id,candidate_set_id,candidate_id,replacement_candidate_id) VALUES ($1,$2,$3,$4,$5)")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id).bind(replacement_candidate_id)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                }
                Op::CoverageLink {
                    candidate_id,
                    goal_id,
                } => {
                    ensure_live_candidate(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        *candidate_id,
                    )
                    .await?;
                    ensure_live_goal(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        *goal_id,
                    )
                    .await?;
                    sqlx::query("INSERT INTO scope_candidate_delta_coverage (tenant_id,workspace_id,candidate_set_id,candidate_id,goal_id) VALUES ($1,$2,$3,$4,$5) ON CONFLICT DO NOTHING")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id).bind(goal_id)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                }
                Op::CoverageUnlink {
                    candidate_id,
                    goal_id,
                } => {
                    let result = sqlx::query("DELETE FROM scope_candidate_delta_coverage WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND goal_id=$5")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(candidate_id).bind(goal_id)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::NotFound);
                    }
                }
                Op::EvidenceAdd {
                    evidence_id,
                    target_kind,
                    target_id,
                    value,
                } => {
                    ensure_source_ref(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        value.source_ref_id,
                    )
                    .await?;
                    let (candidate_id, goal_id) = match target_kind {
                        tect_domain::CandidateDeltaTargetKind::Candidate => {
                            ensure_live_candidate(
                                &mut **tx,
                                tenant_id,
                                workspace_id,
                                request.candidate_set_id,
                                *target_id,
                            )
                            .await?;
                            (Some(*target_id), None)
                        }
                        tect_domain::CandidateDeltaTargetKind::Goal => {
                            ensure_live_goal(
                                &mut **tx,
                                tenant_id,
                                workspace_id,
                                request.candidate_set_id,
                                *target_id,
                            )
                            .await?;
                            (None, Some(*target_id))
                        }
                    };
                    sqlx::query("INSERT INTO scope_candidate_delta_evidence (tenant_id,workspace_id,candidate_set_id,evidence_id,candidate_id,goal_id,source_ref_id,summary) VALUES ($1,$2,$3,$4,$5,$6,$7,$8)")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(evidence_id)
                        .bind(candidate_id).bind(goal_id).bind(value.source_ref_id).bind(&value.summary)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                }
                Op::EvidenceUpdate {
                    evidence_id,
                    expected_revision,
                    value,
                } => {
                    ensure_source_ref(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        value.source_ref_id,
                    )
                    .await?;
                    let result = sqlx::query("UPDATE scope_candidate_delta_evidence SET revision=revision+1,source_ref_id=$5,summary=$6 WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND evidence_id=$4 AND revision=$7 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(evidence_id)
                        .bind(value.source_ref_id).bind(&value.summary).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
                Op::EvidenceRemove {
                    evidence_id,
                    expected_revision,
                } => {
                    let result = sqlx::query("UPDATE scope_candidate_delta_evidence SET revision=revision+1,deleted=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND evidence_id=$4 AND revision=$5 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(evidence_id).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
                Op::BlockerAdd {
                    blocker_id,
                    goal_id,
                    value,
                } => {
                    ensure_live_goal(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        *goal_id,
                    )
                    .await?;
                    ensure_source_ref(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        value.source_ref_id,
                    )
                    .await?;
                    sqlx::query("INSERT INTO scope_candidate_delta_blockers (tenant_id,workspace_id,candidate_set_id,blocker_id,goal_id,source_ref_id,summary) VALUES ($1,$2,$3,$4,$5,$6,$7)")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(blocker_id)
                        .bind(goal_id).bind(value.source_ref_id).bind(&value.summary)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                }
                Op::BlockerUpdate {
                    blocker_id,
                    expected_revision,
                    value,
                } => {
                    ensure_source_ref(
                        &mut **tx,
                        tenant_id,
                        workspace_id,
                        request.candidate_set_id,
                        value.source_ref_id,
                    )
                    .await?;
                    let result = sqlx::query("UPDATE scope_candidate_delta_blockers SET revision=revision+1,source_ref_id=$5,summary=$6 WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND blocker_id=$4 AND revision=$7 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(blocker_id)
                        .bind(value.source_ref_id).bind(&value.summary).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
                Op::BlockerRemove {
                    blocker_id,
                    expected_revision,
                } => {
                    let result = sqlx::query("UPDATE scope_candidate_delta_blockers SET revision=revision+1,deleted=true WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND blocker_id=$4 AND revision=$5 AND NOT deleted")
                        .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id).bind(blocker_id).bind(expected_revision)
                        .execute(&mut **tx).await.map_err(crate::storage_error)?;
                    if result.rows_affected() != 1 {
                        return Err(tect_domain::Error::StaleRevision);
                    }
                }
            }
        }
        let incomplete: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM scope_candidate_delta_goals g
             WHERE g.tenant_id=$1 AND g.workspace_id=$2 AND g.candidate_set_id=$3 AND g.finite AND NOT g.deleted
             AND NOT EXISTS(SELECT 1 FROM scope_candidate_delta_coverage cv
               JOIN scope_candidate_delta_candidates c ON c.tenant_id=cv.tenant_id AND c.workspace_id=cv.workspace_id
                AND c.candidate_set_id=cv.candidate_set_id AND c.candidate_id=cv.candidate_id AND NOT c.deleted
               WHERE cv.tenant_id=g.tenant_id AND cv.workspace_id=g.workspace_id AND cv.candidate_set_id=g.candidate_set_id AND cv.goal_id=g.goal_id)
             AND NOT EXISTS(SELECT 1 FROM scope_candidate_delta_blockers b
               WHERE b.tenant_id=g.tenant_id AND b.workspace_id=g.workspace_id AND b.candidate_set_id=g.candidate_set_id
                 AND b.goal_id=g.goal_id AND NOT b.deleted))")
            .bind(tenant_id).bind(workspace_id).bind(request.candidate_set_id)
            .fetch_one(&mut **tx).await.map_err(crate::storage_error)?;
        if incomplete {
            return Err(tect_domain::Error::refused(
                tect_domain::RefusalCode::CoverageIncomplete,
                "add_coverage_or_blocker",
                "finite_goal_coverage",
            ));
        }
        Ok(tect_domain::CandidateDeltaReceipt {
            candidate_set_id: request.candidate_set_id,
            idempotency_key: request.idempotency_key.clone(),
            from_revision: current,
            to_revision: next,
            stale_reasons: stale,
        })
    }

    async fn candidate_delta_status(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        idempotency_key: &str,
    ) -> Result<Option<tect_domain::CandidateDeltaReceipt>> {
        let tenant_id = self.tenant_id()?;
        let tx = self.transaction()?;
        let row = sqlx::query_as::<_, (i64, i64, serde_json::Value)>(
            "SELECT from_revision,to_revision,stale_reasons FROM scope_candidate_delta_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND idempotency_key=$4"
        ).bind(tenant_id).bind(workspace_id).bind(candidate_set_id).bind(idempotency_key)
        .fetch_optional(&mut **tx).await.map_err(crate::storage_error)?;
        row.map(|(from_revision, to_revision, stale_json)| {
            Ok(tect_domain::CandidateDeltaReceipt {
                candidate_set_id,
                idempotency_key: idempotency_key.to_owned(),
                from_revision,
                to_revision,
                stale_reasons: serde_json::from_value(stale_json).map_err(crate::storage_error)?,
            })
        })
        .transpose()
    }
}

async fn ensure_source_ref(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    source_ref_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scope_candidate_source_refs
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND id=$4)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(source_ref_id)
    .fetch_one(connection)
    .await
    .map_err(crate::storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(tect_domain::Error::NotFound)
    }
}

async fn ensure_live_candidate(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    candidate_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scope_candidate_delta_candidates
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND candidate_id=$4 AND NOT deleted)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(candidate_id)
    .fetch_one(connection)
    .await
    .map_err(crate::storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(tect_domain::Error::NotFound)
    }
}

async fn ensure_live_goal(
    connection: &mut sqlx::PgConnection,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    goal_id: Uuid,
) -> Result<()> {
    let exists: bool = sqlx::query_scalar(
        "SELECT EXISTS(SELECT 1 FROM scope_candidate_delta_goals
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND goal_id=$4 AND NOT deleted)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(goal_id)
    .fetch_one(connection)
    .await
    .map_err(crate::storage_error)?;
    if exists {
        Ok(())
    } else {
        Err(tect_domain::Error::NotFound)
    }
}
