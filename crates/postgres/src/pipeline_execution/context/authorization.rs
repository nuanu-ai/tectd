use super::*;

type OwnedCopyKey = (String, Uuid, i64, Option<String>, Option<Uuid>);

// Keep the context's selected keys and their order: the scalar gate reports the
// first missing/headless copy before a later restricted copy.
async fn authorize_context_copy_keys(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    keys: &[OwnedCopyKey],
) -> Result<()> {
    tect_application::request_diagnostics::count(
        "context.authorization_requested_keys",
        keys.len(),
    );
    if keys.is_empty() {
        return Ok(());
    }

    let ready: bool = sqlx::query_scalar("SELECT public.tect_dk_database_identity_ready()")
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;
    if !ready {
        return Err(Error::KnowledgeUnavailable);
    }
    let owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
        .bind(principal)
        .fetch_one(&mut **tx)
        .await
        .map_err(storage_error)?;

    let relations: Vec<String> = keys.iter().map(|key| key.0.clone()).collect();
    let rows: Vec<Uuid> = keys.iter().map(|key| key.1).collect();
    let revisions: Vec<i64> = keys.iter().map(|key| key.2).collect();
    let operations: Vec<Option<String>> = keys.iter().map(|key| key.3.clone()).collect();
    let requests: Vec<Option<Uuid>> = keys.iter().map(|key| key.4).collect();
    // One grouped scan checks every exact key, retaining duplicate input ordinals.
    // The tenant/workspace predicates and NULL-safe operation/request comparisons
    // are the same as authorize_owned_copy's scalar query.
    let checks: Vec<(i64, i64, bool, bool)> = sqlx::query_as(
        "WITH requested AS (SELECT relation_name,row_id,row_revision,row_operation,row_request_id,ordinality FROM ROWS FROM (pg_catalog.unnest($3::text[]),pg_catalog.unnest($4::uuid[]),pg_catalog.unnest($5::bigint[]),pg_catalog.unnest($6::text[]),pg_catalog.unnest($7::uuid[])) WITH ORDINALITY AS k(relation_name,row_id,row_revision,row_operation,row_request_id,ordinality)) SELECT k.ordinality,COUNT(c.id),COALESCE(pg_catalog.bool_or(c.id IS NOT NULL AND h.unit_id IS NULL),false),COALESCE(pg_catalog.bool_or(h.access_scope='owners_only'),false) FROM requested k LEFT JOIN knowledge_owned_copies c ON c.tenant_id=$1 AND c.workspace_id=$2 AND c.relation_name=k.relation_name AND c.row_id=k.row_id AND c.row_revision=k.row_revision AND c.row_operation IS NOT DISTINCT FROM k.row_operation AND c.row_request_id IS NOT DISTINCT FROM k.row_request_id LEFT JOIN knowledge_unit_heads h ON h.tenant_id=c.tenant_id AND h.workspace_id=c.workspace_id AND h.unit_id=c.unit_id GROUP BY k.ordinality ORDER BY k.ordinality",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(relations)
    .bind(rows)
    .bind(revisions)
    .bind(operations)
    .bind(requests)
    .fetch_all(&mut **tx)
    .await
    .map_err(storage_error)?;
    tect_application::request_diagnostics::count(
        "context.authorization_returned_rows",
        checks.len(),
    );
    if checks.len() != keys.len() {
        return Err(Error::InternalInvariant);
    }
    for (index, (ordinal, copies, missing_head, restricted)) in checks.into_iter().enumerate() {
        if ordinal != (index + 1) as i64 || copies == 0 || missing_head {
            return Err(Error::InternalInvariant);
        }
        if restricted && !owner {
            return Err(Error::Forbidden);
        }
    }
    Ok(())
}

async fn authorize_copy_keys(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    keys: Vec<OwnedCopyKey>,
) -> Result<()> {
    for (relation, row, revision, operation, request) in keys {
        crate::durable_knowledge::manifest::authorize_owned_copy(
            tx,
            tenant,
            workspace,
            principal,
            &relation,
            row,
            revision,
            operation.as_deref(),
            request,
        )
        .await?;
    }
    Ok(())
}

pub(in crate::pipeline_execution) async fn authorize_run_origin(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let keys:Vec<OwnedCopyKey>=sqlx::query_as("SELECT relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='slice_pipeline_runs' AND row_id=$3 ORDER BY row_revision,row_operation,row_request_id")
        .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    if keys.is_empty() {
        return Err(Error::InternalInvariant);
    }
    authorize_copy_keys(tx, tenant, workspace, principal, keys).await
}

pub(in crate::pipeline_execution) async fn authorize_run_origin_if_present(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let keys:Vec<OwnedCopyKey>=sqlx::query_as("SELECT relation_name,row_id,row_revision,row_operation,row_request_id FROM knowledge_owned_copies WHERE tenant_id=$1 AND workspace_id=$2 AND relation_name='slice_pipeline_runs' AND row_id=$3 ORDER BY row_revision,row_operation,row_request_id")
        .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    authorize_copy_keys(tx, tenant, workspace, principal, keys).await
}

pub(super) async fn authorize_context_copies(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
) -> Result<()> {
    let keys:Vec<OwnedCopyKey>=sqlx::query_as("SELECT DISTINCT c.relation_name,c.row_id,c.row_revision,c.row_operation,c.row_request_id FROM knowledge_owned_copies c WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND ((c.relation_name='slice_pipeline_runs' AND c.row_id=$3) OR (c.relation_name='slice_pipeline_phase_attempts' AND EXISTS(SELECT 1 FROM slice_pipeline_phase_attempts a WHERE a.tenant_id=c.tenant_id AND a.workspace_id=c.workspace_id AND a.id=c.row_id AND a.run_id=$3)) OR (c.relation_name='slice_pipeline_phase_outputs' AND EXISTS(SELECT 1 FROM slice_pipeline_phase_outputs o WHERE o.tenant_id=c.tenant_id AND o.workspace_id=c.workspace_id AND o.id=c.row_id AND o.run_id=$3)) OR (c.relation_name='slice_pipeline_inputs' AND EXISTS(SELECT 1 FROM slice_pipeline_inputs i WHERE i.tenant_id=c.tenant_id AND i.workspace_id=c.workspace_id AND i.id=c.row_id AND i.run_id=$3)) OR (c.relation_name='slice_pipeline_receipts' AND c.row_id=$3) OR (c.relation_name='slice_results' AND EXISTS(SELECT 1 FROM slice_results r WHERE r.tenant_id=c.tenant_id AND r.workspace_id=c.workspace_id AND r.id=c.row_id AND r.pipeline_run_id=$3))) ORDER BY c.relation_name,c.row_id,c.row_revision,c.row_operation,c.row_request_id")
        .bind(tenant).bind(workspace).bind(run).fetch_all(&mut **tx).await.map_err(storage_error)?;
    authorize_context_copy_keys(tx, tenant, workspace, principal, &keys).await
}

#[cfg(test)]
mod tests {
    use super::*;

    async fn assert_scalar_batch_parity(
        tx: &mut Transaction<'_, Postgres>,
        tenant: Uuid,
        workspace: Uuid,
        principal: Uuid,
        keys: &[OwnedCopyKey],
        expected: Result<()>,
    ) {
        let mut scalar = Ok(());
        for (relation, row, revision, operation, request) in keys {
            scalar = crate::durable_knowledge::manifest::authorize_owned_copy(
                tx,
                tenant,
                workspace,
                principal,
                relation,
                *row,
                *revision,
                operation.as_deref(),
                *request,
            )
            .await;
            if scalar.is_err() {
                break;
            }
        }
        let batch = authorize_context_copy_keys(tx, tenant, workspace, principal, keys).await;
        assert_eq!(scalar, expected, "scalar fixture expectation");
        assert_eq!(batch, scalar, "batch must match first scalar failure");
    }

    #[tokio::test]
    async fn context_copy_batch_matches_scalar_exact_keys_and_failures() {
        if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
            return;
        }
        fn fixture_uuid(name: &str) -> Uuid {
            std::env::var(name)
                .unwrap_or_else(|_| panic!("{name} is required for the dedicated DK2 fixture"))
                .parse()
                .unwrap_or_else(|_| panic!("{name} must be a UUID"))
        }
        let admin_url =
            std::env::var("TECT_TEST_ADMIN_URL").expect("dedicated DK2 admin URL required");
        let tenant = fixture_uuid("TECT_TEST_BATCH_AUTH_TENANT_ID");
        let workspace = fixture_uuid("TECT_TEST_BATCH_AUTH_WORKSPACE_ID");
        let unit = fixture_uuid("TECT_TEST_BATCH_AUTH_UNIT_ID");
        let owner = fixture_uuid("TECT_TEST_BATCH_AUTH_OWNER_ID");
        let non_owner = fixture_uuid("TECT_TEST_BATCH_AUTH_NON_OWNER_ID");
        let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
        let mut tx = pool.begin().await.unwrap();
        sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
            .bind(tenant.to_string())
            .execute(&mut *tx)
            .await
            .unwrap();
        let ready: bool = sqlx::query_scalar("SELECT public.tect_dk_database_identity_ready()")
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(ready, "the DK2 database identity fixture must be ready");
        let head: Option<String> = sqlx::query_scalar(
            "SELECT access_scope FROM knowledge_unit_heads WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(unit)
        .fetch_optional(&mut *tx)
        .await
        .unwrap();
        assert!(head.is_some(), "a real seeded head is required");
        let owner_exists: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM principals WHERE tenant_id=$1 AND id=$2)",
        )
        .bind(tenant)
        .bind(owner)
        .fetch_one(&mut *tx)
        .await
        .unwrap();
        assert!(owner_exists, "a real seeded owner principal is required");
        let non_owner_exists: bool =
            sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM principals WHERE id=$1)")
                .bind(non_owner)
                .fetch_one(&mut *tx)
                .await
                .unwrap();
        assert!(
            non_owner_exists,
            "a real seeded second principal is required"
        );
        let actual_owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
            .bind(owner)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        let actual_non_owner: bool = sqlx::query_scalar("SELECT tect_dk_is_owner($1)")
            .bind(non_owner)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
        assert!(actual_owner);
        assert!(!actual_non_owner);

        let nullable: OwnedCopyKey = ("slice_pipeline_runs".into(), Uuid::new_v4(), 7, None, None);
        let exact: OwnedCopyKey = (
            "slice_pipeline_runs".into(),
            Uuid::new_v4(),
            11,
            Some("phase_complete".into()),
            Some(Uuid::new_v4()),
        );
        for (relation, row, revision, operation, request) in [&nullable, &exact] {
            sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision,row_operation,row_request_id) VALUES($1,$2,$3,$4,'phase_execution',$5,$6,$7,$8,$9)")
                .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(unit)
                .bind(relation).bind(row).bind(revision).bind(operation).bind(request)
                .execute(&mut *tx).await.unwrap();
        }
        sqlx::query("UPDATE knowledge_unit_heads SET access_scope='workspace_members' WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut *tx).await.unwrap();
        assert_scalar_batch_parity(&mut tx, tenant, workspace, non_owner, &[], Ok(())).await;
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            non_owner,
            &[nullable.clone(), exact.clone(), nullable.clone()],
            Ok(()),
        )
        .await;
        let absent: OwnedCopyKey = ("slice_pipeline_runs".into(), Uuid::new_v4(), 7, None, None);
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            non_owner,
            &[exact.clone(), absent.clone()],
            Err(Error::InternalInvariant),
        )
        .await;
        let wrong_null: OwnedCopyKey = (exact.0.clone(), exact.1, exact.2, None, exact.4);
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            non_owner,
            &[wrong_null],
            Err(Error::InternalInvariant),
        )
        .await;
        assert_scalar_batch_parity(
            &mut tx,
            Uuid::new_v4(),
            workspace,
            owner,
            std::slice::from_ref(&nullable),
            Err(Error::InternalInvariant),
        )
        .await;
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            Uuid::new_v4(),
            owner,
            std::slice::from_ref(&nullable),
            Err(Error::InternalInvariant),
        )
        .await;

        sqlx::query("UPDATE knowledge_unit_heads SET access_scope='owners_only' WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=$3")
            .bind(tenant).bind(workspace).bind(unit).execute(&mut *tx).await.unwrap();
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            owner,
            &[nullable.clone(), exact.clone()],
            Ok(()),
        )
        .await;
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            non_owner,
            std::slice::from_ref(&exact),
            Err(Error::Forbidden),
        )
        .await;
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            non_owner,
            &[absent.clone(), exact.clone()],
            Err(Error::InternalInvariant),
        )
        .await;
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            non_owner,
            &[exact.clone(), absent],
            Err(Error::Forbidden),
        )
        .await;

        let orphan: OwnedCopyKey = ("slice_pipeline_runs".into(), Uuid::new_v4(), 13, None, None);
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision) VALUES($1,$2,$3,$4,'phase_execution',$5,$6,$7)")
            .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(Uuid::new_v4())
            .bind(&orphan.0).bind(orphan.1).bind(orphan.2)
            .execute(&mut *tx).await.unwrap();
        assert_scalar_batch_parity(
            &mut tx,
            tenant,
            workspace,
            owner,
            &[orphan],
            Err(Error::InternalInvariant),
        )
        .await;
        tx.rollback().await.unwrap();
    }
}
