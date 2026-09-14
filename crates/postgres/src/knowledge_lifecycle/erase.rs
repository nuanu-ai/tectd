use super::*;

mod maintenance;
mod propagate;
mod recovery;
mod redact;
mod registry;
mod relation;
mod residual;
mod search;

pub(crate) use recovery::reconcile_change_owned_copies;
pub(crate) use registry::{
    register_knowledge_change_input_copies, register_knowledge_change_output_copies,
    register_pipeline_input_copies, register_pipeline_manifest_copies,
    register_pipeline_phase_copies, register_pipeline_receipt_copies,
    register_pipeline_run_origin_copies,
};
pub(crate) use search::{reconcile_absent_search_copies, register_search_copies};

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnowledgeOwnedPurgeReport {
    pub native_triples_deleted: i64,
    pub native_dictionary_terms_deleted: i64,
    pub relational_rows_redacted: i64,
    pub remaining: i64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnowledgeOwnedResidualReport {
    pub native_owned_triples: i64,
    pub relational_readable_rows: i64,
    pub complete: bool,
}

fn native_count(value: &serde_json::Value, key: &str) -> Result<i64> {
    value
        .get(key)
        .and_then(serde_json::Value::as_i64)
        .filter(|v| *v >= 0)
        .ok_or(Error::InternalInvariant)
}

pub(crate) async fn suppress_owned_unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<KnowledgeOwnedPurgeReport> {
    propagate::reconcile_unit(tx, tenant, workspace, unit).await?;
    let before = residual_owned_unit(tx, tenant, workspace, unit).await?;
    if before.complete {
        return Ok(KnowledgeOwnedPurgeReport {
            native_triples_deleted: 0,
            native_dictionary_terms_deleted: 0,
            relational_rows_redacted: 0,
            remaining: 0,
        });
    }
    let (native_triples_deleted, native_dictionary_terms_deleted) =
        if before.native_owned_triples == 0 {
            (0, 0)
        } else {
            let native: serde_json::Value =
                sqlx::query_scalar("SELECT public.tect_dk_native_erase($1,$2,$3)")
                    .bind(tenant)
                    .bind(workspace)
                    .bind(unit)
                    .fetch_one(&mut **tx)
                    .await
                    .map_err(storage_error)?;
            (
                native_count(&native, "triples_deleted")?,
                native_count(&native, "dictionary_terms_deleted")?,
            )
        };
    crate::knowledge_search::invalidate_unit(tx, tenant, workspace, unit).await?;
    let mut relational_rows_redacted = redact::canonical(tx, tenant, workspace, unit).await?;
    relational_rows_redacted += redact::registered(tx, tenant, workspace, unit).await?;
    let residual = residual_owned_unit(tx, tenant, workspace, unit).await?;
    Ok(KnowledgeOwnedPurgeReport {
        native_triples_deleted,
        native_dictionary_terms_deleted,
        relational_rows_redacted,
        remaining: residual.native_owned_triples + residual.relational_readable_rows,
    })
}

pub(crate) async fn residual_owned_unit(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    unit: Uuid,
) -> Result<KnowledgeOwnedResidualReport> {
    let native: serde_json::Value =
        sqlx::query_scalar("SELECT public.tect_dk_native_owned_residual($1,$2,$3)")
            .bind(tenant)
            .bind(workspace)
            .bind(unit)
            .fetch_one(&mut **tx)
            .await
            .map_err(storage_error)?;
    let native_owned_triples = native_count(&native, "owned_triples")?;
    let relational_readable_rows = residual::relational(tx, tenant, workspace, unit).await?;
    Ok(KnowledgeOwnedResidualReport {
        native_owned_triples,
        relational_readable_rows,
        complete: native_owned_triples == 0 && relational_readable_rows == 0,
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    #[test]
    fn native_counts_are_required_and_nonnegative() {
        assert_eq!(
            native_count(&serde_json::json!({"triples_deleted":2}), "triples_deleted"),
            Ok(2)
        );
        assert_eq!(
            native_count(&serde_json::json!({}), "triples_deleted"),
            Err(Error::InternalInvariant)
        );
        assert_eq!(
            native_count(
                &serde_json::json!({"triples_deleted":-1}),
                "triples_deleted"
            ),
            Err(Error::InternalInvariant)
        );
    }

    #[tokio::test]
    async fn exact_receipt_key_and_residual_body_are_enforced() {
        if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
            return;
        }
        let pool = sqlx::PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
            .await
            .unwrap();
        let (tenant, workspace, session): (Uuid, Uuid, Uuid) = sqlx::query_as(
            "SELECT tenant_id,workspace_id,id FROM agent_sessions ORDER BY created_at DESC LIMIT 1",
        )
        .fetch_one(&pool)
        .await
        .unwrap();
        let unit = Uuid::new_v4();
        let owner = Uuid::new_v4();
        let request = Uuid::new_v4();
        let mut tx = pool.begin().await.unwrap();
        for operation in ["prepare", "review"] {
            sqlx::query("INSERT INTO knowledge_command_receipts(tenant_id,workspace_id,operation,request_id,actor_session_id,request_payload,result_payload) VALUES($1,$2,$3,$4,$5,'{}'::jsonb,'{}'::jsonb)")
                .bind(tenant).bind(workspace).bind(operation).bind(request).bind(session).execute(&mut *tx).await.unwrap();
        }
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision,row_operation,row_request_id) VALUES($1,$2,$3,$4,'legacy_receipt','knowledge_command_receipts',$5,0,'prepare',$6)")
            .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(unit).bind(owner).bind(request).execute(&mut *tx).await.unwrap();
        redact::registered(&mut tx, tenant, workspace, unit)
            .await
            .unwrap();
        let rows:Vec<(String,bool,Option<serde_json::Value>)>=sqlx::query_as("SELECT operation,payload_erased,request_payload FROM knowledge_command_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3 ORDER BY operation")
            .bind(tenant).bind(workspace).bind(request).fetch_all(&mut *tx).await.unwrap();
        assert_eq!(rows[0], ("prepare".into(), true, None));
        assert_eq!(
            rows[1],
            ("review".into(), false, Some(serde_json::json!({})))
        );

        let entity = Uuid::new_v4();
        let independent = Uuid::new_v4();
        for candidate in [entity, independent] {
            sqlx::query("INSERT INTO native_planning_receipts(tenant_id,workspace_id,entity_id,operation,request_id,request_payload,result_payload) VALUES($1,$2,$3,'save_slice_draft',$4,'{}'::jsonb,'{}'::jsonb)")
                .bind(tenant).bind(workspace).bind(candidate).bind(request).execute(&mut *tx).await.unwrap();
        }
        sqlx::query("INSERT INTO knowledge_owned_copies(id,tenant_id,workspace_id,unit_id,copy_kind,relation_name,row_id,row_revision,row_operation,row_request_id) VALUES($1,$2,$3,$4,'planning_receipt','native_planning_receipts',$5,0,'save_slice_draft',$6)")
            .bind(Uuid::new_v4()).bind(tenant).bind(workspace).bind(unit).bind(entity).bind(request).execute(&mut *tx).await.unwrap();
        redact::registered(&mut tx, tenant, workspace, unit)
            .await
            .unwrap();
        let planning:Vec<(Uuid,bool)>=sqlx::query_as("SELECT entity_id,payload_erased FROM native_planning_receipts WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3 ORDER BY entity_id")
            .bind(tenant).bind(workspace).bind(request).fetch_all(&mut *tx).await.unwrap();
        assert!(planning.iter().find(|row| row.0 == entity).unwrap().1);
        assert!(!planning.iter().find(|row| row.0 == independent).unwrap().1);

        sqlx::query("ALTER TABLE knowledge_command_receipts DROP CONSTRAINT knowledge_command_receipts_erased_shape")
            .execute(&mut *tx).await.unwrap();
        sqlx::query("UPDATE knowledge_command_receipts SET request_payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND operation='prepare' AND request_id=$3")
            .bind(tenant).bind(workspace).bind(request).execute(&mut *tx).await.unwrap();
        assert_eq!(
            residual::relational(&mut tx, tenant, workspace, unit)
                .await
                .unwrap(),
            1
        );
        tx.rollback().await.unwrap();
    }
}
