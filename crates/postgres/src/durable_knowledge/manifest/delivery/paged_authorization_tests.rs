use super::*;

#[tokio::test]
async fn all_pinned_rows_recheck_current_acl_and_erasure() {
    let Ok(url) = std::env::var("TECT_TEST_ADMIN_URL") else {
        return;
    };
    let pool = sqlx::PgPool::connect(&url).await.unwrap();
    let mut tx = pool.begin().await.unwrap();
    // Session-local shadows exercise the exact authorization SQL without
    // creating a live knowledge fixture or modifying the main database.
    for ddl in [
        "CREATE TEMP TABLE pipeline_knowledge_manifest_resources(tenant_id uuid,workspace_id uuid,manifest_id uuid,unit_id uuid,revision bigint,entry_kind text,publication_event_id uuid,validation_event_id uuid,binding_id uuid)",
        "CREATE TEMP TABLE knowledge_unit_heads(tenant_id uuid,workspace_id uuid,unit_id uuid,payload_erased bool,access_scope text)",
        "CREATE TEMP TABLE knowledge_revisions(tenant_id uuid,workspace_id uuid,unit_id uuid,revision bigint,payload_erased bool,access_scope text)",
        "CREATE TEMP TABLE knowledge_publication_events(tenant_id uuid,workspace_id uuid,id uuid,unit_id uuid,unit_revision bigint,payload_erased bool)",
        "CREATE TEMP TABLE knowledge_validation_events(tenant_id uuid,workspace_id uuid,id uuid,unit_id uuid,unit_revision bigint,payload_erased bool)",
        "CREATE TEMP TABLE knowledge_bindings(tenant_id uuid,workspace_id uuid,id uuid,unit_id uuid)",
    ] {
        sqlx::query(ddl).execute(&mut *tx).await.unwrap();
    }
    let tenant = Uuid::new_v4();
    let workspace = Uuid::new_v4();
    let manifest = Uuid::new_v4();
    let mut units = Vec::new();
    for ordinal in 0..2 {
        let unit = Uuid::new_v4();
        let event = Uuid::new_v4();
        let binding = Uuid::new_v4();
        units.push(unit);
        sqlx::query(
            "INSERT INTO pipeline_knowledge_manifest_resources VALUES($1,$2,$3,$4,1,$5,$6,NULL,$7)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(manifest)
        .bind(unit)
        .bind(if ordinal == 0 {
            "dk2_event"
        } else {
            "dk1_legacy"
        })
        .bind(if ordinal == 0 { Some(event) } else { None })
        .bind(binding)
        .execute(&mut *tx)
        .await
        .unwrap();
        sqlx::query("INSERT INTO knowledge_unit_heads VALUES($1,$2,$3,false,'workspace_members')")
            .bind(tenant)
            .bind(workspace)
            .bind(unit)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO knowledge_revisions VALUES($1,$2,$3,1,false,'workspace_members')")
            .bind(tenant)
            .bind(workspace)
            .bind(unit)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO knowledge_publication_events VALUES($1,$2,$3,$4,1,false)")
            .bind(tenant)
            .bind(workspace)
            .bind(event)
            .bind(unit)
            .execute(&mut *tx)
            .await
            .unwrap();
        sqlx::query("INSERT INTO knowledge_bindings VALUES($1,$2,$3,$4)")
            .bind(tenant)
            .bind(workspace)
            .bind(binding)
            .bind(unit)
            .execute(&mut *tx)
            .await
            .unwrap();
    }
    assert_eq!(
        authorize_paged_rows(&mut tx, tenant, workspace, manifest, false).await,
        Ok(())
    );
    sqlx::query("UPDATE knowledge_unit_heads SET access_scope='owners_only' WHERE unit_id=$1")
        .bind(units[1])
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        authorize_paged_rows(&mut tx, tenant, workspace, manifest, false).await,
        Err(Error::Forbidden)
    );
    assert_eq!(
        authorize_paged_rows(&mut tx, tenant, workspace, manifest, true).await,
        Ok(())
    );
    sqlx::query("UPDATE knowledge_unit_heads SET payload_erased=true WHERE unit_id=$1")
        .bind(units[1])
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        authorize_paged_rows(&mut tx, tenant, workspace, manifest, true).await,
        Err(Error::KnowledgePayloadErased)
    );
    assert_eq!(
        authorize_paged_rows(&mut tx, Uuid::new_v4(), workspace, manifest, false).await,
        Ok(())
    );
    tx.rollback().await.unwrap();
}
