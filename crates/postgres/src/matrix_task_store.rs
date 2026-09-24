use async_trait::async_trait;
use sqlx::Row;
use tect_application::{
    MATRIX_INPUT_SCHEMA, MatrixTaskRevision, MatrixTaskStore, RecordMatrixTask,
};
use tect_domain::{EngineeringMatrixInput, Error, Result};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

#[async_trait]
impl MatrixTaskStore for PgUnitOfWork {
    async fn record_matrix_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixTask,
        input_digest: &str,
    ) -> Result<MatrixTaskRevision> {
        let tenant_id = self.tenant_id()?;
        if self.principal_id()? != principal_id {
            return Err(Error::Forbidden);
        }
        if request.revision == 1 {
            let inserted = sqlx::query(
                "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) \
                 VALUES ($1,$2,$3,1) ON CONFLICT DO NOTHING",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if inserted.rows_affected() != 1 {
                return Err(Error::StaleRevision);
            }
        } else {
            let current: Option<i64> = sqlx::query_scalar(
                "SELECT current_revision FROM matrix_tasks \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if current != Some(request.expected_current_revision) {
                return Err(Error::StaleRevision);
            }
        }

        let canonical_input = serde_json::to_value(&request.input).map_err(storage_error)?;
        let inserted = sqlx::query(
            "INSERT INTO matrix_task_revisions (tenant_id,workspace_id,task_id,revision,previous_revision,request_id,input_schema,canonical_input,input_digest,recorded_by_principal_id,recorded_by_session_id) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11) ON CONFLICT DO NOTHING",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(request.task_id)
        .bind(request.revision)
        .bind((request.revision > 1).then_some(request.expected_current_revision))
        .bind(request.request_id)
        .bind(MATRIX_INPUT_SCHEMA)
        .bind(canonical_input)
        .bind(input_digest)
        .bind(principal_id)
        .bind(session_id)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if inserted.rows_affected() != 1 {
            return Err(Error::InputConflict);
        }

        if request.revision > 1 {
            let advanced = sqlx::query(
                "UPDATE matrix_tasks SET current_revision=$4 \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND current_revision=$5",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .bind(request.revision)
            .bind(request.expected_current_revision)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
            if advanced.rows_affected() != 1 {
                return Err(Error::StaleRevision);
            }
        }

        Ok(MatrixTaskRevision {
            task_id: request.task_id,
            revision: request.revision,
            request_id: request.request_id,
            input: request.input.clone(),
            input_digest: input_digest.to_owned(),
            recorded_by_principal_id: principal_id,
            recorded_by_session_id: session_id,
        })
    }

    async fn matrix_task(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>> {
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT r.revision,r.request_id,r.input_schema,r.canonical_input,r.input_digest, \
                    r.recorded_by_principal_id,r.recorded_by_session_id \
             FROM matrix_tasks AS t JOIN matrix_task_revisions AS r \
               ON r.tenant_id=t.tenant_id AND r.workspace_id=t.workspace_id \
               AND r.task_id=t.id AND r.revision=t.current_revision \
             WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(|row| {
            let schema: String = row.try_get("input_schema").map_err(storage_error)?;
            if schema != MATRIX_INPUT_SCHEMA {
                return Err(Error::InternalInvariant);
            }
            let canonical_input: serde_json::Value =
                row.try_get("canonical_input").map_err(storage_error)?;
            let input: EngineeringMatrixInput =
                serde_json::from_value(canonical_input).map_err(storage_error)?;
            input.validate().map_err(|_| Error::InternalInvariant)?;
            Ok(MatrixTaskRevision {
                task_id,
                revision: row.try_get("revision").map_err(storage_error)?,
                request_id: row.try_get("request_id").map_err(storage_error)?,
                input,
                input_digest: row.try_get("input_digest").map_err(storage_error)?,
                recorded_by_principal_id: row
                    .try_get("recorded_by_principal_id")
                    .map_err(storage_error)?,
                recorded_by_session_id: row
                    .try_get("recorded_by_session_id")
                    .map_err(storage_error)?,
            })
        })
        .transpose()
    }
}
