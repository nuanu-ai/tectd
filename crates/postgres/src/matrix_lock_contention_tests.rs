use tect_domain::Error;

#[test]
fn matrix_task_lock_paths_fail_fast_with_the_same_error_mapping() {
    let task_store = include_str!("matrix_task_store.rs");
    assert_eq!(task_store.matches("FOR UPDATE NOWAIT").count(), 2);
    assert_eq!(
        task_store
            .matches("map_err(crate::matrix_lock_error)")
            .count(),
        2
    );
    for source in [
        include_str!("context_matrix_verification_store.rs"),
        include_str!("matrix_verification_store.rs"),
        include_str!("advisory/dispatch/authorization/matrix.rs"),
    ] {
        assert!(source.contains("FOR UPDATE OF t NOWAIT") || source.contains("FOR UPDATE NOWAIT"));
        assert_eq!(
            source.matches("map_err(crate::matrix_lock_error)").count(),
            1
        );
    }
}

#[test]
fn failed_advisory_try_lock_invalidates_the_uow_before_context_reads() {
    let source = include_str!("matrix_requirements_context_store.rs");
    assert!(source.contains("pg_try_advisory_xact_lock(pg_catalog.hashtextextended($1,0))"));
    assert!(!source.contains("pg_catalog.pg_advisory_xact_lock("));
    let contention = source.find("if !locked {").unwrap();
    let abort = source[contention..]
        .find("self.abort_matrix_lock_contention().await?;")
        .unwrap();
    let stale = source[contention..]
        .find("return Err(Error::StaleRevision);")
        .unwrap();
    let read = source[contention..]
        .find("SELECT COALESCE(max(context_revision),0)")
        .unwrap();
    assert!(abort < stale && stale < read);
    let uow = include_str!("store.rs");
    let abort = uow
        .split("async fn abort_matrix_lock_contention")
        .nth(1)
        .unwrap();
    assert!(abort.find(".take()").unwrap() < abort.find(".rollback()").unwrap());
}

#[test]
fn matrix_lock_mapping_does_not_reclassify_non_database_failures() {
    assert!(matches!(
        crate::matrix_lock_error(sqlx::Error::RowNotFound),
        Error::StorageUnavailable
    ));
    assert!(matches!(
        crate::matrix_lock_error(sqlx::Error::PoolClosed),
        Error::StorageUnavailable
    ));
}
