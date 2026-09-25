use serde::Deserialize;
use serde_json::{Value, json};
use std::future::Future;
use tect_application::{
    EngineeringAdvisoryRead, GuardedMatrixAdviceOutcome, RequestEngineeringAdvisory,
};
use tect_domain::{AdvisoryOpportunity, AdvisoryRequestPreference, Error, Result};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestArguments {
    task_id: Uuid,
    expected_task_revision: i64,
    request_key: String,
    #[serde(default)]
    session_preference: AdvisoryRequestPreference,
    #[serde(default)]
    request_preference: AdvisoryRequestPreference,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct GetArguments {
    task_id: Uuid,
    request_key: String,
}

pub(crate) enum MatrixAdvisoryInvocation {
    Request(RequestEngineeringAdvisory),
    Get { task_id: Uuid, request_key: String },
}

pub(crate) fn parse(name: &str, arguments: Value) -> Result<MatrixAdvisoryInvocation> {
    match name {
        "request_engineering_advisory" => {
            let args: RequestArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.task_id.is_nil()
                || args.expected_task_revision < 1
                || !valid_request_key(&args.request_key)
            {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixAdvisoryInvocation::Request(
                RequestEngineeringAdvisory {
                    task_id: args.task_id,
                    expected_task_revision: args.expected_task_revision,
                    request_key: args.request_key,
                    session_preference: args.session_preference,
                    request_preference: args.request_preference,
                },
            ))
        }
        "get_engineering_advisory" => {
            let args: GetArguments =
                serde_json::from_value(arguments).map_err(Error::invalid_arguments_from)?;
            if args.task_id.is_nil() || !valid_request_key(&args.request_key) {
                return Err(Error::InvalidArguments);
            }
            Ok(MatrixAdvisoryInvocation::Get {
                task_id: args.task_id,
                request_key: args.request_key,
            })
        }
        _ => Err(Error::InvalidArguments),
    }
}

fn valid_request_key(key: &str) -> bool {
    !key.is_empty() && key.len() <= 256 && !key.contains('\0') && key.trim() == key
}

/// The service persists before returning. Reserve space for the largest
/// possible receipt using this request's exact JSON-escaped key. UUIDs have
/// fixed width, digests are 64 hex bytes, i64::MIN is the widest revision,
/// and deterministic_input_invalid is the longest advisory reason.
pub(crate) fn guard_request_output(
    request: &RequestEngineeringAdvisory,
    capacity: usize,
) -> Result<()> {
    let projected = json!({
        "task_id": request.task_id,
        "task_revision": i64::MIN,
        "choice_set_digest": "0".repeat(64),
        "request_key": request.request_key,
        "opportunity_id": Uuid::nil(),
        "state": "no_call",
        "reason": "deterministic_input_invalid",
        "config_revision": i64::MIN,
        "material_digest": "0".repeat(64),
        "provider_called": false,
    });
    let projected = crate::responses::with_actions(projected, Vec::new(), None);
    if crate::responses::encoded_len(&projected)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(())
}

pub(crate) async fn guarded_request<F, Fut>(
    request: &RequestEngineeringAdvisory,
    capacity: usize,
    send: F,
) -> Result<AdvisoryOpportunity>
where
    F: FnOnce() -> Fut,
    Fut: Future<Output = Result<AdvisoryOpportunity>>,
{
    guard_request_output(request, capacity)?;
    send().await
}

pub(crate) fn receipt(value: AdvisoryOpportunity) -> Value {
    json!({
        "task_id": value.target_id,
        "task_revision": value.matrix_task_revision,
        "choice_set_digest": value.matrix_choice_set_digest,
        "request_key": value.workflow_occurrence_key,
        "opportunity_id": value.id,
        "state": value.state,
        "reason": value.primary_reason,
        "config_revision": value.config_revision,
        "material_digest": value.material_digest,
        "provider_called": value.provider_called,
    })
}

pub(crate) fn read(value: EngineeringAdvisoryRead) -> Value {
    let mut receipt = receipt(value.opportunity);
    if let Some(advice) = value.current_advice {
        let outcome = match advice.outcome {
            GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
                json!({"status":"ranked","ranked_choice_ids":ranked_choice_ids})
            }
            GuardedMatrixAdviceOutcome::Abstained { reason } => {
                json!({"status":"abstained","reason":reason})
            }
            GuardedMatrixAdviceOutcome::Rejected { .. } => return receipt,
        };
        receipt["current_advice"] = json!({
            "advice_id": advice.advice_id,
            "dispatch_id": advice.dispatch_id,
            "task_revision": advice.task_revision,
            "input_digest": advice.input_digest,
            "choice_set_id": advice.choice_set_id,
            "choice_set_version": advice.choice_set_version,
            "choice_set_digest": advice.choice_set_digest,
            "evaluation_digest": advice.evaluation_digest,
            "verification_digest": advice.verification_digest,
            "provider_profile_ref": advice.provider_profile_ref,
            "model_configuration": advice.model_configuration,
            "response_payload_sha256": advice.response_payload_sha256,
            "advice_digest": advice.advice_digest,
            "outcome": outcome,
        });
    }
    receipt
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::Cell;

    #[test]
    fn public_read_exposes_only_current_typed_advice() {
        let task_id = Uuid::new_v4();
        let opportunity: AdvisoryOpportunity = serde_json::from_value(json!({
            "id":Uuid::new_v4(),"workspace_id":Uuid::new_v4(),"session_id":Uuid::new_v4(),
            "authorized_actor_id":Uuid::new_v4(),"capability":"engineering_profile",
            "decision_point":"engineering.profile.before_selection","decision_point_version":1,
            "workflow_occurrence_key":"key","target_kind":"matrix_task","target_id":task_id,
            "work_revision":2,"matrix_task_revision":2,"matrix_choice_set_digest":"b".repeat(64),
            "matrix_verification_digest":"d".repeat(64),"source_ref":null,
            "session_preference":"use_workspace","request_preference":"use_workspace",
            "config_revision":1,"material_digest":"c".repeat(64),"state":"advised",
            "primary_reason":"provider_response","provider_called":true
        }))
        .unwrap();
        let base = tect_application::CurrentMatrixAdvice {
            advice_id: Uuid::new_v4(),
            dispatch_id: Uuid::new_v4(),
            task_revision: 2,
            input_digest: "a".repeat(64),
            choice_set_id: "choice".into(),
            choice_set_version: 1,
            choice_set_digest: "b".repeat(64),
            evaluation_digest: "c".repeat(64),
            verification_digest: "d".repeat(64),
            provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
                id: "provider".into(),
            },
            model_configuration: tect_domain::AdvisoryModelConfiguration {
                model: "model".into(),
            },
            response_payload_sha256: "e".repeat(64),
            advice_digest: "f".repeat(64),
            outcome: GuardedMatrixAdviceOutcome::Ranked {
                ranked_choice_ids: vec!["a".into(), "b".into()],
            },
        };
        let ranked = read(EngineeringAdvisoryRead {
            opportunity: opportunity.clone(),
            current_advice: Some(base.clone()),
        });
        assert_eq!(
            ranked["current_advice"]["outcome"]["ranked_choice_ids"],
            json!(["a", "b"])
        );
        assert!(!ranked.to_string().contains("raw_response_payload"));
        let mut abstained = base;
        abstained.outcome = GuardedMatrixAdviceOutcome::Abstained { reason: None };
        let abstained = read(EngineeringAdvisoryRead {
            opportunity: opportunity.clone(),
            current_advice: Some(abstained),
        });
        assert_eq!(
            abstained["current_advice"]["outcome"]["status"],
            "abstained"
        );
        let mut no_call_opportunity = opportunity;
        no_call_opportunity.state = tect_domain::AdvisoryOpportunityState::NoCall;
        no_call_opportunity.primary_reason = tect_domain::AdvisoryReason::RequestSkip;
        no_call_opportunity.provider_called = false;
        let no_call = read(EngineeringAdvisoryRead {
            opportunity: no_call_opportunity,
            current_advice: None,
        });
        assert!(no_call.get("current_advice").is_none());
        assert_eq!(no_call["provider_called"], false);
    }

    #[test]
    fn strict_matrix_advisory_arguments() {
        let id = Uuid::new_v4();
        let base = json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1"});
        assert!(matches!(
            parse("request_engineering_advisory", base.clone()),
            Ok(MatrixAdvisoryInvocation::Request(_))
        ));
        assert!(matches!(
            parse(
                "get_engineering_advisory",
                json!({"task_id":id,"request_key":"task-1"})
            ),
            Ok(MatrixAdvisoryInvocation::Get { .. })
        ));
        for invalid in [
            json!({"task_id":id,"expected_task_revision":0,"request_key":"task-1"}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":" task-1"}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":"x".repeat(257)}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","principal_id":id}),
            json!({"task_id":id,"expected_task_revision":1,"request_key":"task-1","request_preference":"force"}),
        ] {
            assert!(parse("request_engineering_advisory", invalid).is_err());
        }
        assert!(
            parse(
                "get_engineering_advisory",
                json!({"task_id":id,"request_key":"task-1","workspace_id":id})
            )
            .is_err()
        );
    }

    #[tokio::test]
    async fn tiny_output_capacity_rejects_before_service_invocation() {
        let request = RequestEngineeringAdvisory {
            task_id: Uuid::new_v4(),
            expected_task_revision: 1,
            request_key: "matrix-1".into(),
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
        };
        let called = Cell::new(false);
        let result = guarded_request(&request, 1, || {
            called.set(true);
            async { Err(Error::TransportUnavailable) }
        })
        .await;
        assert!(matches!(result, Err(Error::RequestTooLarge)));
        assert!(!called.get());
    }
}
