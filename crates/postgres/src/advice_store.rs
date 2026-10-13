use crate::advice_words::Inputs;
use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use serde::de::DeserializeOwned;
use tect_application::AdviceStore;
use tect_domain::*;
use uuid::Uuid;

const CONFIG_COLUMNS: &str = "enabled, daily_call_limit, revision, \
     pg_catalog.to_char(updated_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"'), \
     updated_by_session";
const ENTRY_COLUMNS: &str = "id, \
     pg_catalog.to_char(created_at AT TIME ZONE 'UTC', 'YYYY-MM-DD\"T\"HH24:MI:SS.US\"Z\"') \
         AS created_at, \
     workspace_id, session_id, point, route, request_id, program_id, candidate_set_id, \
     scope_id, slice_id, phase, called, outcome, reason, send_certainty, model, request_bytes, \
     response_bytes, input_tokens, output_tokens, latency_ms, advice";
const BODIES: &str = "request_body, response_body";
/// A page read without bodies does not load them.
const NO_BODIES: &str = "NULL::bytea AS request_body, NULL::bytea AS response_body";
/// The point and time filters shared by the journal page and its counters.
const WINDOW: &str = "tenant_id=$1 AND workspace_id=$2 \
     AND ($3::text IS NULL OR point=$3) \
     AND ($4::timestamptz IS NULL OR created_at >= $4::timestamptz) \
     AND ($5::timestamptz IS NULL OR created_at < $5::timestamptz)";

type ConfigRow = (bool, Option<i64>, i64, String, Uuid);

fn config(row: ConfigRow) -> AdviceConfig {
    AdviceConfig {
        enabled: row.0,
        daily_call_limit: row.1,
        revision: row.2,
        updated_at: row.3,
        updated_by_session: row.4,
    }
}

#[derive(sqlx::FromRow)]
struct EntryRow {
    id: Uuid,
    created_at: String,
    workspace_id: Uuid,
    session_id: Uuid,
    point: String,
    route: String,
    request_id: Option<Uuid>,
    program_id: Option<Uuid>,
    candidate_set_id: Option<Uuid>,
    scope_id: Option<Uuid>,
    slice_id: Option<Uuid>,
    phase: Option<String>,
    called: bool,
    outcome: Option<String>,
    reason: Option<String>,
    send_certainty: String,
    model: Option<String>,
    request_bytes: Option<i64>,
    response_bytes: Option<i64>,
    input_tokens: Option<i64>,
    output_tokens: Option<i64>,
    latency_ms: Option<i64>,
    request_body: Option<Vec<u8>>,
    response_body: Option<Vec<u8>>,
    advice: Option<serde_json::Value>,
}

fn named<T: DeserializeOwned>(name: String) -> Result<T> {
    serde_json::from_value(serde_json::Value::String(name)).map_err(storage_error)
}

fn text(body: Option<Vec<u8>>) -> Option<String> {
    body.map(|bytes| String::from_utf8_lossy(&bytes).into_owned())
}

fn size(body: Option<&Vec<u8>>) -> Result<Option<i64>> {
    body.map(|bytes| i64::try_from(bytes.len()).map_err(storage_error))
        .transpose()
}

fn entry(row: EntryRow) -> Result<AdviceJournalEntry> {
    Ok(AdviceJournalEntry {
        id: row.id,
        created_at: row.created_at,
        workspace_id: row.workspace_id,
        session_id: row.session_id,
        point: row.point,
        route: row.route,
        work: AdviceWorkRefs {
            request_id: row.request_id,
            program_id: row.program_id,
            candidate_set_id: row.candidate_set_id,
            scope_id: row.scope_id,
            slice_id: row.slice_id,
            phase: row.phase,
        },
        called: row.called,
        outcome: row.outcome.map(named).transpose()?,
        reason: row.reason.map(named).transpose()?,
        send_certainty: named(row.send_certainty)?,
        model: row.model,
        request_bytes: row.request_bytes,
        response_bytes: row.response_bytes,
        input_tokens: row.input_tokens,
        output_tokens: row.output_tokens,
        latency_ms: row.latency_ms,
        request_body: text(row.request_body),
        response_body: text(row.response_body),
        advice: row
            .advice
            .map(serde_json::from_value)
            .transpose()
            .map_err(storage_error)?,
    })
}

fn stored(advice: &AdviceField) -> Result<serde_json::Value> {
    serde_json::to_value(advice).map_err(storage_error)
}

