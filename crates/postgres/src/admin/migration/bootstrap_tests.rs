use super::bootstrap_support as support;
use super::*;

#[tokio::test]
#[ignore = "requires fresh disposable PG18.6, private trust socket and separate admin/runtime identities"]
async fn schema_without_grants_bootstraps_then_repeats() {
    let pool = support::fixture().await;
    let role = support::role(&pool).await;
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(
        support::privileges(&mut connection, &role).await,
        vec![false; 5]
    );
    drop(connection);
    validate_runtime_role_pregrant(&pool, &role).await.unwrap();
    assert!(matches!(
        validate_runtime_role(&pool, &role).await,
        Err(Error::InvalidConfiguration)
    ));
    migrate(&pool, &role).await.unwrap();
    validate_runtime_role(&pool, &role).await.unwrap();
    let store = support::connected(&role).await;
    store.pool().close().await;
    let mut connection = pool.acquire().await.unwrap();
    let first = support::acl(&mut connection).await;
    assert_eq!(
        support::privileges(&mut connection, &role).await,
        vec![true; 5]
    );
    drop(connection);
    migrate(&pool, &role).await.unwrap();
    validate_runtime_role(&pool, &role).await.unwrap();
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(support::acl(&mut connection).await, first);
    let expected_ledger = support::expected_ledger_count();
    assert_eq!(support::ledger(&pool).await, expected_ledger);
    println!("bootstrap first_migrate/strict/connect/repeat=true ledger={expected_ledger}");
}

#[tokio::test]
#[ignore = "requires fresh disposable PG18.6, private trust socket and separate admin/runtime identities"]
async fn public_strict_requires_each_planning_grant() {
    let pool = support::fixture().await;
    for (name, revoke, expected) in [
        (
            "SELECT",
            "REVOKE SELECT ON matrix_planning_selection_links,matrix_planning_effect_attestations",
            vec![false, true, false, true, true],
        ),
        (
            "INSERT",
            "REVOKE INSERT ON matrix_planning_selection_links,matrix_planning_effect_attestations",
            vec![true, false, true, false, true],
        ),
        (
            "EXECUTE",
            "REVOKE EXECUTE ON FUNCTION matrix_planning_lock_verification(uuid,uuid,uuid)",
            vec![true, true, true, true, false],
        ),
    ] {
        let role = support::role(&pool).await;
        migrate(&pool, &role).await.unwrap();
        validate_runtime_role(&pool, &role).await.unwrap();
        session_preference::assert_transaction_local_acl_cases(&pool, &role).await;
        sqlx::query(&format!(
            "{revoke} FROM {}",
            quote_identifier(&role).unwrap()
        ))
        .execute(&pool)
        .await
        .unwrap();
        let mut connection = pool.acquire().await.unwrap();
        assert_eq!(support::privileges(&mut connection, &role).await, expected);
        drop(connection);
        assert!(matches!(
            validate_runtime_role(&pool, &role).await,
            Err(Error::InvalidConfiguration)
        ));
        println!(
            "bootstrap missing_{name}=strict_rejected other_required_privileges_unchanged=true"
        );
    }
}

#[tokio::test]
#[ignore = "requires fresh disposable PG18.6, private trust socket and separate admin/runtime identities"]
async fn pregrant_security_checks_remain_required() {
    let pool = support::fixture().await;
    for name in [
        "bypass",
        "owner_member",
        "forbidden_execute",
        "column_update",
    ] {
        let role = support::role(&pool).await;
        migrate(&pool, &role).await.unwrap();
        validate_runtime_role(&pool, &role).await.unwrap();
        let quoted = quote_identifier(&role).unwrap();
        let owner: String = sqlx::query_scalar(
            "SELECT pg_get_userbyid(relowner) FROM pg_class WHERE oid='public.matrix_planning_selection_links'::regclass"
        ).fetch_one(&pool).await.unwrap();
        let owner_name = owner;
        let owner = quote_identifier(&owner_name).unwrap();
        let mutation = match name {
            "bypass" => format!("ALTER ROLE {quoted} BYPASSRLS"),
            "owner_member" => format!("GRANT {owner} TO {quoted}"),
            "forbidden_execute" => format!(
                "GRANT EXECUTE ON FUNCTION matrix_planning_selection_require_active_owner() TO {quoted}"
            ),
            _ => format!(
                "GRANT UPDATE(caller_request_id) ON matrix_planning_selection_links TO {quoted}"
            ),
        };
        sqlx::query(&mutation).execute(&pool).await.unwrap();
        let fact_query = match name {
            "bypass" => "SELECT rolbypassrls FROM pg_roles WHERE rolname=$1",
            "owner_member" => "SELECT pg_has_role($1,$2,'MEMBER')",
            "forbidden_execute" => {
                "SELECT has_function_privilege($1,'public.matrix_planning_selection_require_active_owner()','EXECUTE')"
            }
            _ => {
                "SELECT has_column_privilege($1,'public.matrix_planning_selection_links','caller_request_id','UPDATE')"
            }
        };
        let fact = if name == "owner_member" {
            sqlx::query_scalar::<_, bool>(fact_query)
                .bind(&role)
                .bind(&owner_name)
                .fetch_one(&pool)
                .await
        } else {
            sqlx::query_scalar::<_, bool>(fact_query)
                .bind(&role)
                .fetch_one(&pool)
                .await
        };
        let pregrant = validate_runtime_role_pregrant(&pool, &role).await;
        let strict = validate_runtime_role(&pool, &role).await;
        if name == "bypass" {
            sqlx::query(&format!("ALTER ROLE {quoted} NOBYPASSRLS"))
                .execute(&pool)
                .await
                .unwrap();
        } else if name == "owner_member" {
            sqlx::query(&format!("REVOKE {owner} FROM {quoted}"))
                .execute(&pool)
                .await
                .unwrap();
        }
        assert!(fact.unwrap(), "{name} effective triggering fact");
        assert!(
            matches!(pregrant, Err(Error::InvalidConfiguration)),
            "{name}"
        );
        assert!(matches!(strict, Err(Error::InvalidConfiguration)), "{name}");
        println!("bootstrap pregrant/public_strict reject_{name}=true effective_fact=true");
    }
}
