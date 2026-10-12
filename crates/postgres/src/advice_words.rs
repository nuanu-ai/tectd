//! Read-only views of the user's recorded words for an advice point: whole input
//! texts in recorded order. Nothing here writes.
use crate::{storage_error, store::PgUnitOfWork};
use tect_domain::{Result, SizingWordsBasis};
use uuid::Uuid;

/// The three input tables share one shape: an owner column, a sequence and the text.
#[derive(Clone, Copy)]
pub(crate) enum Inputs {
    Program,
    ScopeCandidateSet,
    /// Only rows a user recorded; rows a Slice result produced carry `source_result_id`.
    SliceCandidateSet,
}

impl Inputs {
    const fn select(self) -> &'static str {
        match self {
            Self::Program => {
                "SELECT input FROM program_inputs \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND program_id=$3 AND sequence<=$4 \
                 ORDER BY sequence"
            }
            Self::ScopeCandidateSet => {
                "SELECT input FROM scope_candidate_inputs \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND sequence<=$4 \
                 ORDER BY sequence"
            }
            Self::SliceCandidateSet => {
                "SELECT input FROM slice_planning_inputs \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND sequence<=$4 \
                   AND source_result_id IS NULL \
                 ORDER BY sequence"
            }
        }
    }
}

impl PgUnitOfWork {
    /// Whole input texts of one owner in recorded order, up to and including `through`.
    pub(crate) async fn read_input_texts(
        &mut self,
        workspace_id: Uuid,
        inputs: Inputs,
        owner_id: Uuid,
        through: i64,
    ) -> Result<Vec<String>> {
        let tenant_id = self.tenant_id()?;
        sqlx::query_scalar(inputs.select())
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(owner_id)
            .bind(through)
            .fetch_all(&mut **self.transaction()?)
            .await
            .map_err(storage_error)
    }

    /// The Program of a Scope candidate set and the input cursors of one of its snapshots.
    pub(crate) async fn read_words_basis(
        &mut self,
        workspace_id: Uuid,
        candidate_set_id: Uuid,
        snapshot_id: Uuid,
    ) -> Result<Option<SizingWordsBasis>> {
        let tenant_id = self.tenant_id()?;
        let row: Option<(Uuid, i64, i64)> = sqlx::query_as(
            "SELECT s.program_id, p.program_latest_input, p.planning_latest_input \
             FROM scope_candidate_sets s \
             JOIN scope_candidate_snapshots p ON p.tenant_id=s.tenant_id \
              AND p.workspace_id=s.workspace_id AND p.candidate_set_id=s.id \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 AND p.id=$4",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(candidate_set_id)
        .bind(snapshot_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        Ok(row.map(
            |(program_id, program_latest_input, planning_latest_input)| SizingWordsBasis {
                program_id,
                program_latest_input,
                planning_latest_input,
            },
        ))
    }
}
