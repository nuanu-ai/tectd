use crate::{
    AdvisoryProviderReceiptObservation, AdvisoryProviderReceiptUsage,
    AdvisoryProviderTransportContext, ScopeAdviceProviderFailureReason,
    ScopeAdviceProviderObservation, StoredAdvisoryProviderReceipt,
};
use tect_domain::NormalizedScopeAdviceAnswers;

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct ScopeAdviceRawObservation {
    pub receipt: AdvisoryProviderReceiptObservation,
    /// Fresh legacy SPI compatibility only; never durable or recovered.
    pub legacy_answers: Option<NormalizedScopeAdviceAnswers>,
}

impl ScopeAdviceRawObservation {
    pub(crate) fn from_legacy(value: ScopeAdviceProviderObservation) -> Self {
        let complete = value.response_payload.is_some()
            && !matches!(
                value.failure_reason,
                Some(
                    ScopeAdviceProviderFailureReason::ResponseOversize
                        | ScopeAdviceProviderFailureReason::ResponseBodyRead
                )
            );
        Self {
            receipt: AdvisoryProviderReceiptObservation {
                response_payload: value.response_payload,
                http_status: None,
                input_tokens: value.input_tokens.and_then(|n| u64::try_from(n).ok()),
                output_tokens: value.output_tokens.and_then(|n| u64::try_from(n).ok()),
                response_complete: complete,
                original_transport_context: Some(AdvisoryProviderTransportContext {
                    send_certainty: value.send_certainty,
                    outcome: value.outcome,
                    raw_response_ref: value.raw_response_ref,
                    provider_failure_code: value
                        .failure_reason
                        .map(|reason| reason.as_code().to_owned()),
                }),
            },
            legacy_answers: value.answers,
        }
    }
}

impl ScopeAdviceProviderFailureReason {
    pub fn as_code(self) -> &'static str {
        match self {
            Self::HttpStatus => "http-status",
            Self::InvalidContentType => "content-type",
            Self::ResponseOversize => "oversize",
            Self::ResponseBodyRead => "body-read",
            Self::InvalidResponse => "invalid-response",
        }
    }
    pub fn from_code(code: &str) -> Option<Self> {
        Some(match code {
            "http-status" => Self::HttpStatus,
            "content-type" => Self::InvalidContentType,
            "oversize" => Self::ResponseOversize,
            "body-read" => Self::ResponseBodyRead,
            "invalid-response" => Self::InvalidResponse,
            _ => return None,
        })
    }
}

pub(crate) fn original_usage(
    saved: &StoredAdvisoryProviderReceipt,
) -> AdvisoryProviderReceiptUsage {
    receipt_usage(saved.observation.as_ref())
}

fn receipt_usage(raw: Option<&AdvisoryProviderReceiptObservation>) -> AdvisoryProviderReceiptUsage {
    raw.filter(|raw| raw.response_complete && raw.response_payload.is_some())
        .map(|raw| AdvisoryProviderReceiptUsage {
            input_tokens: raw.input_tokens,
            output_tokens: raw.output_tokens,
        })
        .unwrap_or_default()
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{AdvisoryDispatchOutcome, AdvisorySendCertainty};

    fn legacy(reason: Option<ScopeAdviceProviderFailureReason>) -> ScopeAdviceProviderObservation {
        ScopeAdviceProviderObservation {
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: AdvisoryDispatchOutcome::ProviderFailure,
            answers: None,
            response_payload: Some(b"raw-prefix".to_vec()),
            input_tokens: Some(2),
            output_tokens: Some(3),
            latency_ms: Some(7),
            raw_response_ref: Some("original-provider-ref".to_owned()),
            failure_reason: reason,
        }
    }

    #[test]
    fn legacy_bridge_retains_original_transport_facts_without_ref_parsing() {
        for reason in [
            ScopeAdviceProviderFailureReason::HttpStatus,
            ScopeAdviceProviderFailureReason::InvalidContentType,
            ScopeAdviceProviderFailureReason::ResponseOversize,
            ScopeAdviceProviderFailureReason::ResponseBodyRead,
            ScopeAdviceProviderFailureReason::InvalidResponse,
        ] {
            let raw = ScopeAdviceRawObservation::from_legacy(legacy(Some(reason)));
            let context = raw.receipt.original_transport_context.as_ref().unwrap();
            assert_eq!(context.send_certainty, AdvisorySendCertainty::Sent);
            assert_eq!(context.outcome, AdvisoryDispatchOutcome::ProviderFailure);
            assert_eq!(
                context.raw_response_ref.as_deref(),
                Some("original-provider-ref")
            );
            assert_eq!(
                context.provider_failure_code.as_deref(),
                Some(reason.as_code())
            );
            assert_eq!(
                ScopeAdviceProviderFailureReason::from_code(reason.as_code()),
                Some(reason)
            );
            context.validate_for(&raw.receipt.response_payload).unwrap();
        }
        assert_eq!(
            ScopeAdviceProviderFailureReason::from_code("future-opaque-code"),
            None
        );
    }

    #[test]
    fn original_usage_never_claims_partial_or_absent_response_counters() {
        for reason in [
            ScopeAdviceProviderFailureReason::ResponseOversize,
            ScopeAdviceProviderFailureReason::ResponseBodyRead,
        ] {
            let raw = ScopeAdviceRawObservation::from_legacy(legacy(Some(reason)));
            assert!(!raw.receipt.response_complete);
            assert_eq!(
                receipt_usage(Some(&raw.receipt)),
                AdvisoryProviderReceiptUsage::default()
            );
        }
        let mut raw = ScopeAdviceRawObservation::from_legacy(legacy(None));
        assert_eq!(
            receipt_usage(Some(&raw.receipt)),
            AdvisoryProviderReceiptUsage {
                input_tokens: Some(2),
                output_tokens: Some(3),
            }
        );
        raw.receipt.response_payload = None;
        assert_eq!(
            receipt_usage(Some(&raw.receipt)),
            AdvisoryProviderReceiptUsage::default()
        );
    }
}
