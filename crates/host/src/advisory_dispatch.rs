use crate::advisory_tools::AdvisoryInvocation;
use crate::{Result, responses};
use tect_application::WorkspaceService;
use tect_domain::RequestContext;

pub(crate) async fn execute(
    context: &RequestContext,
    invocation: AdvisoryInvocation,
    service: &WorkspaceService,
    capacity: usize,
) -> Result<serde_json::Value> {
    let value = match invocation {
        AdvisoryInvocation::VerifySelectedSave(request) => {
            let observation = service.verify_selected_save(context, &request).await?;
            serde_json::to_value(serde_json::json!({
                "observation": observation,
                "establishes_independent_approval": false,
                "establishes_current_acceptance": false,
            }))
        }
        AdvisoryInvocation::ScopeRequest(request) => {
            let outcome = service.run_scope_advisory(context, &request).await?;
            serde_json::to_value(serde_json::json!({
                "request_id": request.request_id,
                "opportunity_id": outcome.opportunity.id,
                "candidate_set_id": request.candidate_set_id,
                "state": outcome.opportunity.state,
                "reason": outcome.opportunity.primary_reason,
                "provider_called": outcome.opportunity.provider_called,
                "advice_id": outcome.advice.as_ref().map(|advice| &advice.id),
            }))
        }
        AdvisoryInvocation::ScopeDisposition {
            opportunity_id,
            candidate_set_id,
            request,
        } => serde_json::to_value(
            service
                .decide_scope_advisory(context, opportunity_id, candidate_set_id, request)
                .await?,
        ),
        AdvisoryInvocation::Config => serde_json::to_value(service.advisory_config(context).await?),
        AdvisoryInvocation::Configure(request) => {
            serde_json::to_value(service.configure_advisory(context, &request).await?)
        }
        AdvisoryInvocation::WorkspaceAudit(query) => {
            serde_json::to_value(service.advisory_audit(context, &query).await?)
        }
        AdvisoryInvocation::ScopeAudit { scope_id, query } => serde_json::to_value(
            service
                .scope_advisory_audit(context, scope_id, &query)
                .await?,
        ),
        AdvisoryInvocation::ScopeGet {
            scope_id,
            opportunity_id,
        } => serde_json::to_value(
            service
                .scope_advisory_get(context, scope_id, opportunity_id)
                .await?,
        ),
        AdvisoryInvocation::CandidateAudit {
            candidate_set_id,
            query,
        } => serde_json::to_value(
            service
                .candidate_advisory_audit(context, candidate_set_id, &query)
                .await?,
        ),
        AdvisoryInvocation::CandidateGet {
            candidate_set_id,
            opportunity_id,
        } => serde_json::to_value(
            service
                .candidate_advisory_get(context, candidate_set_id, opportunity_id)
                .await?,
        ),
    }
    .map_err(tect_domain::Error::invalid_arguments_from)?;
    let value = responses::with_actions(value, Vec::new(), None);
    if responses::encoded_len(&value)? > capacity {
        return Err(tect_domain::Error::RequestTooLarge);
    }
    Ok(value)
}
