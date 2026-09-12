mod begin;
mod continuation;
mod history;
mod protected;
mod resolve;
mod save;
mod snapshot;
mod write;
#[cfg(test)]
mod write_tests;

pub(crate) use begin::{ensure, replay as begin_replay};
pub(crate) use history::{historical, history};
pub(crate) use save::{record_input, refresh, replay, save_draft, save_review};

use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{
    CandidateBoundary, CandidateContext, CandidateInputSummary, CandidateMethodSnapshot,
    CandidateRuleSnapshot, CandidateSet, CandidateSetStatus, CandidateSetSummary,
    CandidateSnapshot, CandidateSourceKind, CandidateSourceRef, CandidateTextFragment, Error,
    Program, ResolvedCandidateDraft, Result, ScopeCandidateReview, StoredCandidateContext,
};
use uuid::Uuid;

#[derive(sqlx::FromRow)]
struct SetRow {
    id: Uuid,
    workspace_id: Uuid,
    program_id: Uuid,
    revision: i64,
    status: String,
    boundary: String,
    current_snapshot_id: Option<Uuid>,
    input_cursor: i64,
    latest_input: i64,
    max_input_bytes: i64,
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
    program_body: String,
}

#[derive(sqlx::FromRow)]
struct SourceRefRow {
    id: Uuid,
    kind: String,
    input_sequence: Option<i64>,
    program_field: Option<String>,
    label: String,
}

#[derive(sqlx::FromRow)]
struct CandidateHeadRow {
    id: Uuid,
    program_id: Uuid,
    revision: i64,
    status: String,
    boundary: String,
    current_snapshot_id: Option<Uuid>,
    input_cursor: i64,
    latest_input: i64,
}

#[derive(sqlx::FromRow)]
struct FragmentRow {
    snapshot_id: Uuid,
    kind: String,
    input_sequence: Option<i64>,
    program_field: Option<String>,
    label: String,
    body: String,
}

