use super::*;

pub(super) async fn evaluate_verified_scope_budget(
    evaluator: &dyn crate::ScopeBudgetPolicy,
    request: &ScopeBudgetRequest,
    verified_policy: Option<&tect_domain::AdvisoryBudgetPolicy>,
) -> Result<Option<crate::ScopeBudgetPolicyEvaluation>> {
    let Some(verified_policy) = verified_policy else {
        return Ok(None);
    };
    let evaluation = evaluator.evaluate(request, verified_policy).await?;
    Ok(evaluation.filter(|result| {
        result.policy_id == verified_policy.id().to_string()
            && result.policy_version == verified_policy.version()
            && result.policy_digest == verified_policy.digest()
    }))
}

pub(super) async fn lookup_verified_scope_budget(
    store: Option<&mut dyn crate::AdvisoryBudgetPolicyStore>,
    workspace_id: Uuid,
    now_unix_ms: i64,
) -> Result<Option<tect_domain::AdvisoryBudgetPolicy>> {
    let Some(store) = store else {
        return Ok(None);
    };
    let policy = match store
        .authorized_budget_policy(workspace_id, now_unix_ms)
        .await
    {
        Ok(policy) => policy,
        Err(Error::InvalidConfiguration) => None,
        Err(error) => return Err(error),
    };
    Ok(policy.filter(|policy| policy.validate().is_ok() && policy.is_effective_at(now_unix_ms)))
}
