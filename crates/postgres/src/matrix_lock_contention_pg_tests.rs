//! Isolated PostgreSQL proof of the shared lock/UoW guarantees. These tests do
//! not claim coverage of complete selected-save/verification/dispatch flows.
use crate::{admin, store::PgUnitOfWork};
use sqlx::PgPool;
use std::time::Duration;
use tect_application::{
    MatrixRequirementsContextStore, MatrixTaskStore, RecordMatrixTask, UnitOfWork,
};
use tect_domain::{Error, RequirementsAnchor};
use uuid::Uuid;

#[path = "model_route_live_tests/positive_input.rs"]
mod input;

async fn authenticated(pool: &PgPool, owner: &admin::Enrollment, native: &str) -> PgUnitOfWork {
    let mut uow = PgUnitOfWork::test_begin(pool, owner.tenant_id).await;
    uow.authenticate(&owner.auth).await.unwrap();
    uow.set_tenant(owner.tenant_id).await.unwrap();
    assert!(
        uow.session(owner.auth.host_id, native)
            .await
            .unwrap()
            .is_some()
    );
    uow
}

async fn marker(uow: &mut PgUnitOfWork, tenant: Uuid) -> Uuid {
    let id = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(id)
        .bind(tenant)
        .bind(format!("rollback-{id}"))
        .execute(&mut **uow.transaction().unwrap())
        .await
        .unwrap();
    id
}

async fn absent(pool: &PgPool, id: Uuid) {
    assert!(
        !sqlx::query_scalar::<_, bool>("SELECT EXISTS(SELECT 1 FROM workspaces WHERE id=$1)")
            .bind(id)
            .fetch_one(pool)
            .await
            .unwrap()
    );
}

