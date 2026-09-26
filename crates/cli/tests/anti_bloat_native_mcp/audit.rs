use super::*;

pub(super) async fn immutable(
    pool: &PgPool,
    runtime: &str,
    auth: &tect_domain::HostAuth,
    tenant: Uuid,
    review: Uuid,
) {
    let runtime_pool = PgPool::connect(runtime).await.unwrap();
    let role: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime_pool)
        .await
        .unwrap();
    assert_eq!(role, std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap());
    for assignment in [
        "response_http_status=201",
        "response_original_elapsed_ms=response_original_elapsed_ms+1",
        "request_adapter_identity='different/1'",
        "response_complete=NOT response_complete",
        "original_transport_context=original_transport_context || '{\"provider_failure_code\":\"changed\"}'::jsonb",
    ] {
        identity(pool).await;
        let mut tx = runtime_pool.begin().await.unwrap();
        let digest = format!("{:x}", Sha256::digest(auth.credential.as_bytes()));
        let authenticated: Uuid =
            sqlx::query_scalar("SELECT tenant_id FROM public.tect_authenticate_host($1,$2,true)")
                .bind(auth.host_id)
                .bind(digest)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert_eq!(authenticated, tenant);
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let visible:i64=sqlx::query_scalar("SELECT count(*) FROM public.scope_anti_bloat_reviews WHERE review_id=$1 AND raw_response IS NOT NULL").bind(review).fetch_one(&mut *tx).await.unwrap();
        assert_eq!(visible, 1, "guard target visible under runtime RLS");
        let error = sqlx::query(&format!(
            "UPDATE public.scope_anti_bloat_reviews SET {assignment} WHERE review_id=$1"
        ))
        .bind(review)
        .execute(&mut *tx)
        .await
        .unwrap_err();
        let database = error.as_database_error().unwrap();
        assert_eq!(
            database.code().as_deref(),
            Some(if assignment.starts_with("request_adapter_identity") {
                "P0001"
            } else {
                "23514"
            }),
            "must be immutable guard, not permission failure"
        );
        assert!(
            database.message().contains("immutable"),
            "must identify immutable guard"
        );
        tx.rollback().await.unwrap();
    }
    runtime_pool.close().await;
}
