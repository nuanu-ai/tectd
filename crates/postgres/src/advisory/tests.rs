#[cfg(test)]
mod audit_projection_tests {
    use super::*;

    #[test]
    fn request_lookup_is_tenant_and_workspace_scoped_and_selects_matrix_binding() {
        let source = include_str!("config_opportunity.rs");
        let query = source
            .split("async fn opportunity_by_request_key(")
            .nth(1)
            .expect("request lookup exists");
        assert!(query.contains("WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.request_key=$3"));
        assert!(query.contains("d.send_certainty='sent') AS provider_called"));
        assert!(query.contains("o.matrix_task_revision,o.matrix_choice_set_digest"));
        assert!(query.contains(".bind(tenant)"));
        assert!(query.contains(".bind(workspace)"));
        assert!(query.contains(".bind(request_key)"));
    }

    #[test]
    fn matrix_parent_query_and_capture_keep_workspace_boundary_and_parent_identity() {
        let source = include_str!("config_opportunity.rs");
        let lookup = source
            .split("pub(crate) async fn matrix_decomposition_parent(")
            .nth(1)
            .expect("matrix parent lookup exists");
        assert!(lookup.contains("o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3"));
        assert!(lookup.contains("o.capability='engineering_profile'"));
        assert!(lookup.contains("o.work_item_kind='matrix_task'"));
        assert!(lookup.contains(".bind(tenant)"));
        assert!(lookup.contains(".bind(workspace)"));
        assert!(lookup.contains(".bind(opportunity_id)"));

        let capture = source
            .split("async fn capture_opportunity(")
            .nth(1)
            .unwrap()
            .split("async fn opportunity_by_id(")
            .next()
            .unwrap();
        assert!(capture.contains("primary_reason,parent_opportunity_id) VALUES"));
        assert!(capture.contains(".bind(input.parent_opportunity_id)"));
        assert!(capture.contains("row.parent_opportunity_id != input.parent_opportunity_id"));
    }

    #[test]
    fn matrix_binding_survives_opportunity_row_projection() {
        let workspace = Uuid::new_v4();
        let task = Uuid::new_v4();
        let choice_digest = "a".repeat(64);
        let opportunity = opportunity_from_row(
            workspace,
            OpportunityRow {
                id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                authorized_actor_id: Uuid::new_v4(),
                work_item_kind: "matrix_task".into(),
                work_item_id: Some(task),
                source_revision: Some("7".into()),
                matrix_task_revision: Some(7),
                matrix_choice_set_digest: Some(choice_digest.clone()),
                matrix_verification_digest: None,
                capability: "engineering_profile".into(),
                decision_point: ENGINEERING_PROFILE_DECISION_POINT.into(),
                config_revision: 3,
                session_preference: "use_workspace".into(),
                request_preference: "use_workspace".into(),
                request_key: "request".into(),
                material_digest: "b".repeat(64),
                state: "no_call".into(),
                primary_reason: "capability_unavailable".into(),
                parent_opportunity_id: None,
                provider_called: false,
            },
        )
        .unwrap();
        assert_eq!(opportunity.workspace_id, workspace);
        assert_eq!(opportunity.target_id, Some(task));
        assert_eq!(opportunity.matrix_task_revision, Some(7));
        assert_eq!(
            opportunity.matrix_choice_set_digest.as_deref(),
            Some(choice_digest.as_str())
        );
        assert_eq!(
            opportunity.primary_reason,
            AdvisoryReason::CapabilityUnavailable
        );
        assert!(!opportunity.provider_called);
    }

    #[test]
    fn confirmed_dispatch_send_survives_opportunity_row_projection() {
        let opportunity = opportunity_from_row(
            Uuid::new_v4(),
            OpportunityRow {
                id: Uuid::new_v4(),
                session_id: Uuid::new_v4(),
                authorized_actor_id: Uuid::new_v4(),
                work_item_kind: "matrix_task".into(),
                work_item_id: Some(Uuid::new_v4()),
                source_revision: Some("1".into()),
                matrix_task_revision: Some(1),
                matrix_choice_set_digest: Some("a".repeat(64)),
                matrix_verification_digest: Some("b".repeat(64)),
                capability: "engineering_profile".into(),
                decision_point: ENGINEERING_PROFILE_DECISION_POINT.into(),
                config_revision: 1,
                session_preference: "use_workspace".into(),
                request_preference: "use_workspace".into(),
                request_key: "request".into(),
                material_digest: "c".repeat(64),
                state: "advised".into(),
                primary_reason: "provider_response".into(),
                parent_opportunity_id: None,
                provider_called: true,
            },
        )
        .unwrap();
        assert!(opportunity.provider_called);
    }