pub(crate) async fn load(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
) -> Result<Option<StoredCandidateContext>> {
    let row = sqlx::query_as::<_, SetRow>(
        "SELECT id,workspace_id,program_id,revision,status,boundary,current_snapshot_id,\
                input_cursor,latest_input,max_input_bytes \
         FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    let snapshot_id = row.current_snapshot_id.ok_or(Error::InternalInvariant)?;
    let snapshot_row = sqlx::query_as::<_, SnapshotRow>(
        "SELECT s.id,s.sequence,s.program_revision,s.program_latest_input,\
                s.planning_latest_input,s.selected_worktree_ids,s.selected_sources_digest,\
                s.method_id,s.method_revision,s.method_digest,s.method_body,s.method_origin_refs,\
                s.registry_revision,s.registry_digest,s.rules,c.body AS program_body \
         FROM scope_candidate_snapshots s JOIN scope_candidate_contents c \
           ON c.tenant_id=s.tenant_id AND c.workspace_id=s.workspace_id \
          AND c.digest=s.program_body_digest \
         WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.candidate_set_id=$3 AND s.id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(snapshot_id)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let source_rows = sqlx::query_as::<_, SourceRefRow>(
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
    let draft: Option<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 \
         ORDER BY set_revision DESC LIMIT 1",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let reviews: Vec<serde_json::Value> = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_reviews \
         WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 \
         ORDER BY set_revision",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let set = CandidateSet {
        id: row.id,
        workspace_id: row.workspace_id,
        program_id: row.program_id,
        revision: row.revision,
        status: parse_status(&row.status)?,
        boundary: parse_boundary(&row.boundary)?,
        current_snapshot_id: snapshot_id,
        input_cursor: row.input_cursor,
        latest_input: row.latest_input,
        max_input_bytes: row.max_input_bytes,
    };
    let source_refs = source_rows
        .into_iter()
        .map(|row| {
            Ok(CandidateSourceRef {
                id: row.id,
                kind: parse_source_kind(&row.kind)?,
                input_sequence: row.input_sequence,
                program_field: row.program_field,
                label: row.label,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let snapshot = CandidateSnapshot {
        id: snapshot_row.id,
        sequence: snapshot_row.sequence,
        program_revision: snapshot_row.program_revision,
        program_latest_input: snapshot_row.program_latest_input,
        planning_latest_input: snapshot_row.planning_latest_input,
        selected_worktree_ids: snapshot_row.selected_worktree_ids,
        selected_sources_digest: snapshot_row.selected_sources_digest,
        method: CandidateMethodSnapshot {
            id: snapshot_row.method_id,
            revision: snapshot_row.method_revision,
            digest: snapshot_row.method_digest,
            body: snapshot_row.method_body,
            origin_refs: serde_json::from_value(snapshot_row.method_origin_refs)
                .map_err(storage_error)?,
        },
        registry_revision: snapshot_row.registry_revision,
        registry_digest: snapshot_row.registry_digest,
        rules: serde_json::from_value::<Vec<CandidateRuleSnapshot>>(snapshot_row.rules)
            .map_err(storage_error)?,
        source_refs,
    };
    Ok(Some(StoredCandidateContext {
        context: CandidateContext {
            candidate_set: set,
            snapshot,
            current_program_revision: snapshot_row.program_revision,
            stale_reasons: Vec::new(),
        },
        program: serde_json::from_str::<Program>(&snapshot_row.program_body)
            .map_err(storage_error)?,
        draft: draft
            .map(serde_json::from_value::<ResolvedCandidateDraft>)
            .transpose()
            .map_err(storage_error)?,
        reviews: reviews
            .into_iter()
            .map(serde_json::from_value::<ScopeCandidateReview>)
            .collect::<std::result::Result<Vec<_>, _>>()
            .map_err(storage_error)?,
    }))
}

pub(crate) async fn input_summaries(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    after: i64,
    limit: u32,
) -> Result<Vec<CandidateInputSummary>> {
    let rows: Vec<(Uuid, i64, Uuid, Uuid, Uuid)> = sqlx::query_as(
        "SELECT i.id,i.sequence,i.request_id,i.session_id,r.id FROM scope_candidate_inputs i \
         JOIN scope_candidate_sets s ON s.tenant_id=i.tenant_id AND s.workspace_id=i.workspace_id \
          AND s.id=i.candidate_set_id \
         JOIN scope_candidate_source_refs r ON r.tenant_id=i.tenant_id \
          AND r.workspace_id=i.workspace_id AND r.candidate_set_id=i.candidate_set_id \
          AND r.snapshot_id=s.current_snapshot_id AND r.kind='planning_input' \
          AND r.input_sequence=i.sequence \
         WHERE i.tenant_id=$1 AND i.workspace_id=$2 AND i.candidate_set_id=$3 AND i.sequence>$4 \
         ORDER BY i.sequence LIMIT $5",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(after)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(rows
        .into_iter()
        .map(
            |(id, sequence, request_id, session_id, source_ref_id)| CandidateInputSummary {
                id,
                sequence,
                request_id,
                session_id,
                source_ref_id,
            },
        )
        .collect())
}

pub(crate) async fn heads(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    limit: u32,
) -> Result<Vec<CandidateSetSummary>> {
    let rows = sqlx::query_as::<_, CandidateHeadRow>(
        "SELECT id,program_id,revision,status,boundary,current_snapshot_id,input_cursor,latest_input \
         FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 \
         ORDER BY created_at DESC,id LIMIT $3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    rows.into_iter()
        .map(|row| {
            Ok(CandidateSetSummary {
                id: row.id,
                program_id: row.program_id,
                revision: row.revision,
                status: parse_status(&row.status)?,
                boundary: parse_boundary(&row.boundary)?,
                snapshot_id: row.current_snapshot_id.ok_or(Error::InternalInvariant)?,
                input_cursor: row.input_cursor,
                latest_input: row.latest_input,
            })
        })
        .collect()
}

#[allow(clippy::too_many_arguments)]
pub(crate) async fn fragment(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    candidate_set_id: Uuid,
    snapshot_id: Option<Uuid>,
    source_ref_id: Uuid,
    cursor: usize,
    max_bytes: usize,
) -> Result<CandidateTextFragment> {
    if max_bytes < 4 {
        return Err(Error::InvalidArguments);
    }
    let row = sqlx::query_as::<_, FragmentRow>(
        "SELECT r.snapshot_id,r.kind,r.input_sequence,r.program_field,r.label,c.body \
         FROM scope_candidate_source_refs r \
         JOIN scope_candidate_sets s ON s.tenant_id=r.tenant_id AND s.workspace_id=r.workspace_id \
          AND s.id=r.candidate_set_id \
         JOIN scope_candidate_contents c ON c.tenant_id=r.tenant_id \
          AND c.workspace_id=r.workspace_id AND c.digest=r.body_digest \
         WHERE r.tenant_id=$1 AND r.workspace_id=$2 AND r.candidate_set_id=$3 AND r.id=$4 \
           AND (($5::uuid IS NULL AND s.current_snapshot_id=r.snapshot_id) OR r.snapshot_id=$5)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(candidate_set_id)
    .bind(source_ref_id)
    .bind(snapshot_id)
    .fetch_optional(&mut **transaction)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else {
        return Err(Error::NotFound);
    };
    let FragmentRow {
        snapshot_id,
        kind,
        input_sequence,
        program_field,
        label,
        body,
    } = row;
    if cursor > body.len() || !body.is_char_boundary(cursor) {
        return Err(Error::InvalidArguments);
    }
    let mut end = cursor.saturating_add(max_bytes).min(body.len());
    while end > cursor && !body.is_char_boundary(end) {
        end -= 1;
    }
    if cursor < body.len() && end == cursor {
        return Err(Error::InternalInvariant);
    }
    let next_source_ref_id = if kind == "program_field" || kind == "program_success" {
        let refs: Vec<Uuid> = sqlx::query_scalar(
            "SELECT id FROM scope_candidate_source_refs \
             WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND snapshot_id=$4 \
               AND kind IN ('program_field','program_success') \
             ORDER BY CASE program_field WHEN 'name' THEN 1 WHEN 'intent' THEN 2 \
               WHEN 'basis' THEN 3 WHEN 'boundaries' THEN 4 WHEN 'constraints' THEN 5 \
               WHEN 'success' THEN 6 ELSE 7 END,id",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(snapshot_id)
        .fetch_all(&mut **transaction)
        .await
        .map_err(storage_error)?;
        refs.iter()
            .position(|id| *id == source_ref_id)
            .and_then(|position| refs.get(position + 1))
            .copied()
    } else {
        None
    };
    Ok(CandidateTextFragment {
        source_ref: CandidateSourceRef {
            id: source_ref_id,
            kind: parse_source_kind(&kind)?,
            input_sequence,
            program_field,
            label,
        },
        snapshot_id,
        cursor,
        next_cursor: (end < body.len()).then_some(end),
        next_source_ref_id,
        text: body[cursor..end].to_owned(),
    })
}

fn parse_status(value: &str) -> Result<CandidateSetStatus> {
    match value {
        "draft" => Ok(CandidateSetStatus::Draft),
        "review_required" => Ok(CandidateSetStatus::ReviewRequired),
        "ready" => Ok(CandidateSetStatus::Ready),
        "blocked" => Ok(CandidateSetStatus::Blocked),
        _ => Err(Error::StorageUnavailable),
    }
}

fn parse_boundary(value: &str) -> Result<CandidateBoundary> {
    match value {
        "finite" => Ok(CandidateBoundary::Finite),
        "ongoing" => Ok(CandidateBoundary::Ongoing),
        _ => Err(Error::StorageUnavailable),
    }
}

fn parse_source_kind(value: &str) -> Result<CandidateSourceKind> {
    match value {
        "program_field" => Ok(CandidateSourceKind::ProgramField),
        "program_success" => Ok(CandidateSourceKind::ProgramSuccess),
        "planning_input" => Ok(CandidateSourceKind::PlanningInput),
        _ => Err(Error::StorageUnavailable),
    }
}

fn boundary_name(value: CandidateBoundary) -> &'static str {
    match value {
        CandidateBoundary::Finite => "finite",
        CandidateBoundary::Ongoing => "ongoing",
    }
}

fn status_name(value: CandidateSetStatus) -> &'static str {
    match value {
        CandidateSetStatus::Draft => "draft",
        CandidateSetStatus::ReviewRequired => "review_required",
        CandidateSetStatus::Ready => "ready",
        CandidateSetStatus::Blocked => "blocked",
    }
}
