use crate::matrix_requirements_context_tools::MatrixRequirementsContextInvocation;
use crate::matrix_task_tools::MatrixTaskInvocation;
use crate::{Result, responses};
use serde_json::Value;
use tect_application::{VerifyMatrixTask, WorkspaceService};
use tect_domain::{Error, RequestContext};

pub(crate) async fn task(
    context: &RequestContext,
    invocation: MatrixTaskInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let output = match invocation {
        MatrixTaskInvocation::TechnicalCompare(request) => {
            match service
                .compare_technical_delivery_mechanisms(context, &request)
                .await?
            {
                tect_application::TechnicalDeliveryMechanismRead::Unavailable => {
                    serde_json::json!({"state":"unavailable"})
                }
                tect_application::TechnicalDeliveryMechanismRead::Compared(comparison) => {
                    serde_json::json!({"state":"compared", "comparison":comparison})
                }
            }
        }
        MatrixTaskInvocation::Record(request) => {
            crate::matrix_task_tools::guard_record_output(&request, capacity)?;
            crate::matrix_task_tools::source(tect_application::MatrixTaskSource {
                revision: service.record_matrix_task(context, &request).await?,
                requirements_binding: None,
            })
        }
        MatrixTaskInvocation::BoundRecord(request, locator) => {
            crate::matrix_task_tools::guard_bound_record_output(&request, &locator, capacity)?;
            crate::matrix_task_tools::source(
                service
                    .record_matrix_task_with_requirements(context, &request, &locator)
                    .await?,
            )
        }
        MatrixTaskInvocation::Get(task_id) => crate::matrix_task_tools::source(
            service.get_matrix_task_source(context, task_id).await?,
        ),
        MatrixTaskInvocation::VerifiedCards(request) => crate::matrix_task_tools::verified_cards(
            service.get_verified_matrix_cards(context, &request).await?,
        ),
    };
    let response = responses::with_actions(output, Vec::new(), None);
    if responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(response)
}

pub(crate) async fn verify(
    context: &RequestContext,
    request: VerifyMatrixTask,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    service
        .authenticate_matrix_verifier_session(context)
        .await?;
    let receipt = crate::matrix_verification_tools::guarded_verify(&request, capacity, || {
        service.verify_matrix_task(context, &request)
    })
    .await?;
    Ok(responses::with_actions(
        crate::matrix_verification_tools::receipt(receipt),
        Vec::new(),
        None,
    ))
}

pub(crate) async fn requirements(
    context: &RequestContext,
    invocation: MatrixRequirementsContextInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<Value> {
    let value = match invocation {
        MatrixRequirementsContextInvocation::Propose(request) => {
            Ok(crate::matrix_requirements_context_tools::proposal_output(
                service
                    .propose_matrix_requirements_context(context, &request)
                    .await?,
            ))
        }
        MatrixRequirementsContextInvocation::Confirm(request) => Ok(
            crate::matrix_requirements_context_tools::confirmation_output(
                service
                    .confirm_matrix_requirements_context(context, &request)
                    .await?,
            ),
        ),
        MatrixRequirementsContextInvocation::Get(locator) => serde_json::to_value(
            service
                .get_effective_matrix_requirements_context(context, &locator)
                .await?,
        ),
    }
    .map_err(Error::invalid_arguments_from)?;
    // As in the donor, context writes occur before this response-capacity check.
    let response = responses::with_actions(value, Vec::new(), None);
    if responses::encoded_len(&response)? > capacity {
        return Err(Error::RequestTooLarge);
    }
    Ok(response)
}
