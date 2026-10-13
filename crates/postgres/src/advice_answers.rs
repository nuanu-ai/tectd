//! Jev's answers about candidates, kept with the journal row of the call that gave
//! them, and the departures reviews state from them.
use crate::{storage_error, store::PgUnitOfWork};
use tect_domain::*;
use uuid::Uuid;

impl PgUnitOfWork {
    /// Keep Jev's answer about each candidate of one completed call. The point and
    /// the candidate set are the journal row's own.
    pub(crate) async fn keep_candidate_advice(
        &mut self,
        workspace_id: Uuid,
        entry_id: Uuid,
        kept: &[CandidateAdvice],
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        for answer in kept {
            let result = sqlx::query(
                "INSERT INTO advice_candidate_answers \
                 (tenant_id, workspace_id, journal_id, point, candidate_set_id, level, \
                  candidate_id, candidate_revision, asks_change, answer) \
                 SELECT tenant_id, workspace_id, id, point, candidate_set_id, $4, $5, $6, $7, $8 \
                 FROM advice_journal WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(entry_id)
            .bind(answer.level.as_str())
            .bind(answer.candidate_id)
            .bind(answer.candidate_revision)
            .bind(answer.asks_change)
            .bind(&answer.answer)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if result.rows_affected() != 1 {
                return Err(Error::InternalInvariant);
            }
        }
        Ok(())
    }

    /// The kept answers of one candidate set that ask for a change and have no
    /// departure, in the order they were given.
    pub(crate) async fn read_open_advice(
        &mut self,
        workspace_id: Uuid,
        level: SizingLevel,
        candidate_set_id: Uuid,
    ) -> Result<Vec<OpenAdvice>> {
        let tenant_id = self.tenant_id()?;
        let rows: Vec<(Uuid, Uuid, i64, String, serde_json::Value)> = sqlx::query_as(
            "SELECT id, candidate_id, candidate_revision, point, answer \
             FROM advice_candidate_answers \
             WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND level=$4 \
               AND asks_change AND departed_at IS NULL \
             ORDER BY created_at, id",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(level.as_str())
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        rows.into_iter()
            .map(|(id, candidate_id, candidate_revision, point, answer)| {
                Ok(OpenAdvice {
                    id,
                    candidate_id,
                    candidate_revision,
                    point: serde_json::from_value(serde_json::Value::String(point))
                        .map_err(storage_error)?,
                    answer,
                })
            })
            .collect()
    }

    /// Write a departure on one kept answer that is still open.
    pub(crate) async fn write_advice_departure(
        &mut self,
        workspace_id: Uuid,
        answer_id: Uuid,
        reason: &str,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        let result = sqlx::query(
            "UPDATE advice_candidate_answers \
             SET departure_reason=$4, departed_at=pg_catalog.clock_timestamp() \
             WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 \
               AND asks_change AND departed_at IS NULL",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(answer_id)
        .bind(reason)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if result.rows_affected() != 1 {
            return Err(Error::InternalInvariant);
        }
        Ok(())
    }
}
