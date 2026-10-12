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
/// Every request carries the Program's product level; without one Jev is not asked.
pub(crate) const NO_MODE: &str = "the Program has no mode; set it with program.save";
/// A point that sends the Program's statement has nothing to send without its text.
pub(crate) const NO_PROGRAM: &str = "the Program's text is erased or cannot be read";

/// The host's measure of the command reply that an `advice` field will be attached to.
pub trait AdviceOutputGuard: Send + Sync {
    /// `RequestTooLarge` when the reply with this field attached would exceed the
    /// budget the reply already obeys.
    fn check_reply(&self, advice: &AdviceField) -> Result<()>;
}

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

    /// The product level the Program holds now; `None` while it has none.
    async fn program_mode(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
    ) -> Result<Option<ProgramMode>>;

    /// Whole Program input texts in recorded order, up to and including `through`.
    async fn program_input_texts(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>>;

    /// Whole input texts of a Scope candidate set in recorded order, up to and
    /// including `through`.
    async fn scope_candidate_input_texts(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>>;

    /// Whole texts recorded as input of a Slice candidate set, in recorded order, up
    /// to and including `through`. Rows a Slice result produced are not the user's
    /// words and are left out.
    async fn slice_candidate_input_texts(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>>;

    /// The Program of a Scope candidate set and how far one of its snapshots had read.
    async fn scope_candidate_words_basis(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        snapshot_id: Uuid,
    ) -> Result<Option<SizingWordsBasis>>;
}
