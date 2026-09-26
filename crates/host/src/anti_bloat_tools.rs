use serde::Deserialize;
use serde_json::{Value, json};
use tect_application::{
    AntiBloatAttemptState, AntiBloatAuthoredDelta, AntiBloatNoCall, StoredAntiBloatReview,
    VerifyAntiBloatApply,
};
use tect_domain::{
    AdvisoryRequestPreference, AntiBloatDisposition, CandidateDeltaBatch, Error, Result,
};
use uuid::Uuid;

pub(crate) enum AntiBloatInvocation {
    Prepare {
        candidate_set_id: Uuid,
        expected_revision: i64,
        preference: AdvisoryRequestPreference,
    },
    Run {
        review_id: Uuid,
    },
    Get {
        review_id: Uuid,
    },
    Apply(AntiBloatAuthoredDelta),
    PreservationGet {
        review_id: Uuid,
    },
    PreservationVerify(VerifyAntiBloatApply),
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct PrepareArguments {
    candidate_set_id: Uuid,
    expected_revision: i64,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ReviewArguments {
    review_id: Uuid,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ApplyArguments {
    review_id: Uuid,
    finding_id: String,
    disposition: AntiBloatDisposition,
    delta: CandidateDeltaBatch,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct VerifyPreservationArguments {
    request_id: Uuid,
    review_id: Uuid,
    expected_evidence_digest: String,
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<AntiBloatInvocation> {
    match name {
        "anti_bloat_prepare" => {
            let args: PrepareArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.candidate_set_id.is_nil() || args.expected_revision < 1 {
                return Err(Error::InvalidArguments);
            }
            Ok(AntiBloatInvocation::Prepare {
                candidate_set_id: args.candidate_set_id,
                expected_revision: args.expected_revision,
                preference: args.request_preference,
            })
        }
        "anti_bloat_run" | "anti_bloat_get" => {
            let args: ReviewArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.review_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            if name == "anti_bloat_run" {
                Ok(AntiBloatInvocation::Run {
                    review_id: args.review_id,
                })
            } else {
                Ok(AntiBloatInvocation::Get {
                    review_id: args.review_id,
                })
            }
        }
        "anti_bloat_apply" => {
            let args: ApplyArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.review_id.is_nil()
                || args.finding_id.len() != 64
                || !args
                    .finding_id
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
                || args.disposition != AntiBloatDisposition::Narrow
            {
                return Err(Error::InvalidArguments);
            }
            args.delta.validate()?;
            Ok(AntiBloatInvocation::Apply(AntiBloatAuthoredDelta {
                review_id: args.review_id,
                finding_id: args.finding_id,
                disposition: args.disposition,
                delta: args.delta,
            }))
        }
        "anti_bloat_preservation_get" => {
            let args: ReviewArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.review_id.is_nil() {
                return Err(Error::InvalidArguments);
            }
            Ok(AntiBloatInvocation::PreservationGet {
                review_id: args.review_id,
            })
        }
        "anti_bloat_preservation_verify" => {
            let args: VerifyPreservationArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.request_id.is_nil()
                || args.review_id.is_nil()
                || args.expected_evidence_digest.len() != 64
                || !args
                    .expected_evidence_digest
                    .bytes()
                    .all(|byte| byte.is_ascii_digit() || (b'a'..=b'f').contains(&byte))
            {
                return Err(Error::InvalidArguments);
            }
            Ok(AntiBloatInvocation::PreservationVerify(
                VerifyAntiBloatApply {
                    request_id: args.request_id,
                    review_id: args.review_id,
                    expected_evidence_digest: args.expected_evidence_digest,
                },
            ))
        }
        _ => Err(Error::InvalidArguments),
    }
}

pub(crate) fn state(value: &AntiBloatAttemptState) -> Value {
    match value {
        AntiBloatAttemptState::NoCall(reason) => json!({"status":"no_call","reason":match reason {
            AntiBloatNoCall::Disabled => "disabled", AntiBloatNoCall::Skipped => "skipped",
            AntiBloatNoCall::NoEligibleFindings => "no_eligible_findings",
            AntiBloatNoCall::ProviderUnconfigured => "provider_unconfigured",
            AntiBloatNoCall::PreflightInvalidConfiguration => "preflight_invalid_configuration",
            AntiBloatNoCall::PreflightInvalidArguments => "preflight_invalid_arguments",
            AntiBloatNoCall::PreflightInputConflict => "preflight_input_conflict",
            AntiBloatNoCall::PreflightRequestTooLarge => "preflight_request_too_large",
        }}),
        AntiBloatAttemptState::Prepared => json!({"status":"prepared"}),
        AntiBloatAttemptState::Sending => json!({"status":"sending"}),
        AntiBloatAttemptState::Ranked(ids) => json!({"status":"ranked","ranked_ids":ids}),
        AntiBloatAttemptState::SendUnknown => json!({"status":"send_unknown"}),
        AntiBloatAttemptState::ProviderAbstained => json!({"status":"provider_abstained"}),
        AntiBloatAttemptState::InvalidResponse => json!({"status":"invalid_response"}),
    }
}

pub(crate) fn review(value: StoredAntiBloatReview) -> Value {
    json!({
        "review_id": value.review_id,
        "candidate_set_id": value.review.candidate_set_id,
        "plan_revision": value.review.plan_revision,
        "source_digest": value.review.source_digest,
        "whole_set_digest": value.review.whole_set_digest,
        "material_digest": value.review.material_digest,
        "dependency_digest": value.review.dependency_digest,
        "selected_id": value.review.selected_id,
        "findings": value.review.findings,
        "state": state(&value.state),
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejects_malformed_and_cross_action_inputs() {
        let id = Uuid::new_v4();
        assert!(
            parse(
                "anti_bloat_prepare",
                json!({"candidate_set_id":id,"expected_revision":0})
            )
            .is_err()
        );
        assert!(parse("anti_bloat_run", json!({"review_id":Uuid::nil()})).is_err());
        assert!(parse("anti_bloat_get", json!({"review_id":id,"extra":true})).is_err());
        assert!(
            parse(
                "anti_bloat_preservation_get",
                json!({"review_id":Uuid::nil()})
            )
            .is_err()
        );
        assert!(
            parse(
                "anti_bloat_preservation_get",
                json!({"review_id":id,"extra":true})
            )
            .is_err()
        );
        for params in [
            json!({"request_id":id,"review_id":id,"expected_evidence_digest":"bad"}),
            json!({"request_id":Uuid::nil(),"review_id":id,"expected_evidence_digest":"a".repeat(64)}),
            json!({"request_id":id,"review_id":id,"expected_evidence_digest":"a".repeat(64),"verdict":"pass"}),
        ] {
            assert!(parse("anti_bloat_preservation_verify", params).is_err());
        }
        assert!(
            parse(
                "anti_bloat_apply",
                json!({"review_id":id,"finding_id":"bad","disposition":"narrow","delta":{}})
            )
            .is_err()
        );
    }

    #[test]
    fn public_routes_decode_to_typed_internal_calls() {
        let id = Uuid::new_v4();
        for (tool, route, name, params) in [
            (
                "command",
                "scope.anti_bloat.prepare",
                "anti_bloat_prepare",
                json!({"candidate_set_id":id,"expected_revision":3,"request_preference":"skip"}),
            ),
            (
                "command",
                "scope.anti_bloat.run",
                "anti_bloat_run",
                json!({"review_id":id}),
            ),
            (
                "query",
                "scope.anti_bloat.get",
                "anti_bloat_get",
                json!({"review_id":id}),
            ),
            (
                "query",
                "scope.anti_bloat.preservation.get",
                "anti_bloat_preservation_get",
                json!({"review_id":id}),
            ),
            (
                "command",
                "scope.anti_bloat.preservation.verify",
                "anti_bloat_preservation_verify",
                json!({"request_id":id,"review_id":id,"expected_evidence_digest":"a".repeat(64)}),
            ),
        ] {
            let call = crate::api::decode_public_call(tool, json!({"route":route,"params":params}))
                .unwrap();
            assert_eq!(call.name, name);
            assert!(parse(call.name, call.arguments).is_ok());
        }
    }
}
