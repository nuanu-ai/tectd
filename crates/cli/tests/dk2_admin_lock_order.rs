//! Real PostgreSQL lock ordering for concurrent DK2 ADMIN setup.
use sqlx::PgPool;
use sqlx::postgres::PgPoolOptions;
use std::time::Duration;
use tect_postgres::admin;
use uuid::Uuid;

fn tagged_url(url: &str, tag: &str) -> String {
    format!(
        "{url}{}application_name={tag}",
        if url.contains('?') { "&" } else { "?" }
    )
}

async fn wait_until_blocked_by(pool: &PgPool, waiter: &str, blocker_pid: i32) {
    tokio::time::timeout(Duration::from_secs(10), async {
        loop {
            let blocked: bool = sqlx::query_scalar(
                "SELECT EXISTS (SELECT 1 FROM pg_catalog.pg_stat_activity \
                 WHERE application_name=$1 AND wait_event_type='Lock' \
                 AND wait_event='advisory' AND $2=ANY(pg_catalog.pg_blocking_pids(pid)))",
            )
            .bind(waiter)
            .bind(blocker_pid)
            .fetch_one(pool)
            .await
            .unwrap();
            if blocked {
                return;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .expect("expected advisory lock wait on the selected backend");
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn enable_serializes_with_migrate_before_taking_publisher_lock() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let observer = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&observer, &role).await.unwrap();

    let enable_tag = format!("dk2-enable-{}", Uuid::new_v4());
    let migrate_tag = format!("dk2-migrate-{}", Uuid::new_v4());
    let enable_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&tagged_url(&admin_url, &enable_tag))
        .await
        .unwrap();
    let migrate_pool = PgPoolOptions::new()
        .max_connections(1)
        .connect(&tagged_url(&admin_url, &migrate_tag))
        .await
        .unwrap();

    // Hold the publisher gate. Enable must own the runtime-grants gate while
    // waiting here, so a concurrent migrate waits behind enable, not vice versa.
    let mut publisher = observer.begin().await.unwrap();
    let publisher_pid: i32 = sqlx::query_scalar("SELECT pg_catalog.pg_backend_pid()")
        .fetch_one(&mut *publisher)
        .await
        .unwrap();
    sqlx::query(
        "SELECT pg_catalog.pg_advisory_xact_lock(\
         pg_catalog.hashtextextended('tect-dk-native-publisher', 0))",
    )
    .execute(&mut *publisher)
    .await
    .unwrap();

    let enable = tokio::spawn({
        let pool = enable_pool.clone();
        let role = role.clone();
        async move { tect_postgres::enable_durable_knowledge(&pool, &role).await }
    });
    wait_until_blocked_by(&observer, &enable_tag, publisher_pid).await;
    let enable_pid: i32 =
        sqlx::query_scalar("SELECT pid FROM pg_catalog.pg_stat_activity WHERE application_name=$1")
            .bind(&enable_tag)
            .fetch_one(&observer)
            .await
            .unwrap();

    let migrate = tokio::spawn({
        let pool = migrate_pool.clone();
        let role = role.clone();
        async move { admin::migrate(&pool, &role).await }
    });
    wait_until_blocked_by(&observer, &migrate_tag, enable_pid).await;
    publisher.commit().await.unwrap();
    tokio::time::timeout(Duration::from_secs(90), enable)
        .await
        .expect("enable deadlocked")
        .unwrap()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(90), migrate)
        .await
        .expect("migrate deadlocked")
        .unwrap()
        .unwrap();

    let (ready, internal, public_wrapper, search_read): (bool, bool, bool, bool) =
        sqlx::query_as(
            "SELECT (SELECT capability_ready FROM durable_knowledge_capability WHERE singleton), \
             pg_catalog.has_function_privilege($1, \
             'public.tect_dk2_internal_native_read(uuid,uuid,uuid,bigint,uuid,boolean)', 'EXECUTE'), \
             pg_catalog.has_function_privilege($1, \
             'public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean)', 'EXECUTE'), \
             pg_catalog.has_table_privilege($1, 'public.knowledge_search_resources', 'SELECT')",
        )
        .bind(&role)
        .fetch_one(&observer)
        .await
        .unwrap();
    assert!(ready);
    assert!(!internal);
    assert!(public_wrapper);
    assert!(search_read);
}
