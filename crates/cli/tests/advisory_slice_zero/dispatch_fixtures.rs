use super::*;

pub(super) async fn record_scope_dispatches(
    pool: &PgPool,
    enrollment: &admin::Enrollment,
    workspace_id: Uuid,
    scope_id: Uuid,
    session_id: Uuid,
    principal_id: Uuid,
) -> Uuid {
    let opportunity_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) \
         VALUES($1,$2,$3,$4,'scope',$4,$5,$6,'1','scope_decomposition','scope.decomposition.before_selection',2,'use_workspace','use_workspace','slice-00.v1',$7,$8,'prepared','dispatch_authorized')",
    )
    .bind(opportunity_id)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(scope_id)
    .bind(session_id)
    .bind(principal_id)
    .bind(format!("scope-audit-{opportunity_id}"))
    .bind("1".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    let dispatch_id = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) \
         VALUES($1,$2,$3,$4,1,'jev-route','jev-advisory-v1','{}',$5,$6,$7,'request','sending','sent_unknown','initial',clock_timestamp())",
    )
    .bind(dispatch_id)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(opportunity_id)
    .bind("2".repeat(64))
    .bind("1".repeat(64))
    .bind("3".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    assert!(
        sqlx::query(
            "UPDATE advisory_dispatch SET state='cancelled',sealed_at=clock_timestamp() WHERE id=$1"
        )
        .bind(dispatch_id)
        .execute(pool)
        .await
        .is_err()
    );

    let retry_opportunity = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,scope_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) \
         VALUES($1,$2,$3,$4,'scope',$4,$5,$6,'1','scope_decomposition','scope.decomposition.before_selection',2,'use_workspace','use_workspace','slice-00.v1',$7,$8,'prepared','dispatch_authorized')",
    )
    .bind(retry_opportunity)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(scope_id)
    .bind(session_id)
    .bind(principal_id)
    .bind(format!("retry-audit-{retry_opportunity}"))
    .bind("4".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    let predecessor = Uuid::new_v4();
    sqlx::query(
        "INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,sealed_at) \
         VALUES($1,$2,$3,$4,1,'jev-route','jev-advisory-v1','{}',$5,$6,$7,'request','cancelled','not_sent','initial',clock_timestamp())",
    )
    .bind(predecessor)
    .bind(enrollment.tenant_id)
    .bind(workspace_id)
    .bind(retry_opportunity)
    .bind("5".repeat(64))
    .bind("4".repeat(64))
    .bind("6".repeat(64))
    .execute(pool)
    .await
    .unwrap();
    let retry_insert = |attempt: i32, id: Uuid| {
        sqlx::query(
            "INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,predecessor_dispatch_id,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis) \
             VALUES($1,$2,$3,$4,$5,$6,'jev-route','jev-advisory-v1','{}',$7,$8,$9,'retry','authorized','not_sent','proven_not_sent')",
        )
        .bind(id)
        .bind(enrollment.tenant_id)
        .bind(workspace_id)
        .bind(retry_opportunity)
        .bind(attempt)
        .bind(predecessor)
        .bind("5".repeat(64))
        .bind("4".repeat(64))
        .bind("7".repeat(64))
    };
    retry_insert(2, Uuid::new_v4()).execute(pool).await.unwrap();
    assert!(retry_insert(3, Uuid::new_v4()).execute(pool).await.is_err());

    opportunity_id
}
