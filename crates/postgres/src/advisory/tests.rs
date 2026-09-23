#[cfg(test)]
mod audit_projection_tests {
    use super::*;

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
