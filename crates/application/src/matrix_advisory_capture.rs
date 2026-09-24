use crate::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixProviderRequest, MatrixTaskRevision, PreparedMatrixAdviceAttempt,
};
use tect_domain::{
    AdvisoryOpportunityInput, AdvisoryOpportunityState, AdvisoryReason, Error,
    WorkspaceAdvisoryConfig,
};
use uuid::Uuid;

/// Captured preparation is retained for the later, transaction-bound dispatch
/// flow. NoCall carries no provider body or budget grant.
pub(crate) enum PreparedMatrixOpportunity {
    Authorized {
        prepared: PreparedMatrixAdviceAttempt,
        authorization: MatrixBudgetAuthorization,
    },
    NoCall,
}

/// Pure preparation and budget decision for an eligible, enabled Matrix task.
/// This boundary captures Prepared/DispatchAuthorized only. A later packet must
/// persist a dispatch authorization and committed dispatch start before minting
/// a send permit; neither provider transport nor dispatch starts here.
pub(crate) async fn prepare_eligible_matrix_opportunity(
    input: &mut AdvisoryOpportunityInput,
    revision: &MatrixTaskRevision,
    config: &WorkspaceAdvisoryConfig,
    workspace_id: Uuid,
    actor_id: Uuid,
    provider: &dyn MatrixAdviceProvider,
    budget: &dyn MatrixBudgetPolicy,
) -> tect_domain::Result<PreparedMatrixOpportunity> {
    if input.primary_reason != AdvisoryReason::CapabilityUnavailable {
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    // Positive capture cannot persist until an exact, validated Matrix
    // verification has been attached to this opportunity.
    if input.matrix_verification_digest.is_none() {
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    let composition =
        super::matrix_tasks::compose_current_revision(revision.clone(), revision.revision)?;
    if !composition.unresolved_evidence.is_empty() {
        input.primary_reason = AdvisoryReason::MatrixEvidenceUnresolved;
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    if !composition.is_resolved() {
        input.primary_reason = AdvisoryReason::MatrixSourceUnverified;
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    let (Some(profile), Some(model)) = (
        config.provider_profile_ref.as_ref(),
        config.model_configuration.as_ref(),
    ) else {
        input.primary_reason = AdvisoryReason::ProviderUnconfigured;
        return Ok(PreparedMatrixOpportunity::NoCall);
    };
    let Some(identity) = provider.identity() else {
        return Ok(PreparedMatrixOpportunity::NoCall);
    };
    if identity.provider_profile_ref != *profile || identity.model_configuration != *model {
        input.primary_reason = AdvisoryReason::ProviderUnconfigured;
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    let request = match MatrixProviderRequest::new(
        revision.clone(),
        composition,
        profile.clone(),
        model.clone(),
    ) {
        Ok(request) => request,
        Err(_) => return Ok(PreparedMatrixOpportunity::NoCall),
    };
    let prepared = match provider.prepare(&request) {
        Ok(prepared)
            if prepared.validate_for(&request).is_ok() && prepared.identity() == &identity =>
        {
            prepared
        }
        _ => return Ok(PreparedMatrixOpportunity::NoCall),
    };
    let budget_request = MatrixBudgetRequest::from_prepared(workspace_id, actor_id, &prepared)?;
    let authorization = budget.authorize(&budget_request).await;
    let result = match authorization {
        Ok(Some(auth)) if valid_policy_id(&auth.policy_id) => {
            // Positive opportunities bind the exact Matrix evaluation. The
            // legacy no-call digest remains unchanged for existing receipts.
            input.material_digest = request.binding().evaluation_digest.clone();
            input.state = AdvisoryOpportunityState::Prepared;
            input.primary_reason = AdvisoryReason::DispatchAuthorized;
            PreparedMatrixOpportunity::Authorized {
                prepared,
                authorization: auth,
            }
        }
        Ok(None) | Err(_) | Ok(Some(_)) => {
            input.primary_reason = AdvisoryReason::BudgetPolicyInvalid;
            PreparedMatrixOpportunity::NoCall
        }
    };
    input.validate().map_err(|_| Error::InternalInvariant)?;
    Ok(result)
}

fn valid_policy_id(value: &str) -> bool {
    !value.is_empty() && value.len() <= 256 && !value.contains('\0') && value.trim() == value
}

#[cfg(test)]
mod tests {
    use super::valid_policy_id;

    #[test]
    fn budget_policy_identity_is_nonempty_and_bounded() {
        assert!(valid_policy_id("configured-policy-v1"));
        assert!(!valid_policy_id(""));
        assert!(!valid_policy_id(" policy"));
        assert!(!valid_policy_id("policy\0"));
        assert!(!valid_policy_id(&"p".repeat(257)));
    }
}