#[async_trait]
impl AdviceStore for PgUnitOfWork {
    async fn advice_config(
        &mut self,
        workspace_id: Uuid,
        for_update: bool,
    ) -> Result<Option<AdviceConfig>> {
        let tenant_id = self.tenant_id()?;
        let lock = if for_update { " FOR UPDATE" } else { "" };
        let row: Option<ConfigRow> = sqlx::query_as(&format!(
            "SELECT {CONFIG_COLUMNS} FROM advice_config \
             WHERE tenant_id=$1 AND workspace_id=$2{lock}"
        ))
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(row.map(config))
    }

    async fn configure_advice(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        request: &ConfigureAdvice,
    ) -> Result<AdviceConfig> {
        let tenant_id = self.tenant_id()?;
        // Revision 0 means "no setting yet": only an insert may satisfy it. An omitted
        // limit keeps the stored one, which only a disabling request may omit.
        let row: Option<ConfigRow> = if request.expected_revision == 0 {
            sqlx::query_as(&format!(
                "INSERT INTO advice_config \
                 (tenant_id, workspace_id, enabled, daily_call_limit, revision, updated_by_session) \
                 VALUES ($1, $2, $3, $4, 1, $5) \
                 ON CONFLICT (tenant_id, workspace_id) DO NOTHING \
                 RETURNING {CONFIG_COLUMNS}"
            ))
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.enabled)
            .bind(request.daily_call_limit)
            .bind(session_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
        } else {
            sqlx::query_as(&format!(
                "UPDATE advice_config SET enabled=$3, \
                     daily_call_limit=COALESCE($4, daily_call_limit), revision=revision+1, \
                     updated_at=pg_catalog.clock_timestamp(), updated_by_session=$5 \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND revision=$6 \
                 RETURNING {CONFIG_COLUMNS}"
            ))
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.enabled)
            .bind(request.daily_call_limit)
            .bind(session_id)
            .bind(request.expected_revision)
            .fetch_optional(&mut **self.transaction()?)
            .await
        }
        .map_err(storage_error)?;
        row.map(config).ok_or(Error::StaleRevision)
    }

    async fn advice_calls_today(&mut self, workspace_id: Uuid) -> Result<i64> {
        let tenant_id = self.tenant_id()?;
        sqlx::query_scalar(
            "SELECT pg_catalog.count(*) FROM advice_journal \
             WHERE tenant_id=$1 AND workspace_id=$2 AND called \
               AND created_at >= pg_catalog.date_trunc('day', pg_catalog.now(), 'UTC')",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)
    }

    async fn insert_advice_entry(
        &mut self,
        workspace_id: Uuid,
        session_id: Uuid,
        entry: &NewAdviceEntry,
    ) -> Result<Uuid> {
        let tenant_id = self.tenant_id()?;
        let skipped = entry.not_called.as_ref();
        let certainty = if skipped.is_some() {
            AdviceSendCertainty::NotSent
        } else {
            AdviceSendCertainty::SentUnknown
        };
        sqlx::query_scalar(
            "INSERT INTO advice_journal \
             (tenant_id, workspace_id, session_id, point, route, request_id, program_id, \
              candidate_set_id, scope_id, slice_id, phase, called, outcome, reason, \
              send_certainty, advice) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) RETURNING id",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(session_id)
        .bind(&entry.point)
        .bind(&entry.route)
        .bind(entry.work.request_id)
        .bind(entry.work.program_id)
        .bind(entry.work.candidate_set_id)
        .bind(entry.work.scope_id)
        .bind(entry.work.slice_id)
        .bind(&entry.work.phase)
        .bind(skipped.is_none())
        .bind(skipped.map(|advice| advice.outcome().as_str()))
        .bind(
            skipped
                .and_then(AdviceField::reason)
                .map(|reason| reason.as_str()),
        )
        .bind(certainty.as_str())
        .bind(skipped.map(stored).transpose()?)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)
    }

    async fn complete_advice_entry(
        &mut self,
        workspace_id: Uuid,
        entry_id: Uuid,
        completion: &AdviceCompletion,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        let exchange = &completion.exchange;
        let result = sqlx::query(
            "UPDATE advice_journal SET called=$4, outcome=$5, reason=$6, send_certainty=$7, \
                 model=$8, request_bytes=$9, response_bytes=$10, input_tokens=$11, \
                 output_tokens=$12, latency_ms=$13, request_body=$14, response_body=$15, \
                 advice=$16 \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND outcome IS NULL",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(entry_id)
        .bind(completion.send_certainty != AdviceSendCertainty::NotSent)
        .bind(completion.advice.outcome().as_str())
        .bind(completion.advice.reason().map(|reason| reason.as_str()))
        .bind(completion.send_certainty.as_str())
        .bind(&exchange.model)
        .bind(size(exchange.request_body.as_ref())?)
        .bind(size(exchange.response_body.as_ref())?)
        .bind(exchange.input_tokens)
        .bind(exchange.output_tokens)
        .bind(exchange.latency_ms)
        .bind(&exchange.request_body)
        .bind(&exchange.response_body)
        .bind(stored(&completion.advice)?)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(Error::InternalInvariant);
        }
        self.keep_candidate_advice(workspace_id, entry_id, &completion.kept)
            .await
    }

    async fn advice_entry_for_request(
        &mut self,
        workspace_id: Uuid,
        route: &str,
        point: Option<&str>,
        request_id: Uuid,
    ) -> Result<Option<AdviceJournalEntry>> {
        let tenant_id = self.tenant_id()?;
        let row: Option<EntryRow> = sqlx::query_as(&format!(
            "SELECT {ENTRY_COLUMNS}, {NO_BODIES} FROM advice_journal \
             WHERE tenant_id=$1 AND workspace_id=$2 AND route=$3 AND request_id=$4 \
               AND ($5::text IS NULL OR point=$5) \
             ORDER BY created_at DESC, id DESC LIMIT 1"
        ))
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(route)
        .bind(request_id)
        .bind(point)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(entry).transpose()
    }

    async fn advice_entries(
        &mut self,
        workspace_id: Uuid,
        query: &AdviceAuditQuery,
        limit: u32,
    ) -> Result<Vec<AdviceJournalEntry>> {
        let tenant_id = self.tenant_id()?;
        if let Some(after) = query.after {
            // An unknown cursor is a caller error, never an empty last page.
            let known: bool = sqlx::query_scalar(
                "SELECT EXISTS(SELECT 1 FROM advice_journal \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(after)
            .fetch_one(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if !known {
                return Err(Error::InvalidArguments);
            }
        }
        let bodies = if query.bodies { BODIES } else { NO_BODIES };
        let rows: Vec<EntryRow> = sqlx::query_as(&format!(
            "SELECT {ENTRY_COLUMNS}, {bodies} FROM advice_journal WHERE {WINDOW} \
               AND ($6::text IS NULL OR outcome=$6) \
               AND ($7::uuid IS NULL OR (created_at, id) < ( \
                   SELECT pivot.created_at, pivot.id FROM advice_journal pivot \
                   WHERE pivot.tenant_id=$1 AND pivot.workspace_id=$2 AND pivot.id=$7)) \
             ORDER BY created_at DESC, id DESC LIMIT $8"
        ))
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(&query.point)
        .bind(&query.since)
        .bind(&query.until)
        .bind(query.outcome.map(AdviceOutcome::as_str))
        .bind(query.after)
        .bind(i64::from(limit))
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        rows.into_iter().map(entry).collect()
    }

    async fn advice_counters(
        &mut self,
        workspace_id: Uuid,
        query: &AdviceAuditQuery,
    ) -> Result<AdviceCounters> {
        let tenant_id = self.tenant_id()?;
        let rows: Vec<(bool, Option<String>, Option<String>, i64)> = sqlx::query_as(&format!(
            "SELECT called, outcome, reason, pg_catalog.count(*) FROM advice_journal \
             WHERE {WINDOW} GROUP BY called, outcome, reason"
        ))
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(&query.point)
        .bind(&query.since)
        .bind(&query.until)
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let mut counters = AdviceCounters::default();
        for (called, outcome, reason, count) in rows {
            counters.add(
                called,
                outcome.map(named).transpose()?,
                reason.map(named).transpose()?,
                count,
            );
        }
        Ok(counters)
    }

    async fn program_mode(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
    ) -> Result<Option<ProgramMode>> {
        self.read_program_mode(workspace_id, program_id).await
    }

    async fn program_input_texts(
        &mut self,
        workspace_id: Uuid,
        program_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        self.read_input_texts(workspace_id, Inputs::Program, program_id, through)
            .await
    }

    async fn scope_candidate_input_texts(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        let inputs = Inputs::ScopeCandidateSet;
        self.read_input_texts(workspace_id, inputs, candidate_set_id, through)
            .await
    }

    async fn slice_candidate_input_texts(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        let inputs = Inputs::SliceCandidateSet;
        self.read_input_texts(workspace_id, inputs, candidate_set_id, through)
            .await
    }

    async fn scope_candidate_words_basis(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        snapshot_id: Uuid,
    ) -> Result<Option<SizingWordsBasis>> {
        self.read_words_basis(workspace_id, candidate_set_id, snapshot_id)
            .await
    }
}
