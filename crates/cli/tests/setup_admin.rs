//! Real PostgreSQL operator-grant acceptance. Uses only the isolated test database.

use sqlx::PgPool;
use std::fs;
use std::os::unix::fs::{MetadataExt, PermissionsExt};
use std::path::Path;
use tect_application::{Store, TransactionMode};
use tect_domain::Error;
use tect_postgres::{PgStore, admin};
use uuid::Uuid;

fn private_temp() -> tempfile::TempDir {
    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    temp
}

async fn command(admin_url: &str, arguments: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .args(arguments)
        .env("TECT_ADMIN_DATABASE_URL", admin_url)
        .output()
        .await
        .unwrap()
}

fn assert_private(path: &Path) {
    let metadata = fs::metadata(path).unwrap();
    assert_eq!(metadata.mode() & 0o777, 0o600);
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enrollment_and_existing_host_grants_are_separate_bounded_and_concurrent() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let source_root = root.join("source");
    let setup_a = root.join("setup-a");
    let setup_b = root.join("setup-b");
    let setup_c = root.join("setup-c");
    let setup_d = root.join("setup-d");
    for directory in [&source_root, &setup_a, &setup_b, &setup_c, &setup_d] {
        fs::create_dir(directory).unwrap();
    }
    let auth_path = root.join("host.json");
    let output = command(
        &admin_url,
        &[
            "enroll",
            "--source-root",
            source_root.to_str().unwrap(),
            "--setup-root",
            setup_a.to_str().unwrap(),
            "--setup-root",
            setup_a.to_str().unwrap(),
            "--out",
            auth_path.to_str().unwrap(),
        ],
    )
    .await;
    assert!(output.status.success(), "enroll failed");
    assert_private(&auth_path);
    let auth: tect_domain::HostAuth =
        serde_json::from_slice(&fs::read(&auth_path).unwrap()).unwrap();
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&auth.credential));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&auth.credential));

    type HostRow = (
        Uuid,
        Uuid,
        String,
        sqlx::types::Json<Vec<String>>,
        sqlx::types::Json<Vec<String>>,
        bool,
    );
    let before: HostRow = sqlx::query_as(
        "SELECT tenant_id, principal_id, credential_digest, allowed_source_roots, \
                allowed_setup_roots, revoked FROM hosts WHERE id=$1",
    )
    .bind(auth.host_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(before.3.0, vec![source_root.to_str().unwrap()]);
    assert_eq!(before.4.0, vec![setup_a.to_str().unwrap()]);

    let store = PgStore::connect(&runtime_url, 2).await.unwrap();
    let mut transaction = store.begin(TransactionMode::ReadOnly).await.unwrap();
    let identity = transaction.authenticate(&auth).await.unwrap();
    assert_eq!(&identity.allowed_source_roots, &before.3.0);
    assert_eq!(&identity.allowed_setup_roots, &before.4.0);
    transaction.set_tenant(identity.tenant_id).await.unwrap();
    transaction.commit().await.unwrap();

    let legacy = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let legacy_setup_roots: sqlx::types::Json<Vec<String>> =
        sqlx::query_scalar("SELECT allowed_setup_roots FROM hosts WHERE id=$1")
            .bind(legacy.auth.host_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert!(legacy_setup_roots.0.is_empty());

    let output = command(
        &admin_url,
        &[
            "grant-setup-root",
            "--host-id",
            &auth.host_id.to_string(),
            "--setup-root",
            setup_b.to_str().unwrap(),
        ],
    )
    .await;
    assert!(output.status.success(), "grant failed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&auth.credential));

    let (left, right) = tokio::join!(
        admin::grant_setup_root(&pool, auth.host_id, setup_c.to_str().unwrap().to_owned()),
        admin::grant_setup_root(&pool, auth.host_id, setup_d.to_str().unwrap().to_owned())
    );
    left.unwrap();
    right.unwrap();
    admin::grant_setup_root(&pool, auth.host_id, setup_b.to_str().unwrap().to_owned())
        .await
        .unwrap();

    let after: HostRow = sqlx::query_as(
        "SELECT tenant_id, principal_id, credential_digest, allowed_source_roots, \
                allowed_setup_roots, revoked FROM hosts WHERE id=$1",
    )
    .bind(auth.host_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (&after.0, &after.1, &after.2, &after.3, &after.5),
        (&before.0, &before.1, &before.2, &before.3, &before.5)
    );
    assert_eq!(after.4.0.len(), 4);
    for expected in [&setup_a, &setup_b, &setup_c, &setup_d] {
        assert!(
            after
                .4
                .0
                .iter()
                .any(|root| root == expected.to_str().unwrap())
        );
    }

    assert_eq!(
        admin::grant_setup_root(&pool, Uuid::nil(), setup_a.to_str().unwrap().to_owned()).await,
        Err(Error::InvalidArguments)
    );
    assert_eq!(
        admin::grant_setup_root(&pool, Uuid::new_v4(), setup_a.to_str().unwrap().to_owned()).await,
        Err(Error::NotFound)
    );
    assert_eq!(
        admin::grant_setup_root(&pool, auth.host_id, "relative".into()).await,
        Err(Error::InvalidArguments)
    );
    admin::revoke_host(&pool, auth.host_id).await.unwrap();
    assert_eq!(
        admin::grant_setup_root(&pool, auth.host_id, setup_a.to_str().unwrap().to_owned()).await,
        Err(Error::Unauthorized)
    );
}

#[tokio::test]
async fn setup_root_cli_classifies_shape_and_filesystem_failures() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let temp = private_temp();
    let missing = temp.path().join("missing");
    let output = command(
        &admin_url,
        &[
            "grant-setup-root",
            "--host-id",
            "00000000-0000-0000-0000-000000000000",
            "--setup-root",
            temp.path().to_str().unwrap(),
        ],
    )
    .await;
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: invalid_arguments\n"
    );

    let output = command(
        &admin_url,
        &[
            "grant-setup-root",
            "--host-id",
            &Uuid::new_v4().to_string(),
            "--setup-root",
            missing.to_str().unwrap(),
        ],
    )
    .await;
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: setup_unavailable\n"
    );

    let output = command(
        &admin_url,
        &[
            "grant-setup-root",
            "--host-id",
            &Uuid::new_v4().to_string(),
            "--setup-root",
            "relative",
        ],
    )
    .await;
    assert!(!output.status.success());
    assert_eq!(
        String::from_utf8_lossy(&output.stderr),
        "error: invalid_arguments\n"
    );
}
