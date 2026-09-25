use crate::{
    MatrixAdviceProvider, MatrixBudgetAuthorization, MatrixBudgetPolicy, MatrixBudgetRequest,
    MatrixProviderRequest, PreparedMatrixAdviceAttempt,
};
use tect_domain::{
    AdvisoryBudgetPolicy, AdvisoryOpportunityInput, AdvisoryOpportunityState, AdvisoryReason, Error,
};
use uuid::Uuid;

/// Captured preparation is retained for the later, transaction-bound dispatch
/// flow. NoCall carries no provider body or budget grant.
pub(crate) enum PreparedMatrixOpportunity {
    Authorized {
        prepared: Box<PreparedMatrixAdviceAttempt>,
        authorization: MatrixBudgetAuthorization,
    },
    NoCall,
}

/// Pure preparation and budget decision for a currently verified Matrix task.
pub(crate) async fn prepare_eligible_matrix_opportunity(
    input: &mut AdvisoryOpportunityInput,
    request: Option<&MatrixProviderRequest>,
    workspace_id: Uuid,
    actor_id: Uuid,
    provider: &dyn MatrixAdviceProvider,
    budget: &dyn MatrixBudgetPolicy,
    verified_policy: Option<&AdvisoryBudgetPolicy>,
) -> tect_domain::Result<PreparedMatrixOpportunity> {
    if input.primary_reason != AdvisoryReason::CapabilityUnavailable {
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    let Some(request) = request else {
        return Ok(PreparedMatrixOpportunity::NoCall);
    };
    if request.binding().verification_digest.is_none()
        || input.matrix_task_revision != Some(request.revision().revision)
        || input.matrix_choice_set_digest.as_deref()
            != Some(request.binding().choice_set_digest.as_str())
        || !request.composition().is_resolved()
    {
        return Err(Error::InternalInvariant);
    }
    let Some(identity) = provider.identity() else {
        input.primary_reason = AdvisoryReason::ProviderUnconfigured;
        return Ok(PreparedMatrixOpportunity::NoCall);
    };
    if identity.provider_profile_ref != *request.provider_profile_ref()
        || identity.model_configuration != *request.model_configuration()
    {
        input.primary_reason = AdvisoryReason::ProviderUnconfigured;
        return Ok(PreparedMatrixOpportunity::NoCall);
    }
    let prepared = match provider.prepare(request) {
        Ok(prepared)
            if prepared.validate_for(request).is_ok() && prepared.identity() == &identity =>
        {
            prepared
        }
        _ => return Ok(PreparedMatrixOpportunity::NoCall),
    };
    let budget_request = MatrixBudgetRequest::from_prepared(workspace_id, actor_id, &prepared)?;
    let authorization = match verified_policy {
        Some(policy) => budget.authorize(&budget_request, policy).await,
        None => Ok(None),
    };
    let result = match authorization {
        Ok(Some(auth))
            if verified_policy.is_some_and(|policy| {
                valid_policy_id(&auth.policy_id) && auth.policy_id == policy.id().to_string()
            }) =>
        {
            // Positive opportunities bind the exact Matrix evaluation. The
            // legacy no-call digest remains unchanged for existing receipts.
            input.material_digest = request.binding().evaluation_digest.clone();
            input.matrix_verification_digest = request.binding().verification_digest.clone();
            input.state = AdvisoryOpportunityState::Prepared;
            input.primary_reason = AdvisoryReason::DispatchAuthorized;
            PreparedMatrixOpportunity::Authorized {
                prepared: Box::new(prepared),
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

pub(crate) async fn lookup_verified_matrix_budget(
    store: Option<&mut dyn crate::AdvisoryBudgetPolicyStore>,
    workspace_id: Uuid,
) -> tect_domain::Result<Option<AdvisoryBudgetPolicy>> {
    let Some(store) = store else { return Ok(None) };
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .map_err(|_| Error::BudgetPolicyInvalid)?
            .as_millis(),
    )
    .map_err(|_| Error::BudgetPolicyInvalid)?;
    let policy = match store.authorized_budget_policy(workspace_id, now).await {
        Ok(policy) => policy,
        Err(Error::InvalidConfiguration) => None,
        Err(error) => return Err(error),
    };
    Ok(policy.filter(|policy| policy.validate().is_ok() && policy.is_effective_at(now)))
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
