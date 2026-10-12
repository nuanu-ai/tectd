use async_trait::async_trait;
use tect_domain::*;
use uuid::Uuid;

/// The outside adviser. It knows nothing of Programs, Scopes, Slices or pipelines.
#[async_trait]
pub trait AdviceProvider: Send + Sync {
    /// Whether the daemon was started with a provider.
    fn configured(&self) -> bool;
    /// Ask one question set once. Implementations never retry.
    async fn ask(
        &self,
        questions: &AdviceQuestionSet,
    ) -> std::result::Result<AdviceReply, AdviceFailure>;
}

pub(crate) const NO_PROVIDER: &str = "the daemon has no Jev provider configured";

#[derive(Debug, Default)]
pub struct DisabledAdviceProvider;

#[async_trait]
impl AdviceProvider for DisabledAdviceProvider {
    fn configured(&self) -> bool {
        false
    }

    async fn ask(&self, _: &AdviceQuestionSet) -> std::result::Result<AdviceReply, AdviceFailure> {
        Err(AdviceFailure::not_called(
            AdviceUnavailableReason::ProviderUnconfigured,
            NO_PROVIDER,
        ))
    }
}

#[async_trait]
pub trait AdviceStore: Send {
    /// `for_update` locks the setting row so a limit check and a journal insert
    /// form one decision.
    async fn advice_config(
        &mut self,
        workspace_id: Uuid,
        for_update: bool,
    ) -> Result<Option<AdviceConfig>>;

    /// Compare-and-swap on the revision; a mismatch is `StaleRevision`.
    async fn configure_advice(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &ConfigureAdvice,
    ) -> Result<AdviceConfig>;

    /// Journal rows of the current UTC day whose request went out.
    async fn advice_calls_today(&mut self, workspace_id: Uuid) -> Result<i64>;

    async fn insert_advice_entry(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        entry: &NewAdviceEntry,
    ) -> Result<Uuid>;

    async fn complete_advice_entry(
        &mut self,
        workspace_id: Uuid,
        entry_id: Uuid,
        completion: &AdviceCompletion,
    ) -> Result<()>;

    /// Newest row a command wrote under one idempotency request id.
    async fn advice_entry_for_request(
        &mut self,
        workspace_id: Uuid,
        route: &str,
        point: Option<&str>,
        request_id: Uuid,
    ) -> Result<Option<AdviceJournalEntry>>;

    /// Newest first; `limit` may be one more than the page for look-ahead.
    async fn advice_entries(
        &mut self,
        workspace_id: Uuid,
        query: &AdviceAuditQuery,
        limit: u32,
    ) -> Result<Vec<AdviceJournalEntry>>;

    async fn advice_counters(
        &mut self,
        workspace_id: Uuid,
        query: &AdviceAuditQuery,
    ) -> Result<AdviceCounters>;
}
