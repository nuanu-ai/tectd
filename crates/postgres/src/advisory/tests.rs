#[cfg(test)]
mod audit_projection_tests {
    use super::*;

    #[test]
    fn budget_policy_invalid_reason_survives_audit_projection() {
        let aggregate = audit_aggregate_from_rows(
            AuditAggregateRow {
                opportunities: 1,
                opportunities_with_attempts: 0,
                no_call_opportunities: 1,
                authorized_attempts: 0,
                confirmed_sent_attempts: 0,
                send_unknown_attempts: 0,
                proven_unsent_attempts: 0,
                known_input_tokens: 0,
                known_output_tokens: 0,
                attempts_with_unknown_token_usage: 0,
            },
            vec![ReasonCountRow {
                reason: "budget_policy_invalid".into(),
                count: 1,
            }],
        )
        .unwrap();
        assert_eq!(aggregate.no_call_by_reason[0].reason, AdvisoryReason::BudgetPolicyInvalid);
        assert_eq!(aggregate.no_call_by_reason[0].count, 1);
        assert_eq!(aggregate.authorized_attempts, 0);
    }

    #[test]
    fn aggregate_keeps_no_call_unknown_send_and_unknown_usage_distinct() {
        let aggregate = audit_aggregate_from_rows(
            AuditAggregateRow {
                opportunities: 4,
                opportunities_with_attempts: 2,
                no_call_opportunities: 2,
                authorized_attempts: 3,
                confirmed_sent_attempts: 1,
                send_unknown_attempts: 1,
                proven_unsent_attempts: 1,
                known_input_tokens: 7,
                known_output_tokens: 11,
                attempts_with_unknown_token_usage: 2,
            },
            vec![
                ReasonCountRow {
                    reason: "request_skip".into(),
                    count: 1,
                },
                ReasonCountRow {
                    reason: "workspace_disabled".into(),
                    count: 1,
                },
            ],
        )
        .unwrap();
        assert_eq!(aggregate.no_call_opportunities, 2);
        assert_eq!(aggregate.opportunities_with_attempts, 2);
        assert_eq!(aggregate.confirmed_sent_attempts, 1);
        assert_eq!(aggregate.send_unknown_attempts, 1);
        assert_eq!(aggregate.proven_unsent_attempts, 1);
        assert_eq!(aggregate.attempts_with_unknown_token_usage, 2);
        assert_eq!(aggregate.no_call_by_reason.len(), 2);
    }

    #[test]
    fn unresolved_dispatch_projection_preserves_nullable_measurements_and_lineage() {
        let predecessor = Uuid::new_v4();
        let dispatch = dispatch_audit_from_row(DispatchAuditRow {
            id: Uuid::new_v4(),
            opportunity_id: Uuid::new_v4(),
            predecessor_dispatch_id: Some(predecessor),
            attempt_number: 2,
            provider: "fixture".into(),
            model: "fixture-model".into(),
            configuration_digest: "a".repeat(64),
            material_digest: "b".repeat(64),
            payload_digest: "c".repeat(64),
            request_bytes: 17,
            response_bytes: None,
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: "sending".into(),
            send_certainty: "sent_unknown".into(),
            outcome: None,
            retry_basis: "proven_not_sent".into(),
            raw_response_ref: None,
            authorized_at: "2026-09-22T00:00:00.000000Z".into(),
            send_started_at: Some("2026-09-22T00:00:01.000000Z".into()),
            sealed_at: None,
        })
        .unwrap();
        assert_eq!(dispatch.predecessor_dispatch_id, Some(predecessor));
        assert_eq!(dispatch.send_certainty, AdvisorySendCertainty::SentUnknown);
        assert_eq!(dispatch.state, AdvisoryDispatchState::Sending);
        assert_eq!(dispatch.response_bytes, None);
        assert_eq!(dispatch.input_tokens, None);
        assert_eq!(dispatch.output_tokens, None);
        assert_eq!(dispatch.latency_ms, None);
        assert_eq!(dispatch.outcome, None);
    }

    #[test]
    fn opportunity_projection_emits_future_links_as_null_without_raw_material() {
        let opportunity = opportunity_audit_from_row(OpportunityAuditRow {
            id: Uuid::new_v4(),
            workspace_id: Uuid::new_v4(),
            scope_id: Some(Uuid::new_v4()),
            session_id: Uuid::new_v4(),
            authorized_actor_id: Uuid::new_v4(),
            work_item_kind: "scope".into(),
            work_item_id: Some(Uuid::new_v4()),
            source_revision: Some("7".into()),
            run_id: None,
            phase: None,
            step: Some("selection".into()),
            capability: "scope_decomposition".into(),
            decision_point: SCOPE_DECOMPOSITION_DECISION_POINT.into(),
            config_revision: 3,
            session_preference: "use_workspace".into(),
            request_preference: "skip".into(),
            policy_version: ADVISORY_POLICY_VERSION.into(),
            request_key: Uuid::new_v4().to_string(),
            material_digest: "a".repeat(64),
            deterministic_baseline_ref: None,
            eligible_material_ref: None,
            state: "no_call".into(),
            primary_reason: "request_skip".into(),
            parent_opportunity_id: None,
            created_at: "2026-09-22T00:00:00.000000Z".into(),
            updated_at: "2026-09-22T00:00:00.000000Z".into(),
        })
        .unwrap();
        let projected = serde_json::to_value(opportunity).unwrap();
        for field in [
            "guarded_advice_id",
            "guarded_advice_digest",
            "disposition_id",
            "preservation_receipt_id",
            "preservation_status",
            "caller_receipt_id",
            "caller_link_id",
            "verifier_receipt_id",
        ] {
            assert_eq!(projected[field], serde_json::Value::Null);
        }
        assert!(projected.get("request_payload").is_none());
        assert!(projected.get("response_payload").is_none());
        assert!(projected.get("configuration_snapshot").is_none());
    }
}

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
        sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
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
                    source_ref: None,
                    session_preference: AdvisoryRequestPreference::UseWorkspace,
                    request_preference: AdvisoryRequestPreference::UseWorkspace,
                    config_revision: 1,
                    material_digest: "a".repeat(64),
                    state: AdvisoryOpportunityState::NoCall,
                    primary_reason: AdvisoryReason::BudgetPolicyInvalid,
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
        assert_eq!(audit.opportunities[0].primary_reason, AdvisoryReason::BudgetPolicyInvalid);
        assert!(audit.dispatches.is_empty());
        assert_eq!(audit.aggregate.no_call_opportunities, 1);
        assert_eq!(audit.aggregate.authorized_attempts, 0);
        assert_eq!(audit.aggregate.no_call_by_reason[0].reason, AdvisoryReason::BudgetPolicyInvalid);
        assert_eq!(audit.aggregate.no_call_by_reason[0].count, 1);
    }
}
