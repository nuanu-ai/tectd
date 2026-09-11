use crate::TestResult;
use serde::Serialize;
use sqlx::PgPool;
use tect_postgres::admin::Enrollment;
use uuid::Uuid;

pub(crate) const SEEDED_WORKSPACES: i64 = 10_000;
pub(crate) const SEEDED_SESSIONS: i64 = 100_000;
pub(crate) const SEEDED_WORKTREES: i64 = 100;

pub(crate) struct SeedFixture {
    pub first_workspace_key: String,
    pub measured_native_ids: Vec<String>,
    pub worktree_ids: Vec<Uuid>,
}

#[derive(Debug, Clone, Serialize)]
pub(crate) struct Cardinalities {
    pub workspaces: i64,
    pub memberships: i64,
    pub sessions: i64,
    pub source_repositories: i64,
    pub source_worktrees: i64,
    pub session_worktrees: i64,
    pub workspace_events: i64,
}

pub(crate) async fn seed_fixture(
    pool: &PgPool,
    enrollment: &Enrollment,
    run_id: Uuid,
    measured_sessions: usize,
) -> TestResult<SeedFixture> {
    assert_eq!(measured_sessions, 10);
    let first_workspace_id = Uuid::new_v4();
    let first_workspace_key = format!("perf-seed-{}-primary", run_id.simple());
    let measured_session_ids: Vec<Uuid> = (0..measured_sessions).map(|_| Uuid::new_v4()).collect();
    let measured_native_ids: Vec<String> = (0..measured_sessions)
        .map(|_| Uuid::new_v4().to_string())
        .collect();
    let repository_id = Uuid::new_v4();
    let worktree_ids: Vec<Uuid> = (0..SEEDED_WORKTREES).map(|_| Uuid::new_v4()).collect();
    let workspace_prefix = format!("perf-seed-{}-", run_id.simple());
    let source_prefix = format!("/synthetic/tect-performance/{}/", run_id.simple());

    let mut transaction = pool.begin().await?;
    sqlx::query("INSERT INTO workspaces (id,tenant_id,key) VALUES ($1,$2,$3)")
        .bind(first_workspace_id)
        .bind(enrollment.tenant_id)
        .bind(&first_workspace_key)
        .execute(&mut *transaction)
        .await?;
    sqlx::query(
        "INSERT INTO workspaces (id,tenant_id,key) \
         SELECT pg_catalog.gen_random_uuid(),$1,$2 || gs::text \
         FROM pg_catalog.generate_series(1,9999) AS gs",
    )
    .bind(enrollment.tenant_id)
    .bind(&workspace_prefix)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO memberships (tenant_id,workspace_id,principal_id) \
         SELECT tenant_id,id,$2 FROM workspaces WHERE tenant_id=$1",
    )
    .bind(enrollment.tenant_id)
    .bind(enrollment.principal_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO agent_sessions \
             (id,tenant_id,host_id,workspace_id,native_session_id) \
         SELECT fixture.id,$1,$2,$3,fixture.native_id \
         FROM UNNEST($4::uuid[],$5::text[]) AS fixture(id,native_id)",
    )
    .bind(enrollment.tenant_id)
    .bind(enrollment.auth.host_id)
    .bind(first_workspace_id)
    .bind(&measured_session_ids)
    .bind(&measured_native_ids)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO agent_sessions \
             (id,tenant_id,host_id,workspace_id,native_session_id) \
         SELECT pg_catalog.gen_random_uuid(),$1,$2,w.id,pg_catalog.gen_random_uuid()::text \
         FROM workspaces AS w CROSS JOIN pg_catalog.generate_series(1,10) AS n \
         WHERE w.tenant_id=$1 AND w.id<>$3",
    )
    .bind(enrollment.tenant_id)
    .bind(enrollment.auth.host_id)
    .bind(first_workspace_id)
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO source_repositories \
             (id,tenant_id,workspace_id,host_id,common_dir) VALUES ($1,$2,$3,$4,$5)",
    )
    .bind(repository_id)
    .bind(enrollment.tenant_id)
    .bind(first_workspace_id)
    .bind(enrollment.auth.host_id)
    .bind(format!("{source_prefix}repository.git"))
    .execute(&mut *transaction)
    .await?;
    sqlx::query(
        "INSERT INTO source_worktrees \
             (id,tenant_id,workspace_id,host_id,repository_id,path) \
         SELECT fixture.id,$1,$2,$3,$4,$5 || LPAD(fixture.ordinality::text,3,'0') \
         FROM UNNEST($6::uuid[]) WITH ORDINALITY AS fixture(id,ordinality)",
    )
    .bind(enrollment.tenant_id)
    .bind(first_workspace_id)
    .bind(enrollment.auth.host_id)
    .bind(repository_id)
    .bind(format!("{source_prefix}worktree-"))
    .bind(&worktree_ids)
    .execute(&mut *transaction)
    .await?;
    transaction.commit().await?;

    let counts = cardinalities(pool, Some(enrollment.tenant_id)).await?;
    assert_eq!(counts.workspaces, SEEDED_WORKSPACES);
    assert_eq!(counts.memberships, SEEDED_WORKSPACES);
    assert_eq!(counts.sessions, SEEDED_SESSIONS);
    assert_eq!(counts.source_repositories, 1);
    assert_eq!(counts.source_worktrees, SEEDED_WORKTREES);
    assert_eq!(counts.session_worktrees, 0);
    Ok(SeedFixture {
        first_workspace_key,
        measured_native_ids,
        worktree_ids,
    })
}

pub(crate) async fn cardinalities(
    pool: &PgPool,
    tenant: Option<Uuid>,
) -> TestResult<Cardinalities> {
    Ok(Cardinalities {
        workspaces: count(pool, "workspaces", tenant).await?,
        memberships: count(pool, "memberships", tenant).await?,
        sessions: count(pool, "agent_sessions", tenant).await?,
        source_repositories: count(pool, "source_repositories", tenant).await?,
        source_worktrees: count(pool, "source_worktrees", tenant).await?,
        session_worktrees: count(pool, "session_worktrees", tenant).await?,
        workspace_events: count(pool, "workspace_events", tenant).await?,
    })
}

async fn count(pool: &PgPool, table: &str, tenant: Option<Uuid>) -> TestResult<i64> {
    let query = match tenant {
        Some(_) => format!("SELECT count(*) FROM {table} WHERE tenant_id=$1"),
        None => format!("SELECT count(*) FROM {table}"),
    };
    let mut query = sqlx::query_scalar::<_, i64>(&query);
    if let Some(tenant) = tenant {
        query = query.bind(tenant);
    }
    Ok(query.fetch_one(pool).await?)
}
