//! Stand-ins for the unit tests: an in-memory journal that also serves numbered
//! input texts, a point with a chosen largest field, and a command reply with a
//! chosen room.
use super::super::*;
use crate::AdviceOutputGuard;
use async_trait::async_trait;
use serde_json::json;
#[derive(Default)]
pub(super) struct Journal {
    pub config: Option<AdviceConfig>,
    pub calls_today: i64,
    pub rows: Vec<(Uuid, NewAdviceEntry)>,
    pub locked: bool,
    /// Every read of input texts: which inputs, whose, and through which entry.
    pub reads: Vec<(&'static str, Uuid, i64)>,
}

impl Journal {
    /// The texts of entries 1 through `through`, each naming its inputs and number.
    fn texts(&mut self, inputs: &'static str, owner: Uuid, through: i64) -> Vec<String> {
        self.reads.push((inputs, owner, through));
        (1..=through)
            .map(|entry| format!("{inputs} {entry}"))
            .collect()
    }
}

#[async_trait]
impl AdviceStore for Journal {
    async fn advice_config(&mut self, _: Uuid, for_update: bool) -> Result<Option<AdviceConfig>> {
        self.locked = for_update;
        Ok(self.config.clone())
    }
    async fn configure_advice(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: &ConfigureAdvice,
    ) -> Result<AdviceConfig> {
        unreachable!()
    }
    async fn advice_calls_today(&mut self, _: Uuid) -> Result<i64> {
        Ok(self.calls_today)
    }
    async fn insert_advice_entry(
        &mut self,
        _: Uuid,
        _: Uuid,
        entry: &NewAdviceEntry,
    ) -> Result<Uuid> {
        let id = Uuid::new_v4();
        self.rows.push((id, entry.clone()));
        Ok(id)
    }
    async fn complete_advice_entry(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: &AdviceCompletion,
    ) -> Result<()> {
        unreachable!()
    }
    async fn advice_entry_for_request(
        &mut self,
        workspace_id: Uuid,
        route: &str,
        point: Option<&str>,
        request_id: Uuid,
    ) -> Result<Option<AdviceJournalEntry>> {
        Ok(self
            .rows
            .iter()
            .rev()
            .find(|(_, row)| {
                row.route == route
                    && point.is_none_or(|point| row.point == point)
                    && row.work.request_id == Some(request_id)
            })
            .map(|(id, row)| AdviceJournalEntry {
                id: *id,
                created_at: "2026-10-12T00:00:00.000000Z".into(),
                workspace_id,
                session_id: Uuid::new_v4(),
                point: row.point.clone(),
                route: row.route.clone(),
                work: row.work.clone(),
                called: row.not_called.is_none(),
                outcome: row.not_called.as_ref().map(AdviceField::outcome),
                reason: row.not_called.as_ref().and_then(AdviceField::reason),
                send_certainty: if row.not_called.is_none() {
                    AdviceSendCertainty::SentUnknown
                } else {
                    AdviceSendCertainty::NotSent
                },
                model: None,
                request_bytes: None,
                response_bytes: None,
                input_tokens: None,
                output_tokens: None,
                latency_ms: None,
                request_body: None,
                response_body: None,
                advice: row.not_called.clone(),
            }))
    }
    async fn advice_entries(
        &mut self,
        _: Uuid,
        _: &AdviceAuditQuery,
        _: u32,
    ) -> Result<Vec<AdviceJournalEntry>> {
        unreachable!()
    }
    async fn advice_counters(&mut self, _: Uuid, _: &AdviceAuditQuery) -> Result<AdviceCounters> {
        unreachable!()
    }
    async fn program_mode(&mut self, _: Uuid, _: Uuid) -> Result<Option<ProgramMode>> {
        unreachable!()
    }
    async fn program_input_texts(
        &mut self,
        _: Uuid,
        program_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        Ok(self.texts("program", program_id, through))
    }
    async fn scope_candidate_input_texts(
        &mut self,
        _: Uuid,
        candidate_set_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        Ok(self.texts("scope set", candidate_set_id, through))
    }
    async fn slice_candidate_input_texts(
        &mut self,
        _: Uuid,
        candidate_set_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        Ok(self.texts("slice set", candidate_set_id, through))
    }
    async fn scope_candidate_words_basis(
        &mut self,
        _: Uuid,
        _: Uuid,
        _: Uuid,
    ) -> Result<Option<SizingWordsBasis>> {
        unreachable!()
    }
}

/// A point whose largest field is a note of a chosen length.
pub(super) struct Point {
    pub largest_note_bytes: usize,
}

impl AdviceShaping for Point {
    fn shape(&self, _: &std::collections::BTreeMap<String, AdviceAnswer>) -> AdviceField {
        unreachable!()
    }
    fn largest(&self) -> AdviceField {
        AdviceField::Advised {
            content: [(
                "note".to_owned(),
                json!("n".repeat(self.largest_note_bytes)),
            )]
            .into_iter()
            .collect(),
        }
    }
    fn subject(&self) -> String {
        "this draft (40 candidates, 3 included items)".into()
    }
}

/// A command reply with room for a field of at most this many JSON bytes.
pub(super) struct Room(pub usize);

impl AdviceOutputGuard for Room {
    fn check_reply(&self, advice: &AdviceField) -> Result<()> {
        if json!(advice).to_string().len() <= self.0 {
            Ok(())
        } else {
            Err(Error::RequestTooLarge)
        }
    }
}

pub(super) const SMALL: Point = Point {
    largest_note_bytes: 10,
};
pub(super) const HUGE: Point = Point {
    largest_note_bytes: 10_000,
};
