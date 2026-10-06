use super::super::bootstrap_support as support;
use super::*;

#[tokio::test]
#[ignore = "requires fresh disposable PG18.6, private trust socket and separate admin/runtime identities"]
async fn planning_postgrant_failure_rolls_back_acl_and_private_guards_hold() {
    let pool = support::fixture().await;
    let role = support::role(&pool).await;
    let quoted = quote_identifier(&role).unwrap();
    super::validate_runtime_role_pregrant(&pool, &role)
        .await
        .unwrap();
    let mut connection = pool.acquire().await.unwrap();
    let before = support::acl(&mut connection).await;
    let privileges = support::privileges(&mut connection, &role).await;
    assert_eq!(privileges, vec![false; 5]);
    let baseline_trigger: String = sqlx::query_scalar(
        "SELECT tgenabled::text FROM pg_trigger WHERE tgrelid='public.matrix_planning_selection_links'::regclass AND tgname='matrix_planning_selection_context_guard'"
    ).fetch_one(&mut *connection).await.unwrap();
    assert_eq!(baseline_trigger, "O");
    drop(connection);
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("ALTER TABLE matrix_planning_selection_links DISABLE TRIGGER matrix_planning_selection_context_guard")
        .execute(&mut *tx).await.unwrap();
    let result = grant_runtime(&mut tx, &quoted, &role).await;
    let granted = support::privileges(&mut tx, &role).await;
    let disabled: String = sqlx::query_scalar(
        "SELECT tgenabled::text FROM pg_trigger WHERE tgrelid='public.matrix_planning_selection_links'::regclass AND tgname='matrix_planning_selection_context_guard'"
    ).fetch_one(&mut *tx).await.unwrap();
    tx.rollback().await.unwrap();
    assert!(matches!(result, Err(Error::InvalidConfiguration)));
    assert_eq!(granted, vec![true; 5]);
    assert_eq!(disabled, "D");
    let mut connection = pool.acquire().await.unwrap();
    assert_eq!(support::acl(&mut connection).await, before);
    assert_eq!(
        support::privileges(&mut connection, &role).await,
        privileges
    );
    drop(connection);
    println!("bootstrap direct_grant_tx full_postcheck_failed/explicit_rollback/ACL_exact=true");
    super::super::migrate(&pool, &role).await.unwrap();
    for (name, mutation) in [
        (
            "PUBLIC_ACL",
            "GRANT SELECT ON matrix_planning_selection_links TO PUBLIC",
        ),
        (
            "disabled_trigger",
            "ALTER TABLE matrix_planning_selection_links DISABLE TRIGGER matrix_planning_selection_context_guard",
        ),
    ] {
        super::super::validate_runtime_role(&pool, &role)
            .await
            .unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query(mutation).execute(&mut *tx).await.unwrap();
        let fact_query = if name == "PUBLIC_ACL" {
            "SELECT EXISTS(SELECT 1 FROM pg_class c,
             LATERAL aclexplode(COALESCE(c.relacl,acldefault('r',c.relowner))) acl
             WHERE c.oid='public.matrix_planning_selection_links'::regclass
             AND acl.grantee=0 AND acl.privilege_type='SELECT')"
        } else {
            "SELECT tgenabled='D' FROM pg_trigger WHERE tgrelid='public.matrix_planning_selection_links'::regclass
             AND tgname='matrix_planning_selection_context_guard'"
        };
        let fact = sqlx::query_scalar::<_, bool>(fact_query)
            .fetch_one(&mut *tx)
            .await;
        let pregrant = validate_runtime_role_connection(&mut tx, &role, false).await;
        let strict = validate_runtime_role_connection(&mut tx, &role, true).await;
        tx.rollback().await.unwrap();
        assert!(fact.unwrap(), "{name} effective triggering fact");
        assert!(
            matches!(pregrant, Err(Error::InvalidConfiguration)),
            "{name}"
        );
        assert!(matches!(strict, Err(Error::InvalidConfiguration)), "{name}");
        println!(
            "bootstrap private_connection reject_{name}=true effective_fact=true rollback_before_assert=true"
        );
    }
    super::super::validate_runtime_role(&pool, &role)
        .await
        .unwrap();
}
