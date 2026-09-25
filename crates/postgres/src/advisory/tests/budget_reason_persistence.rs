#[cfg(test)]
mod budget_reason_persistence_tests {
    use super::*;
    use crate::{PgStore, admin};
    use tect_application::{Store, TransactionMode};

    #[tokio::test]
    #[ignore = "requires disposable PostgreSQL 18.6 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
    async fn budget_no_call_round_trips_through_capture_and_candidate_audit() {
        let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
        let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
        let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
        let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
        admin::migrate(&pool, &role).await.unwrap();
        let version: String = sqlx::query_scalar("SHOW server_version_num")
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(version.parse::<i32>().unwrap(), 180_006);

        let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
        let tenant = enrollment.tenant_id;
        let actor = enrollment.principal_id;
        let workspace = Uuid::new_v4();
        let session = Uuid::new_v4();
        let program = Uuid::new_v4();
        let candidate = Uuid::new_v4();
        let request_key = Uuid::new_v4().to_string();
        sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
            .bind(workspace)
            .bind(tenant)
            .bind(format!("budget-audit-{workspace}"))
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query(
            "INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(session)
            .bind(tenant)
            .bind(enrollment.auth.host_id)
            .bind(workspace)
            .bind(session.to_string())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
            .bind(program)
            .bind(tenant)
            .bind(workspace)
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
            .bind(candidate)
            .bind(tenant)
            .bind(workspace)
            .bind(program)
            .bind(Uuid::new_v4())
            .execute(&pool)
            .await
            .unwrap();
        sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4)")
            .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
            .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
        sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,1,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
            .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();

        let store = PgStore::connect(&runtime_url, 4).await.unwrap();
        let mut write = store.begin(TransactionMode::ReadWrite).await.unwrap();
        write.authenticate(&enrollment.auth).await.unwrap();
        write.set_tenant(tenant).await.unwrap();
        let captured = write
            .capture_advisory_opportunity(
                workspace,
                &AdvisoryOpportunityInput {
                    session_id: session,
                    authorized_actor_id: actor,
                    capability: AdvisoryCapability::ScopeDecomposition,
                    decision_point: AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
                    decision_point_version: 1,
                    workflow_occurrence_key: request_key.clone(),
                    target_kind: "scope_candidate_set".into(),
                    target_id: Some(candidate),
                    work_revision: Some(3),
                    matrix_task_revision: None,
                    matrix_choice_set_digest: None,
                    matrix_verification_digest: None,
                    source_ref: None,
                    session_preference: AdvisoryRequestPreference::UseWorkspace,
                    request_preference: AdvisoryRequestPreference::UseWorkspace,
                    config_revision: 1,
                    material_digest: "a".repeat(64),
                    state: AdvisoryOpportunityState::NoCall,
                    primary_reason: AdvisoryReason::BudgetPolicyInvalid,
                    parent_opportunity_id: None,
                },
            )
            .await
            .unwrap();
        write.commit().await.unwrap();

        let mut read = store.begin(TransactionMode::ReadOnly).await.unwrap();
        read.authenticate(&enrollment.auth).await.unwrap();
        read.set_tenant(tenant).await.unwrap();
        let loaded = read
            .advisory_opportunity_by_request(workspace, &request_key)
            .await
            .unwrap()
            .unwrap();
        assert_eq!(loaded.id, captured.id);
        assert_eq!(loaded.state, AdvisoryOpportunityState::NoCall);
        assert_eq!(loaded.primary_reason, AdvisoryReason::BudgetPolicyInvalid);
        assert!(!loaded.provider_called);
        let audit = read
            .candidate_advisory_audit(
                workspace,
                candidate,
                &AdvisoryAuditQuery {
                    limit: 10,
                    scope_id: None,
                    after: None,
                    capability: None,
                    decision_point: None,
                    reason: Some(AdvisoryReason::BudgetPolicyInvalid),
                    state: Some(AdvisoryOpportunityState::NoCall),
                },
            )
            .await
            .unwrap();
        read.commit().await.unwrap();
        assert_eq!(audit.opportunities.len(), 1);
        assert_eq!(
            audit.opportunities[0].primary_reason,
            AdvisoryReason::BudgetPolicyInvalid
        );
        assert!(audit.dispatches.is_empty());
        assert_eq!(audit.aggregate.no_call_opportunities, 1);
        assert_eq!(audit.aggregate.authorized_attempts, 0);
        assert_eq!(
            audit.aggregate.no_call_by_reason[0].reason,
            AdvisoryReason::BudgetPolicyInvalid
        );
        assert_eq!(audit.aggregate.no_call_by_reason[0].count, 1);
    }
}
