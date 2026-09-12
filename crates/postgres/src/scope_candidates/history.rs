use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    CandidateHistoryEntry, CandidateHistoryStatus, CandidateMethodSnapshot, CandidateRuleSnapshot,
    CandidateSnapshot, CandidateSourceKind, CandidateSourceRef, Error, Result,
    StoredHistoricalCandidateDraft,
};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct DraftRow {
    snapshot_id: Uuid,
    input_cursor: i64,
    payload: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct HistoryRow {
    candidate_id: Uuid,
    candidate_revision: i64,
    title: String,
    first_draft_revision: i64,
    latest_draft_revision: i64,
    latest_snapshot_id: Uuid,
    status: String,
    superseded_reason: Option<String>,
    replacement_candidate_ids: Vec<Uuid>,
}

#[derive(sqlx::FromRow)]
struct SnapshotRow {
    id: Uuid,
    sequence: i64,
    program_revision: i64,
    program_latest_input: i64,
    planning_latest_input: i64,
    selected_worktree_ids: Vec<Uuid>,
    selected_sources_digest: String,
    method_id: String,
    method_revision: String,
    method_digest: String,
    method_body: String,
    method_origin_refs: serde_json::Value,
    registry_revision: String,
    registry_digest: String,
    rules: serde_json::Value,
}

#[derive(sqlx::FromRow)]
struct SourceRow {
    id: Uuid,
    kind: String,
    input_sequence: Option<i64>,
    program_field: Option<String>,
    label: String,
}

async fn draft(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    draft_revision: i64,
) -> Result<Option<DraftRow>> {
    sqlx::query_as(
        "SELECT (r.request_payload->>'snapshot_id')::uuid snapshot_id,\
                (r.request_payload->>'input_cursor')::bigint input_cursor,d.payload \
         FROM scope_candidate_drafts d JOIN scope_candidate_receipts r \
           ON r.tenant_id=d.tenant_id AND r.workspace_id=d.workspace_id \
          AND r.candidate_set_id=d.candidate_set_id AND r.operation='save_draft' \
          AND r.result_revision=d.set_revision \
         WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.candidate_set_id=$3 \
           AND d.set_revision=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(draft_revision)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)
}

pub(crate) async fn history(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    after: i64,
    limit: u32,
) -> Result<Vec<CandidateHistoryEntry>> {
    let rows: Vec<HistoryRow> = sqlx::query_as(
        "WITH drafts AS (\
           SELECT d.set_revision,d.payload,(r.request_payload->>'snapshot_id')::uuid snapshot_id \
           FROM scope_candidate_drafts d JOIN scope_candidate_receipts r \
             ON r.tenant_id=d.tenant_id AND r.workspace_id=d.workspace_id \
            AND r.candidate_set_id=d.candidate_set_id AND r.operation='save_draft' \
            AND r.result_revision=d.set_revision \
           WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.candidate_set_id=$3\
         ), occurrences AS (\
           SELECT (c->>'id')::uuid candidate_id,(c->>'revision')::bigint candidate_revision,\
             c->>'title' title,set_revision,snapshot_id \
           FROM drafts CROSS JOIN LATERAL jsonb_array_elements(payload->'candidates') c\
         ), versions AS (\
           SELECT candidate_id,candidate_revision,min(title) title,min(set_revision) first_draft_revision,\
             max(set_revision) latest_draft_revision,\
             (array_agg(snapshot_id ORDER BY set_revision DESC))[1] latest_snapshot_id \
           FROM occurrences GROUP BY candidate_id,candidate_revision\
         ), latest AS (\
           SELECT candidate_id,max(candidate_revision) latest_candidate_revision \
           FROM versions GROUP BY candidate_id\
         ), superseded AS (\
           SELECT DISTINCT ON ((s->'prior'->>'id')::uuid,(s->'prior'->>'revision')::bigint)\
             (s->'prior'->>'id')::uuid candidate_id,(s->'prior'->>'revision')::bigint candidate_revision,\
             s->'prior'->>'title' title,set_revision,snapshot_id,s->>'reason' reason,\
             ARRAY(SELECT jsonb_array_elements_text(s->'replacement_candidate_ids')::uuid) replacements \
           FROM drafts CROSS JOIN LATERAL jsonb_array_elements(COALESCE(payload->'delta'->'superseded','[]')) s \
           ORDER BY (s->'prior'->>'id')::uuid,(s->'prior'->>'revision')::bigint,set_revision DESC\
         ) \
         SELECT v.candidate_id,v.candidate_revision,v.title,v.first_draft_revision,\
           COALESCE(s.set_revision,v.latest_draft_revision) latest_draft_revision,\
           COALESCE(s.snapshot_id,v.latest_snapshot_id) latest_snapshot_id,\
           CASE WHEN s.candidate_id IS NOT NULL THEN 'superseded' \
                WHEN l.latest_candidate_revision=v.candidate_revision THEN 'active' ELSE 'prior' END status,\
           s.reason superseded_reason,COALESCE(s.replacements,ARRAY[]::uuid[]) replacement_candidate_ids \
         FROM versions v JOIN latest l USING (candidate_id) \
         LEFT JOIN superseded s USING (candidate_id,candidate_revision) \
         ORDER BY v.candidate_id,v.candidate_revision OFFSET $4 LIMIT $5",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(after)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(CandidateHistoryEntry {
                candidate_id: row.candidate_id,
                candidate_revision: row.candidate_revision,
                title: row.title,
                first_draft_revision: row.first_draft_revision,
                latest_draft_revision: row.latest_draft_revision,
                latest_snapshot_id: row.latest_snapshot_id,
                status: history_status(&row.status)?,
                superseded_reason: row.superseded_reason,
                replacement_candidate_ids: row.replacement_candidate_ids,
            })
        })
        .collect()
}

pub(crate) async fn historical(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    draft_revision: i64,
) -> Result<Option<StoredHistoricalCandidateDraft>> {
    let row = draft(
        transaction,
        tenant_id,
        workspace_id,
        candidate_set_id,
        draft_revision,
    )
    .await?;
    let Some(row) = row else { return Ok(None) };
    let snapshot = snapshot(
        transaction,
        tenant_id,
        workspace_id,
        candidate_set_id,
        row.snapshot_id,
    )
    .await?;
    Ok(Some(StoredHistoricalCandidateDraft {
        snapshot,
        input_cursor: row.input_cursor,
        draft: serde_json::from_value(row.payload).map_err(storage_error)?,
    }))
}

fn history_status(value: &str) -> Result<CandidateHistoryStatus> {
    match value {
        "active" => Ok(CandidateHistoryStatus::Active),
        "prior" => Ok(CandidateHistoryStatus::Prior),
        "superseded" => Ok(CandidateHistoryStatus::Superseded),
        _ => Err(Error::StorageUnavailable),
    }
}

async fn snapshot(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    snapshot_id: Uuid,
) -> Result<CandidateSnapshot> {
    let row = sqlx::query_as::<_, SnapshotRow>(
        "SELECT id,sequence,program_revision,program_latest_input,planning_latest_input,\
                selected_worktree_ids,selected_sources_digest,method_id,method_revision,\
                method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules \
         FROM scope_candidate_snapshots WHERE tenant_id=$1 AND workspace_id=$2 \
           AND candidate_set_id=$3 AND id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(snapshot_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let sources = sqlx::query_as::<_, SourceRow>(
        "SELECT id,kind,input_sequence,program_field,label FROM scope_candidate_source_refs \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND snapshot_id=$4 \
         ORDER BY kind,input_sequence NULLS FIRST,id",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(snapshot_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(CandidateSnapshot {
        id: row.id,
        sequence: row.sequence,
        program_revision: row.program_revision,
        program_latest_input: row.program_latest_input,
        planning_latest_input: row.planning_latest_input,
        selected_worktree_ids: row.selected_worktree_ids,
        selected_sources_digest: row.selected_sources_digest,
        method: CandidateMethodSnapshot {
            id: row.method_id,
            revision: row.method_revision,
            digest: row.method_digest,
            body: row.method_body,
            origin_refs: serde_json::from_value(row.method_origin_refs).map_err(storage_error)?,
        },
        registry_revision: row.registry_revision,
        registry_digest: row.registry_digest,
        rules: serde_json::from_value::<Vec<CandidateRuleSnapshot>>(row.rules)
            .map_err(storage_error)?,
        source_refs: sources
            .into_iter()
            .map(|source| {
                Ok(CandidateSourceRef {
                    id: source.id,
                    kind: source_kind(&source.kind)?,
                    input_sequence: source.input_sequence,
                    program_field: source.program_field,
                    label: source.label,
                })
            })
            .collect::<Result<_>>()?,
    })
}

fn source_kind(value: &str) -> Result<CandidateSourceKind> {
    match value {
        "program_field" => Ok(CandidateSourceKind::ProgramField),
        "program_success" => Ok(CandidateSourceKind::ProgramSuccess),
        "planning_input" => Ok(CandidateSourceKind::PlanningInput),
        _ => Err(Error::StorageUnavailable),
    }
}
