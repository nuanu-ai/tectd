//! Real PostgreSQL acceptance for idempotent Runtime tenant and host registration.

use sqlx::PgPool;
use std::fs;
use std::os::unix::fs::PermissionsExt;
use tect_domain::{Error, HostAuth};
use tect_postgres::admin;
use uuid::Uuid;

fn credential() -> String {
    format!("{}{}", Uuid::new_v4().simple(), Uuid::new_v4().simple())
}

async fn command(admin_url: &str, arguments: &[&str]) -> std::process::Output {
    tokio::process::Command::new(env!("CARGO_BIN_EXE_tect-admin"))
        .args(arguments)
        .env("TECT_ADMIN_DATABASE_URL", admin_url)
        .output()
        .await
        .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn runtime_tenant_and_host_registration_is_atomic_idempotent_and_secret_free() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();

    assert_eq!(
        admin::ensure_tenant(&pool, Uuid::nil()).await,
        Err(Error::InvalidArguments)
    );
    let tenant_id = Uuid::new_v4();
    let (left, right) = tokio::join!(
        admin::ensure_tenant(&pool, tenant_id),
        admin::ensure_tenant(&pool, tenant_id)
    );
    let left = left.unwrap();
    let right = right.unwrap();
    assert_eq!(left, right);
    let ensured = command(
        &admin_url,
        &["ensure-tenant", "--tenant", &tenant_id.to_string()],
    )
    .await;
    assert!(ensured.status.success(), "ensure-tenant retry failed");
    assert_eq!(
        String::from_utf8_lossy(&ensured.stdout),
        format!(
            "tenant {} principal {}\n",
            left.tenant_id, left.principal_id
        )
    );
    let owners: i64 =
        sqlx::query_scalar("SELECT count(*) FROM principals WHERE tenant_id=$1 AND role='owner'")
            .bind(tenant_id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(owners, 1);

    let malformed = Uuid::new_v4();
    let malformed_identity = admin::ensure_tenant(&pool, malformed).await.unwrap();
    sqlx::query("DELETE FROM principals WHERE id=$1")
        .bind(malformed_identity.principal_id)
        .execute(&pool)
        .await
        .unwrap();
    assert_eq!(
        admin::ensure_tenant(&pool, malformed).await,
        Err(Error::InternalInvariant)
    );
    sqlx::query("INSERT INTO principals (id, tenant_id, role) VALUES ($1, $2, 'owner')")
        .bind(malformed_identity.principal_id)
        .bind(malformed)
        .execute(&pool)
        .await
        .unwrap();

    let temp = tempfile::tempdir().unwrap();
    fs::set_permissions(temp.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let root = temp.path().canonicalize().unwrap();
    let source_a = root.join("source-a");
    let source_b = root.join("source-b");
    let setup_a = root.join("setup-a");
    for directory in [&source_a, &source_b, &setup_a] {
        fs::create_dir(directory).unwrap();
    }
    let source_a = source_a
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let source_b = source_b
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();
    let setup_a = setup_a
        .canonicalize()
        .unwrap()
        .to_string_lossy()
        .into_owned();

    let auth = HostAuth {
        host_id: Uuid::new_v4(),
        credential: credential(),
    };
    let (left, right) = tokio::join!(
        admin::register_host(
            &pool,
            tenant_id,
            &auth,
            vec![source_a.clone(), source_b.clone()],
            vec![setup_a.clone()],
        ),
        admin::register_host(
            &pool,
            tenant_id,
            &auth,
            vec![source_b.clone(), source_a.clone(), source_a.clone()],
            vec![setup_a.clone(), setup_a.clone()],
        )
    );
    assert_eq!(left.unwrap(), right.unwrap());
    let host_count: i64 = sqlx::query_scalar("SELECT count(*) FROM hosts WHERE id=$1")
        .bind(auth.host_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(host_count, 1);

    let changed_credential = HostAuth {
        host_id: auth.host_id,
        credential: credential(),
    };
    assert_eq!(
        admin::register_host(
            &pool,
            tenant_id,
            &changed_credential,
            vec![source_a.clone(), source_b.clone()],
            vec![setup_a.clone()],
        )
        .await,
        Err(Error::Unauthorized)
    );
    let duplicate_credential = HostAuth {
        host_id: Uuid::new_v4(),
        credential: auth.credential.clone(),
    };
    assert_eq!(
        admin::register_host(
            &pool,
            tenant_id,
            &duplicate_credential,
            vec![source_a.clone(), source_b.clone()],
            vec![setup_a.clone()],
        )
        .await,
        Err(Error::Unauthorized)
    );
    let other_tenant = admin::ensure_tenant(&pool, Uuid::new_v4()).await.unwrap();
    assert_eq!(
        admin::register_host(
            &pool,
            other_tenant.tenant_id,
            &auth,
            vec![source_a.clone(), source_b.clone()],
            vec![setup_a.clone()],
        )
        .await,
        Err(Error::Unauthorized)
    );
    assert_eq!(
        admin::register_host(
            &pool,
            tenant_id,
            &auth,
            vec![source_a.clone()],
            vec![setup_a.clone()],
        )
        .await,
        Err(Error::Unauthorized)
    );

    let auth_file = root.join("host.json");
    fs::write(&auth_file, serde_json::to_vec(&auth).unwrap()).unwrap();
    fs::set_permissions(&auth_file, fs::Permissions::from_mode(0o600)).unwrap();
    let output = command(
        &admin_url,
        &[
            "register-host",
            "--tenant",
            &tenant_id.to_string(),
            "--auth-file",
            auth_file.to_str().unwrap(),
            "--source-root",
            &source_b,
            "--source-root",
            &source_a,
            "--setup-root",
            &setup_a,
        ],
    )
    .await;
    assert!(output.status.success(), "register-host retry failed");
    assert!(!String::from_utf8_lossy(&output.stdout).contains(&auth.credential));
    assert!(!String::from_utf8_lossy(&output.stderr).contains(&auth.credential));

    admin::revoke_host(&pool, auth.host_id).await.unwrap();
    assert_eq!(
        admin::register_host(
            &pool,
            tenant_id,
            &auth,
            vec![source_a, source_b],
            vec![setup_a],
        )
        .await,
        Err(Error::Unauthorized)
    );
}
