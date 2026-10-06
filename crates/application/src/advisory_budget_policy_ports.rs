use async_trait::async_trait;
use tect_domain::{AdvisoryBudgetPolicy, Result};
use uuid::Uuid;

/// Policy lookup and installation are separate from dispatch reservation.
#[async_trait]
pub trait AdvisoryBudgetPolicyStore: Send {
    /// Untrusted stored candidate. Presence alone never permits a send: the
    /// caller must verify the owner's signature against a trusted owner key.
    async fn candidate_budget_policy(
        &mut self,
        workspace_id: Uuid,
        now_unix_ms: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>>;

    /// The dispatch integration must override this only after trusted approval
    /// verification. This default keeps newly installed rows inert.
    async fn authorized_budget_policy(
        &mut self,
        _workspace_id: Uuid,
        _now_unix_ms: i64,
    ) -> Result<Option<AdvisoryBudgetPolicy>> {
        Ok(None)
    }

    /// Append an owner-submitted candidate. Existing versions cannot be changed.
    async fn install_budget_policy(
        &mut self,
        workspace_id: Uuid,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<()>;
}

#[cfg(test)]
mod tests {
    use super::*;
    struct Empty;
    #[async_trait]
    impl AdvisoryBudgetPolicyStore for Empty {
        async fn candidate_budget_policy(
            &mut self,
            _: Uuid,
            _: i64,
        ) -> Result<Option<AdvisoryBudgetPolicy>> {
            Ok(None)
        }
        async fn install_budget_policy(&mut self, _: Uuid, _: &AdvisoryBudgetPolicy) -> Result<()> {
            Ok(())
        }
    }
    #[tokio::test]
    async fn no_approval_verifier_means_no_authorized_policy() {
        assert!(
            Empty
                .authorized_budget_policy(Uuid::new_v4(), 123)
                .await
                .unwrap()
                .is_none()
        );
    }
}
