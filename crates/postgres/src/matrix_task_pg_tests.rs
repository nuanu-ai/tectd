//! Live PostgreSQL checks for migration 0047. Requires the PostgreSQL 18 test URLs.

use crate::admin;
use serde_json::{Value, json};
use sqlx::{PgPool, Postgres, Transaction};
use uuid::Uuid;

fn input(label: &str) -> Value {
    json!({
        "mode": "build", "envelope": {"label": label}, "criticality": "normal",
        "intent": label, "urgency": "normal", "promised_behavior": [],
        "promised_proof": [], "affected_guarantees": [], "actual_exposure": [],
        "demand_commitment": null, "latency_commitment": null, "urgent_repair": false
    })
}

async fn tenant(tx: &mut Transaction<'_, Postgres>, id: Uuid) {
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id', $1, true)")
        .bind(id.to_string())
        .execute(&mut **tx)
        .await
        .unwrap();
}

async fn revision(
    tx: &mut Transaction<'_, Postgres>,
    tenant_id: Uuid,
    workspace_id: Uuid,
    task_id: Uuid,
    revision: i64,
    request_id: Uuid,
    principal_id: Uuid,
    session_id: Uuid,
    payload: Value,
) -> Result<(), sqlx::Error> {
    sqlx::query(
        "INSERT INTO matrix_task_revisions
         (tenant_id, workspace_id, task_id, revision, previous_revision, request_id,
          input_schema, canonical_input, input_digest, recorded_by_principal_id,
          recorded_by_session_id)
         VALUES ($1,$2,$3,$4,$5,$6,'tect.engineering-matrix-input/1',$7,$8,$9,$10)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(task_id)
    .bind(revision)
    .bind((revision > 1).then_some(revision - 1))
    .bind(request_id)
    .bind(payload)
    .bind("a".repeat(64))
    .bind(principal_id)
    .bind(session_id)
    .execute(&mut **tx)
    .await
    .map(|_| ())
}

fn sqlstate(error: &sqlx::Error) -> String {
    error
        .as_database_error()
        .unwrap()
        .code()
        .unwrap()
        .into_owned()
}

#[tokio::test]
async fn matrix_task_revisions_enforce_atomic_owner_accepted_lineage() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let admin_pool = PgPool::connect(&admin_url).await.unwrap();
    let server_version: i32 =
        sqlx::query_scalar("SELECT current_setting('server_version_num')::integer")
            .fetch_one(&admin_pool)
            .await
            .unwrap();
    assert!(
        (180000..190000).contains(&server_version),
        "PostgreSQL 18 required, got {server_version}"
    );
    admin::migrate(&admin_pool, &role).await.unwrap();
    let runtime_pool = PgPool::connect(&runtime_url).await.unwrap();

    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let tenant_id = owner.tenant_id;
    let workspace_id = Uuid::new_v4();
    let other_workspace_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let other_session_id = Uuid::new_v4();
    for workspace in [workspace_id, other_workspace_id] {
        sqlx::query("INSERT INTO workspaces (id, tenant_id, key) VALUES ($1,$2,$3)")
            .bind(workspace)
            .bind(tenant_id)
            .bind(format!("matrix-{}", workspace.simple()))
            .execute(&admin_pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO memberships (tenant_id, workspace_id, principal_id) VALUES ($1,$2,$3)",
        )
        .bind(tenant_id)
        .bind(workspace)
        .bind(owner.principal_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    }
    for (session, workspace) in [
        (session_id, workspace_id),
        (other_session_id, other_workspace_id),
    ] {
        sqlx::query("INSERT INTO agent_sessions (id, tenant_id, host_id, workspace_id, native_session_id) VALUES ($1,$2,$3,$4,$5)")
            .bind(session)
            .bind(tenant_id)
            .bind(owner.auth.host_id)
            .bind(workspace)
            .bind(Uuid::new_v4().to_string())
            .execute(&admin_pool)
            .await
            .unwrap();
    }

    let task_id = Uuid::new_v4();
    let first_request = Uuid::new_v4();
    let first_input = input("first");
    let mut unpaired = runtime_pool.begin().await.unwrap();
    tenant(&mut unpaired, tenant_id).await;
    sqlx::query(
        "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) VALUES ($1,$2,$3,1)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(Uuid::new_v4())
    .execute(&mut *unpaired)
    .await
    .unwrap();
    assert_eq!(sqlstate(&unpaired.commit().await.unwrap_err()), "23503");

    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    sqlx::query(
        "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) VALUES ($1,$2,$3,1)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(task_id)
    .execute(&mut *tx)
    .await
    .unwrap();
    revision(
        &mut tx,
        tenant_id,
        workspace_id,
        task_id,
        1,
        first_request,
        owner.principal_id,
        session_id,
        first_input.clone(),
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();

    let second_request = Uuid::new_v4();
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    revision(
        &mut tx,
        tenant_id,
        workspace_id,
        task_id,
        2,
        second_request,
        owner.principal_id,
        session_id,
        input("second"),
    )
    .await
    .unwrap();
    assert_eq!(sqlx::query("UPDATE matrix_tasks SET current_revision=2 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND current_revision=1")
        .bind(tenant_id).bind(workspace_id).bind(task_id)
        .execute(&mut *tx).await.unwrap().rows_affected(), 1);
    tx.commit().await.unwrap();

    let current: i64 = sqlx::query_scalar("SELECT current_revision FROM matrix_tasks WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant_id).bind(workspace_id).bind(task_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(current, 2);
    let preserved: Value = sqlx::query_scalar("SELECT canonical_input FROM matrix_task_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=1")
        .bind(tenant_id).bind(workspace_id).bind(task_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(preserved, first_input);

    // A losing CAS leaves no orphan accepted revision after rollback.
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    revision(
        &mut tx,
        tenant_id,
        workspace_id,
        task_id,
        3,
        Uuid::new_v4(),
        owner.principal_id,
        session_id,
        input("stale"),
    )
    .await
    .unwrap();
    assert_eq!(sqlx::query("UPDATE matrix_tasks SET current_revision=3 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND current_revision=1")
        .bind(tenant_id).bind(workspace_id).bind(task_id)
        .execute(&mut *tx).await.unwrap().rows_affected(), 0);
    tx.rollback().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3")
        .bind(tenant_id).bind(workspace_id).bind(task_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(count, 2);

    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    assert_eq!(
        sqlstate(
            &revision(
                &mut tx,
                tenant_id,
                workspace_id,
                task_id,
                2,
                second_request,
                owner.principal_id,
                session_id,
                input("replay")
            )
            .await
            .unwrap_err()
        ),
        "23505"
    );
    tx.rollback().await.unwrap();

    for attempted in [1_i64, 2, 4] {
        let mut tx = runtime_pool.begin().await.unwrap();
        tenant(&mut tx, tenant_id).await;
        let error = sqlx::query("UPDATE matrix_tasks SET current_revision=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant_id).bind(workspace_id).bind(task_id).bind(attempted)
            .execute(&mut *tx).await.unwrap_err();
        assert_eq!(sqlstate(&error), "23514");
        tx.rollback().await.unwrap();
    }
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    let error = sqlx::query("UPDATE matrix_task_revisions SET canonical_input=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(input("mutated"))
        .execute(&mut *tx).await.unwrap_err();
    assert_eq!(sqlstate(&error), "42501");
    tx.rollback().await.unwrap();

    // The session's workspace must match the fact's workspace, even within one tenant.
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    assert_eq!(
        sqlstate(
            &revision(
                &mut tx,
                tenant_id,
                workspace_id,
                task_id,
                3,
                Uuid::new_v4(),
                owner.principal_id,
                other_session_id,
                input("wrong-workspace")
            )
            .await
            .unwrap_err()
        ),
        "42501"
    );
    tx.rollback().await.unwrap();

    let verifier_id = Uuid::new_v4();
    let verifier_host = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    sqlx::query("INSERT INTO principals (id, tenant_id, role) VALUES ($1,$2,'verifier')")
        .bind(verifier_id)
        .bind(tenant_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO hosts (id, tenant_id, principal_id, credential_digest) VALUES ($1,$2,$3,$4)",
    )
    .bind(verifier_host)
    .bind(tenant_id)
    .bind(verifier_id)
    .bind("b".repeat(64))
    .execute(&admin_pool)
    .await
    .unwrap();
    sqlx::query(
        "INSERT INTO memberships (tenant_id, workspace_id, principal_id) VALUES ($1,$2,$3)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(verifier_id)
    .execute(&admin_pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO agent_sessions (id, tenant_id, host_id, workspace_id, native_session_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(verifier_session).bind(tenant_id).bind(verifier_host).bind(workspace_id)
        .bind(Uuid::new_v4().to_string()).execute(&admin_pool).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    assert_eq!(
        sqlstate(
            &revision(
                &mut tx,
                tenant_id,
                workspace_id,
                task_id,
                3,
                Uuid::new_v4(),
                verifier_id,
                verifier_session,
                input("non-owner")
            )
            .await
            .unwrap_err()
        ),
        "42501"
    );
    tx.rollback().await.unwrap();

    admin::revoke_session(&admin_pool, session_id)
        .await
        .unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, tenant_id).await;
    assert_eq!(
        sqlstate(
            &revision(
                &mut tx,
                tenant_id,
                workspace_id,
                task_id,
                3,
                Uuid::new_v4(),
                owner.principal_id,
                session_id,
                input("revoked")
            )
            .await
            .unwrap_err()
        ),
        "42501"
    );
    tx.rollback().await.unwrap();

    let other_owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, other_owner.tenant_id).await;
    let visible: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM matrix_tasks WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .fetch_one(&mut *tx)
    .await
    .unwrap();
    assert_eq!(visible, 0);
    assert_eq!(
        sqlstate(
            &revision(
                &mut tx,
                tenant_id,
                workspace_id,
                task_id,
                3,
                Uuid::new_v4(),
                owner.principal_id,
                other_session_id,
                input("cross-tenant")
            )
            .await
            .unwrap_err()
        ),
        "42501"
    );
    tx.rollback().await.unwrap();

    let mut tx = runtime_pool.begin().await.unwrap();
    tenant(&mut tx, other_owner.tenant_id).await;
    let error = sqlx::query(
        "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) VALUES ($1,$2,$3,1)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(Uuid::new_v4())
    .execute(&mut *tx)
    .await
    .unwrap_err();
    assert_eq!(sqlstate(&error), "42501");
    tx.rollback().await.unwrap();
}
