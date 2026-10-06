use serde::Deserialize;
use serde_json::{Value, json};
use std::future::Future;
use tect_application::{
    EngineeringAdvisoryRead, GuardedMatrixAdviceOutcome, RequestEngineeringAdvisory,
};
use tect_domain::{
    AdvisoryOpportunity, AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference,
    Error, RequestContext, Result,
};
use uuid::Uuid;

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct RequestArguments {
    task_id: Uuid,
    expected_task_revision: i64,
    request_key: String,
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
                    // The service replaces this placeholder with the durable session preference.
                    session_preference: AdvisoryRequestPreference::UseWorkspace,
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

/// Bound the persisted receipt before invoking the service, not provider policy.
/// NoCall/MatrixOperatingEvidenceUnresolved is the largest valid current
/// state/reason pair. UUIDs and digests have fixed widths; populated optional
/// fields, i64::MIN and false conservatively cover shorter representations.
/// Use the exact validated key and the actual complete response encoding.
fn request_projection(request: &RequestEngineeringAdvisory) -> Value {
    crate::responses::with_actions(
        json!({
            "task_id": request.task_id,
            "task_revision": i64::MIN,
            "choice_set_digest": "0".repeat(64),
            "request_key": request.request_key,
            "opportunity_id": Uuid::nil(),
            "state": AdvisoryOpportunityState::NoCall,
            "reason": AdvisoryReason::MatrixOperatingEvidenceUnresolved,
            "config_revision": i64::MIN,
            "material_digest": "0".repeat(64),
            "provider_called": false,
        }),
        Vec::new(),
        None,
    )
}

pub(crate) fn guard_request_output(
    request: &RequestEngineeringAdvisory,
    capacity: usize,
) -> Result<()> {
    if crate::responses::encoded_len(&request_projection(request))? > capacity {
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
        let trial_uncertainty = advice.trial_evidence.as_ref().map(|evidence| {
            let mut metadata = json!(evidence);
            metadata["schema"] = json!("tect.matrix-trial-uncertainty/1");
            metadata["digest_linkage"] = json!({
                "input_digest": advice.input_digest,
                "choice_set_digest": advice.choice_set_digest,
                "evaluation_digest": advice.evaluation_digest,
                "verification_digest": advice.verification_digest,
                "response_payload_sha256": advice.response_payload_sha256,
                "advice_digest": advice.advice_digest,
            });
            metadata
        });
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
        if let Some(metadata) = trial_uncertainty {
            receipt["current_advice"]["trial_uncertainty"] = metadata;
        }
    }
    receipt
}

pub(crate) async fn dispatch(
    context: &RequestContext,
    invocation: MatrixAdvisoryInvocation,
    service: &tect_application::WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let output = match invocation {
        MatrixAdvisoryInvocation::Request(request) => {
            let opportunity = guarded_request(&request, capacity, || {
                service.request_engineering_advisory(context, &request)
            })
            .await?;
            receipt(opportunity)
        }
        MatrixAdvisoryInvocation::Get {
            task_id,
            request_key,
        } => read(
            service
                .get_engineering_advisory(context, task_id, &request_key)
                .await?,
        ),
    };
    let response = crate::responses::with_actions(output, Vec::new(), None);
    if crate::responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(response)
}

#[cfg(test)]
#[path = "matrix_advisory_tools/capacity_tests.rs"]
mod capacity_tests;
#[cfg(test)]
#[path = "matrix_advisory_tools/tests.rs"]
mod tests;
