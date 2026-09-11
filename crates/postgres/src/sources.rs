use crate::storage_error;
use sqlx::{Postgres, Transaction};
use tect_domain::{Error, RegisteredSource, Result, SourceLocation, WorktreeSummary};
use uuid::Uuid;

pub(crate) async fn register_source(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    location: &SourceLocation,
) -> Result<RegisteredSource> {
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
             pg_catalog.hashtextextended(\
                 $1::text || ':' || $2::text || ':' || $3::text || ':' || $4, 0))",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(&location.common_dir)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;

    sqlx::query(
        "INSERT INTO source_repositories \
             (id, tenant_id, workspace_id, host_id, common_dir) \
         VALUES (pg_catalog.gen_random_uuid(), $1, $2, $3, $4) \
         ON CONFLICT (tenant_id, workspace_id, host_id, common_dir) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(&location.common_dir)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;

    let repository_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM source_repositories \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND common_dir=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(&location.common_dir)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;

    sqlx::query(
        "INSERT INTO source_worktrees \
             (id, tenant_id, workspace_id, host_id, repository_id, path) \
         VALUES (pg_catalog.gen_random_uuid(), $1, $2, $3, $4, $5) \
         ON CONFLICT (tenant_id, workspace_id, host_id, path) DO NOTHING",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(repository_id)
    .bind(&location.worktree_path)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;

    let (id, stored_repository_id): (Uuid, Uuid) = sqlx::query_as(
        "SELECT id, repository_id FROM source_worktrees \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND path=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(&location.worktree_path)
    .fetch_one(&mut **transaction)
    .await
    .map_err(storage_error)?;
    if stored_repository_id != repository_id {
        return Err(Error::InvalidSource);
    }

    Ok(RegisteredSource {
        id,
        repository_id,
        path: location.worktree_path.clone(),
        common_dir: location.common_dir.clone(),
    })
}

pub(crate) async fn source_worktrees(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    ids: &[Uuid],
) -> Result<Vec<WorktreeSummary>> {
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT id, repository_id, path FROM source_worktrees \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND id = ANY($4::uuid[]) \
         ORDER BY id",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(ids)
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(rows.into_iter().map(worktree_summary).collect())
}

pub(crate) async fn replace_selection(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
    ids: &[Uuid],
) -> Result<()> {
    sqlx::query(
        "DELETE FROM session_worktrees \
         WHERE tenant_id=$1 AND workspace_id=$2 AND host_id=$3 AND session_id=$4",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(session_id)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;

    sqlx::query(
        "INSERT INTO session_worktrees \
             (tenant_id, workspace_id, host_id, session_id, worktree_id) \
         SELECT $1, $2, $3, $4, worktree_id \
         FROM pg_catalog.unnest($5::uuid[]) AS selected(worktree_id) \
         ON CONFLICT DO NOTHING",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(session_id)
    .bind(ids)
    .execute(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(crate) async fn selected_worktrees(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    session_id: Uuid,
) -> Result<Vec<WorktreeSummary>> {
    let rows: Vec<(Uuid, Uuid, String)> = sqlx::query_as(
        "SELECT sw.id, sw.repository_id, sw.path \
         FROM session_worktrees AS selected \
         JOIN source_worktrees AS sw \
           ON sw.tenant_id=selected.tenant_id \
          AND sw.workspace_id=selected.workspace_id \
          AND sw.host_id=selected.host_id \
          AND sw.id=selected.worktree_id \
         WHERE selected.tenant_id=$1 AND selected.workspace_id=$2 \
           AND selected.host_id=$3 AND selected.session_id=$4 \
         ORDER BY sw.id \
         LIMIT 100",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(session_id)
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(rows.into_iter().map(worktree_summary).collect())
}

pub(crate) async fn list_sources(
    transaction: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    host_id: Uuid,
    after: Option<Uuid>,
    limit: u32,
) -> Result<Vec<RegisteredSource>> {
    let rows: Vec<(Uuid, Uuid, String, String)> = sqlx::query_as(
        "SELECT sw.id, sw.repository_id, sw.path, repository.common_dir \
         FROM source_worktrees AS sw \
         JOIN source_repositories AS repository \
           ON repository.tenant_id=sw.tenant_id \
          AND repository.workspace_id=sw.workspace_id \
          AND repository.host_id=sw.host_id \
          AND repository.id=sw.repository_id \
         WHERE sw.tenant_id=$1 AND sw.workspace_id=$2 AND sw.host_id=$3 \
           AND ($4::uuid IS NULL OR sw.id > $4) \
         ORDER BY sw.id \
         LIMIT $5",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(host_id)
    .bind(after)
    .bind(i64::from(limit))
    .fetch_all(&mut **transaction)
    .await
    .map_err(storage_error)?;
    Ok(rows
        .into_iter()
        .map(|(id, repository_id, path, common_dir)| RegisteredSource {
            id,
            repository_id,
            path,
            common_dir,
        })
        .collect())
}

fn worktree_summary((id, repository_id, path): (Uuid, Uuid, String)) -> WorktreeSummary {
    WorktreeSummary {
        id,
        repository_id,
        path,
    }
}