    #[test]
    fn finalization_preserves_confirmed_send_readback_for_every_certainty() {
        let source = include_str!("config_opportunity.rs");
        for lookup in [
            "async fn opportunity_by_id(",
            "async fn opportunity_by_request_key(",
        ] {
            let query = source
                .split(lookup)
                .nth(1)
                .expect("opportunity lookup exists");
            assert!(query.contains("d.send_certainty='sent') AS provider_called"));
        }

        for (latest_certainty, earlier_sent, provider_called) in [
            (AdvisorySendCertainty::Sent, false, true),
            (AdvisorySendCertainty::NotSent, false, false),
            (AdvisorySendCertainty::SentUnknown, false, false),
            (AdvisorySendCertainty::SentUnknown, true, true),
        ] {
            assert_eq!(
                provider_called,
                latest_certainty == AdvisorySendCertainty::Sent || earlier_sent
            );
            let (state, reason) = if latest_certainty == AdvisorySendCertainty::SentUnknown {
                (
                    AdvisoryOpportunityState::Unresolved,
                    AdvisoryReason::SendUnknown,
                )
            } else {
                (
                    AdvisoryOpportunityState::Failed,
                    AdvisoryReason::ProviderFailure,
                )
            };
            let opportunity = opportunity_from_row(
                Uuid::new_v4(),
                OpportunityRow {
                    id: Uuid::new_v4(),
                    session_id: Uuid::new_v4(),
                    authorized_actor_id: Uuid::new_v4(),
                    work_item_kind: "scope".into(),
                    work_item_id: Some(Uuid::new_v4()),
                    source_revision: None,
                    matrix_task_revision: None,
                    matrix_choice_set_digest: None,
                    matrix_verification_digest: None,
                    capability: "scope_decomposition".into(),
                    decision_point: SCOPE_DECOMPOSITION_DECISION_POINT.into(),
                    config_revision: 1,
                    session_preference: "use_workspace".into(),
                    request_preference: "use_workspace".into(),
                    request_key: "request".into(),
                    material_digest: "a".repeat(64),
                    state: "awaiting_response".into(),
                    primary_reason: "dispatch_authorized".into(),
                    parent_opportunity_id: None,
                    provider_called,
                },
            )
            .unwrap();
            let finalized = finalized_opportunity(opportunity, state, reason);
            assert_eq!(finalized.provider_called, provider_called);
            let replay = finalized_opportunity(finalized, state, reason);
            assert_eq!(replay.provider_called, provider_called);
        }
    }

    #[test]
    fn engineering_profile_decision_point_decodes_without_changing_scope_or_unknown_rows() {
        assert_eq!(
            decision_point(ENGINEERING_PROFILE_DECISION_POINT),
            Ok(AdvisoryDecisionPoint::EngineeringProfileBeforeSelection)
        );
        assert_eq!(
            decision_point(SCOPE_DECOMPOSITION_DECISION_POINT),
            Ok(AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection)
        );
        assert_eq!(
            decision_point("engineering.profile.unknown"),
            Err(Error::StorageUnavailable)
        );
    }

    #[test]
    fn pipeline_prepare_opportunity_decodes_after_insert() {
        assert_eq!(
            decision_point(PIPELINE_RECOMMENDATION_DECISION_POINT),
            Ok(AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen)
        );
        assert_eq!(
            reason("recommendation_prepared"),
            Ok(AdvisoryReason::RecommendationPrepared)
        );
    }

    #[test]
    fn choice_set_not_applicable_reason_parses_and_survives_audit_projection() {
        assert_eq!(
            reason("matrix_evidence_unresolved").unwrap(),
            AdvisoryReason::MatrixEvidenceUnresolved
        );
        assert_eq!(
            reason("matrix_source_unverified").unwrap(),
            AdvisoryReason::MatrixSourceUnverified
        );
        assert_eq!(
            reason("choice_set_not_applicable").unwrap(),
            AdvisoryReason::ChoiceSetNotApplicable
        );
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
                reason: "choice_set_not_applicable".into(),
                count: 1,
            }],
        )
        .unwrap();
        assert_eq!(
            aggregate.no_call_by_reason[0].reason,
            AdvisoryReason::ChoiceSetNotApplicable
        );
    }

    #[test]
    fn matrix_revision_change_reason_decodes_for_audit() {
        assert_eq!(
            reason("matrix_task_revision_changed"),
            Ok(AdvisoryReason::MatrixTaskRevisionChanged)
        );
        assert_eq!(
            AdvisoryReason::MatrixTaskRevisionChanged.as_str(),
            "matrix_task_revision_changed"
        );
    }

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
        assert_eq!(
            aggregate.no_call_by_reason[0].reason,
            AdvisoryReason::BudgetPolicyInvalid
        );
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
            "selected_save_observation",
        ] {
            assert_eq!(projected[field], serde_json::Value::Null);
        }
        assert!(projected.get("request_payload").is_none());
        assert!(projected.get("response_payload").is_none());
        assert!(projected.get("configuration_snapshot").is_none());
    }
}

include!("tests/budget_reason_persistence.rs");

#[cfg(test)]
mod pipeline_budget_sql_contract_tests {
    #[test]
    fn failure_seal_still_requires_explicit_unknown_delivery_and_no_response() {
        let sql = include_str!("../../migrations/0086_pipeline_budget_failure_seal.sql");
        assert!(sql.contains("NEW.send_certainty='sent_unknown' AND NEW.outcome='provider_failure'"));
        assert!(sql.contains("NEW.response_payload IS NOT NULL OR NEW.pipeline_response_sha256 IS NOT NULL"));
        assert!(sql.contains("NEW.input_tokens IS NOT NULL OR NEW.output_tokens IS NOT NULL"));
    }

    #[test]
    fn exhaustion_verdict_is_checked_against_authoritative_measurements() {
        let sql = include_str!("../../migrations/0087_advisory_budget_consumption_exhaustion_guard.sql");
        for required in [
            "FOR UPDATE",
            "NOT NEW.exhausted_after_response",
            "NEW.unknown_usage",
            "prior_exhausted",
            "NEW.monotonic_elapsed_ms > reservation.remaining_elapsed_ms",
            "prior_input + NEW.input_tokens::numeric > input_limit::numeric",
            "prior_output + NEW.output_tokens::numeric > output_limit::numeric",
            "prior_elapsed + NEW.monotonic_elapsed_ms::numeric > elapsed_limit::numeric",
        ] {
            assert!(sql.contains(required), "missing {required}");
        }
    }
}
