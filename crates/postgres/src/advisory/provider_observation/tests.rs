#[cfg(test)]
mod provider_observation_tests {
    use super::*;

    #[test]
    fn pipeline_receipt_family_rejects_all_crossed_families() {
        for capability in [
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryCapability::EngineeringProfile,
            AdvisoryCapability::PipelineRecommendation,
        ] {
            for decision in [
                AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
                AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
                AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
            ] {
                for target in ["scope_candidate_set", "matrix_task", "slice_candidate_node"] {
                    for target_id in [None, Some(Uuid::from_u128(1))] {
                        assert_eq!(
                            pipeline_receipt_identity(capability, decision, target, target_id),
                            capability == AdvisoryCapability::PipelineRecommendation
                                && decision
                                    == AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
                                && target == "slice_candidate_node"
                                && target_id.is_some(),
                        );
                    }
                    assert_eq!(
                        pipeline_receipt_family(capability, decision, target),
                        capability == AdvisoryCapability::PipelineRecommendation
                            && decision
                                == AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen
                            && target == "slice_candidate_node",
                    );
                }
            }
        }
        assert!(!pipeline_receipt_family(
            AdvisoryCapability::PipelineRecommendation,
            AdvisoryDecisionPoint::PipelineRecommendationBeforeSliceOpen,
            "",
        ));
    }

    #[test]
    fn scope_receipt_reader_and_terminal_family_are_exact() {
        assert!(scope_receipt_family(
            AdvisoryCapability::ScopeDecomposition,
            AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
            "scope_candidate_set",
        ));
        for (capability, decision, target) in [
            (
                AdvisoryCapability::ScopeDecomposition,
                AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
                "scope_candidate_set",
            ),
            (
                AdvisoryCapability::ScopeDecomposition,
                AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
                "matrix_task",
            ),
            (
                AdvisoryCapability::EngineeringProfile,
                AdvisoryDecisionPoint::EngineeringProfileBeforeSelection,
                "matrix_task",
            ),
            (
                AdvisoryCapability::PipelineRecommendation,
                AdvisoryDecisionPoint::ScopeDecompositionBeforeSelection,
                "scope_candidate_set",
            ),
        ] {
            assert!(!scope_receipt_family(capability, decision, target));
        }
    }

    #[test]
    fn scope_transport_failure_preserves_classification_ref_and_partial_unknown_usage() {
        let context = AdvisoryProviderTransportContext {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            raw_response_ref: Some("exact-original-ref".to_owned()),
            provider_failure_code: Some("opaque-provider-code".to_owned()),
        };
        let mut seal = provider_usage_seal(
            Uuid::new_v4(),
            Some(b"prefix".to_vec()),
            AdvisoryProviderReceiptUsage {
                input_tokens: Some(3),
                output_tokens: Some(4),
            },
            Some(19),
            false,
        );
        apply_scope_transport_context(&mut seal, Some(&context)).unwrap();
        assert_eq!(seal.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderFailure);
        assert_eq!(seal.raw_response_ref, context.raw_response_ref);
        assert_eq!((seal.input_tokens, seal.output_tokens), (None, None));
        assert_eq!(seal.latency_ms, Some(19));
        assert_eq!(
            apply_scope_transport_context(&mut seal, None),
            Err(Error::InputConflict)
        );
        let mut contradictory = context;
        contradictory.send_certainty = AdvisorySendCertainty::SentUnknown;
        assert_eq!(
            apply_scope_transport_context(&mut seal, Some(&contradictory)),
            Err(Error::InvalidArguments)
        );
    }

    #[test]
    fn received_empty_or_malformed_bytes_remain_known_received_with_original_elapsed() {
        for raw in [
            Vec::new(),
            b"provider HTTP error is not JSON".to_vec(),
            vec![0xff, 0],
        ] {
            let seal = provider_usage_seal(
                Uuid::new_v4(),
                Some(raw.clone()),
                AdvisoryProviderReceiptUsage::default(),
                Some(23),
                true,
            );
            seal.validate().unwrap();
            assert_eq!(seal.send_certainty, AdvisorySendCertainty::Sent);
            assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderResponse);
            assert_eq!(seal.response_payload, Some(raw));
            assert_eq!(seal.latency_ms, Some(23));
        }
    }

    #[test]
    fn transport_failure_and_partial_cannot_claim_known_usage() {
        let seal = provider_usage_seal(
            Uuid::new_v4(),
            None,
            AdvisoryProviderReceiptUsage::default(),
            Some(5),
            false,
        );
        assert_eq!(seal.send_certainty, AdvisorySendCertainty::SentUnknown);
        assert_eq!(seal.outcome, AdvisoryDispatchOutcome::ProviderFailure);
        let supplied = AdvisoryProviderReceiptUsage {
            input_tokens: Some(1),
            output_tokens: Some(2),
        };
        let absent = provider_usage_seal(Uuid::new_v4(), None, supplied, Some(7), true);
        assert_eq!((absent.input_tokens, absent.output_tokens), (None, None));
        let partial = provider_usage_seal(
            Uuid::new_v4(),
            Some(b"prefix".to_vec()),
            supplied,
            Some(7),
            false,
        );
        assert_eq!(partial.send_certainty, AdvisorySendCertainty::Sent);
        assert_eq!(
            partial.response_payload.as_deref(),
            Some(b"prefix".as_slice())
        );
        assert_eq!((partial.input_tokens, partial.output_tokens), (None, None));
        let overflow = provider_usage_seal(
            Uuid::new_v4(),
            Some(b"raw".to_vec()),
            AdvisoryProviderReceiptUsage {
                input_tokens: Some(u64::MAX),
                output_tokens: Some(4),
            },
            Some(5),
            true,
        );
        assert_eq!(
            (overflow.input_tokens, overflow.output_tokens),
            (None, Some(4))
        );
    }
}
