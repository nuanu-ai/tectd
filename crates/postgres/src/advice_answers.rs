//! Jev's answers about candidates, kept with the journal row of the call that gave them.
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
}