#[tokio::test]
#[ignore = "requires identity-pinned disposable PostgreSQL 18 database; migrates and writes fixtures"]
async fn matrix_lock_contention_pg_authenticated_rollback_and_retry() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let expected_data = std::env::var("TECT_TEST_DATA_DIRECTORY").unwrap();
    let admin_pool = PgPool::connect(&admin_url).await.unwrap();
    // Pin the fresh cluster identity before any migrations or writes.
    let actual: (String, i32) = sqlx::query_as(
        "SELECT current_setting('data_directory'),current_setting('server_version_num')::integer",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(actual, (expected_data, 180006));
    admin::migrate(&admin_pool, &role).await.unwrap();
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    crate::runtime::verify_runtime_role(&runtime).await.unwrap();
    let role_flags: (bool, bool) =
        sqlx::query_as("SELECT rolsuper,rolbypassrls FROM pg_roles WHERE rolname=current_user")
            .fetch_one(&runtime)
            .await
            .unwrap();
    assert_eq!(role_flags, (false, false));
    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let workspace = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(owner.tenant_id)
        .bind(format!("locks-{workspace}"))
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(owner.tenant_id)
        .bind(workspace)
        .bind(owner.principal_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    let sessions = [Uuid::new_v4(), Uuid::new_v4()];
    for session in sessions {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(session).bind(owner.tenant_id).bind(owner.auth.host_id).bind(workspace)
            .bind(session.to_string()).execute(&admin_pool).await.unwrap();
    }
    assert_ne!(sessions[0], sessions[1]);
    // Runtime RLS hides this tenant without its transaction-local tenant scope.
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM workspaces WHERE id=$1")
            .bind(workspace)
            .fetch_one(&runtime)
            .await
            .unwrap(),
        0
    );
    let task = Uuid::new_v4();
    let request = RecordMatrixTask {
        task_id: task,
        revision: 1,
        expected_current_revision: 0,
        request_id: Uuid::new_v4(),
        input: input::matrix_input(),
        choice_set: None,
    };
    let canonical = serde_json::to_value(&request.input).unwrap();
    let digest = tect_application::canonical_matrix_input_digest(&canonical).unwrap();
    let mut setup = authenticated(&runtime, &owner, &sessions[0].to_string()).await;
    setup
        .record_matrix_task(
            workspace,
            owner.principal_id,
            sessions[0],
            &request,
            &canonical,
            &digest,
        )
        .await
        .unwrap();
    Box::new(setup).commit().await.unwrap();
    let anchor = RequirementsAnchor::Program {
        program_id: Uuid::new_v4(),
    };

    // Holder task -> contender declarations -> task: NOWAIT aborts the SQL tx,
    // releasing its earlier declarations lock and discarding earlier writes.
    let mut holder = authenticated(&runtime, &owner, &sessions[0].to_string()).await;
    holder
        .lock_matrix_task(workspace, task)
        .await
        .unwrap()
        .unwrap();
    let mut loser = authenticated(&runtime, &owner, &sessions[1].to_string()).await;
    let holder_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **holder.transaction().unwrap())
        .await
        .unwrap();
    let loser_pid: i32 = sqlx::query_scalar("SELECT pg_backend_pid()")
        .fetch_one(&mut **loser.transaction().unwrap())
        .await
        .unwrap();
    assert_ne!(holder_pid, loser_pid);
    loser
        .lock_matrix_requirements_head(workspace, anchor)
        .await
        .unwrap();
    let lost_marker = marker(&mut loser, owner.tenant_id).await;
    assert!(matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            loser.lock_matrix_task(workspace, task)
        )
        .await
        .unwrap(),
        Err(Error::StaleRevision)
    ));
    assert!(matches!(
        loser.matrix_task(workspace, task).await,
        Err(Error::StorageUnavailable)
    ));
    holder
        .lock_matrix_requirements_head(workspace, anchor)
        .await
        .unwrap();
    let _ = Box::new(loser).commit().await; // SQL COMMIT of an aborted tx may return ROLLBACK success.
    absent(&admin_pool, lost_marker).await;
    Box::new(holder).commit().await.unwrap();

    // The public owner-record path has its own direct task NOWAIT query.
    let mut holder = authenticated(&runtime, &owner, &sessions[0].to_string()).await;
    holder
        .lock_matrix_task(workspace, task)
        .await
        .unwrap()
        .unwrap();
    let mut loser = authenticated(&runtime, &owner, &sessions[1].to_string()).await;
    let lost_marker = marker(&mut loser, owner.tenant_id).await;
    let edit = RecordMatrixTask {
        revision: 2,
        expected_current_revision: 1,
        request_id: Uuid::new_v4(),
        ..request.clone()
    };
    assert!(matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            loser.record_matrix_task(
                workspace,
                owner.principal_id,
                sessions[1],
                &edit,
                &canonical,
                &digest
            )
        )
        .await
        .unwrap(),
        Err(Error::StaleRevision)
    ));
    let _ = Box::new(loser).commit().await;
    absent(&admin_pool, lost_marker).await;
    Box::new(holder).commit().await.unwrap();

    // Holder declarations -> contender task -> declarations: failed try-lock
    // explicitly removes and rolls back the UoW before a swallowed context error
    // could be converted into a no-call receipt or committed earlier writes.
    let mut holder = authenticated(&runtime, &owner, &sessions[0].to_string()).await;
    holder
        .lock_matrix_requirements_head(workspace, anchor)
        .await
        .unwrap();
    let mut loser = authenticated(&runtime, &owner, &sessions[1].to_string()).await;
    loser
        .lock_matrix_task(workspace, task)
        .await
        .unwrap()
        .unwrap();
    let lost_marker = marker(&mut loser, owner.tenant_id).await;
    assert!(matches!(
        tokio::time::timeout(
            Duration::from_secs(2),
            loser.lock_matrix_requirements_head(workspace, anchor)
        )
        .await
        .unwrap(),
        Err(Error::StaleRevision)
    ));
    assert!(matches!(
        loser.transaction(),
        Err(Error::StorageUnavailable)
    ));
    assert!(matches!(
        loser.matrix_task(workspace, task).await,
        Err(Error::StorageUnavailable)
    ));
    assert!(matches!(
        Box::new(loser).commit().await,
        Err(Error::StorageUnavailable)
    ));
    absent(&admin_pool, lost_marker).await;
    holder
        .lock_matrix_task(workspace, task)
        .await
        .unwrap()
        .unwrap();
    Box::new(holder).commit().await.unwrap();

    // Unchanged-source retry works; the accepted request replays, while a
    // genuinely stale task edit rejects without advancing the task head.
    let mut retry = authenticated(&runtime, &owner, &sessions[1].to_string()).await;
    retry
        .lock_matrix_requirements_head(workspace, anchor)
        .await
        .unwrap();
    retry
        .lock_matrix_task(workspace, task)
        .await
        .unwrap()
        .unwrap();
    let replay = retry
        .record_matrix_task(
            workspace,
            owner.principal_id,
            sessions[1],
            &request,
            &canonical,
            &digest,
        )
        .await
        .unwrap();
    assert_eq!(replay.revision, 1);
    let stale = RecordMatrixTask {
        revision: 2,
        expected_current_revision: 0,
        request_id: Uuid::new_v4(),
        ..request
    };
    assert!(matches!(
        retry
            .record_matrix_task(
                workspace,
                owner.principal_id,
                sessions[1],
                &stale,
                &canonical,
                &digest
            )
            .await,
        Err(Error::StaleRevision)
    ));
    Box::new(retry).commit().await.unwrap();
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT current_revision FROM matrix_tasks WHERE id=$1")
            .bind(task)
            .fetch_one(&admin_pool)
            .await
            .unwrap(),
        1
    );
    eprintln!(
        "PG18.6 authenticated distinct-session task/declaration contention, rollback, RLS, replay and stale task proof passed; complete service interleavings are outside this helper proof"
    );
}
