use async_trait::async_trait;
use sqlx::{Row, postgres::PgRow};
use tect_application::{
    BoundMatrixTaskRecord, MATRIX_INPUT_SCHEMA, MatrixRequirementsLocator,
    MatrixTaskRequirementsBinding, MatrixTaskRevision, MatrixTaskSource, MatrixTaskStore,
    RecordMatrixTask, canonical_matrix_input_digest,
};
use tect_domain::{
    EngineeringChoiceSet, EngineeringMatrixInput, Error, MATRIX_CHOICE_SET_SCHEMA, Result,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

impl PgUnitOfWork {
    async fn matrix_receipt_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>> {
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT task_id,revision,request_id,input_schema,canonical_input,input_digest,choice_set_schema,choice_set,choice_set_digest, \
                    recorded_by_principal_id,recorded_by_session_id \
             FROM matrix_task_revisions \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        row.map(decode_revision).transpose()
    }

    async fn matrix_retry_or_error(
        &mut self,
        workspace_id: Uuid,
        request: &RecordMatrixTask,
        canonical_input: &serde_json::Value,
        input_digest: &str,
        fallback: Error,
    ) -> Result<MatrixTaskRevision> {
        match self
            .matrix_receipt_by_request(workspace_id, request.request_id)
            .await?
        {
            Some(prior) => {
                self.matrix_unbound_replay(
                    workspace_id,
                    prior,
                    request,
                    canonical_input,
                    input_digest,
                )
                .await
            }
            None => Err(fallback),
        }
    }

    async fn matrix_unbound_replay(
        &mut self,
        workspace_id: Uuid,
        prior: MatrixTaskRevision,
        request: &RecordMatrixTask,
        canonical_input: &serde_json::Value,
        input_digest: &str,
    ) -> Result<MatrixTaskRevision> {
        if !same_request(&prior, request, canonical_input, input_digest)? {
            return Err(Error::InputConflict);
        }
        let bound: bool = sqlx::query_scalar(
            "SELECT EXISTS(SELECT 1 FROM matrix_task_requirements_bindings \
             WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4 AND request_id=$5)",
        )
        .bind(self.tenant_id()?)
        .bind(workspace_id)
        .bind(prior.task_id)
        .bind(prior.revision)
        .bind(prior.request_id)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if bound {
            Err(Error::InputConflict)
        } else {
            Ok(prior)
        }
    }
}

mod decoding;

use decoding::{decode_binding, decode_revision, matrix_write_error, same_request};

#[async_trait]
impl MatrixTaskStore for PgUnitOfWork {
    async fn matrix_task_source_by_request(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<(MatrixTaskSource, String)>> {
        let prior = self
            .matrix_receipt_by_request(workspace_id, request_id)
            .await?;
        let Some(revision) = prior else {
            return Ok(None);
        };
        let tenant = self.tenant_id()?;
        let row = sqlx::query("SELECT requirements_locator,snapshot_id,semantic_digest,authority_schema,original_request_digest FROM matrix_task_requirements_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4 AND request_id=$5")
            .bind(tenant).bind(workspace_id).bind(revision.task_id).bind(revision.revision).bind(request_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let (binding, digest) = row.ok_or(Error::InputConflict).and_then(decode_binding)?;
        Ok(Some((
            MatrixTaskSource {
                revision,
                requirements_binding: Some(binding),
            },
            digest,
        )))
    }

    async fn record_matrix_task_bound(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        record: BoundMatrixTaskRecord<'_>,
    ) -> Result<MatrixTaskSource> {
        let BoundMatrixTaskRecord {
            request,
            canonical_input,
            input_digest,
            original_request_digest,
            binding,
        } = record;
        if !self.is_read_write()
            || self.principal_id()? != principal_id
            || original_request_digest.len() != 64
            || !original_request_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
            || canonical_matrix_input_digest(canonical_input)? != input_digest
            || serde_json::to_value(&request.input).map_err(storage_error)? != *canonical_input
        {
            return Err(Error::InputConflict);
        }
        if let Some((prior, digest)) = self
            .matrix_task_source_by_request(workspace_id, request.request_id)
            .await?
        {
            return if digest == original_request_digest
                && prior.revision.task_id == request.task_id
                && prior.revision.revision == request.revision
                && prior.requirements_binding.as_ref().map(|b| &b.locator) == Some(&binding.locator)
            {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }
        let tenant = self.tenant_id()?;
        let snapshot:Option<(serde_json::Value,String,String)>=sqlx::query_as("SELECT anchor,semantic_digest,schema_version FROM matrix_requirements_snapshots WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace_id).bind(binding.snapshot_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let (anchor, semantic, schema) = snapshot.ok_or(Error::InputConflict)?;
        if semantic != binding.semantic_digest
            || schema != binding.authority_schema
            || anchor.get("program_id").is_none()
        {
            return Err(Error::InputConflict);
        }
        let revision = match self
            .record_matrix_task(
                workspace_id,
                principal_id,
                session_id,
                request,
                canonical_input,
                input_digest,
            )
            .await
        {
            Ok(value) => value,
            Err(error @ (Error::InputConflict | Error::StaleRevision)) => {
                if let Some((prior, digest)) = self
                    .matrix_task_source_by_request(workspace_id, request.request_id)
                    .await?
                {
                    return if digest == original_request_digest
                        && prior.revision.task_id == request.task_id
                        && prior.revision.revision == request.revision
                        && prior.requirements_binding.as_ref().map(|b| &b.locator)
                            == Some(&binding.locator)
                    {
                        Ok(prior)
                    } else {
                        Err(Error::InputConflict)
                    };
                }
                return Err(error);
            }
            Err(error) => return Err(error),
        };
        sqlx::query("INSERT INTO matrix_task_requirements_bindings(tenant_id,workspace_id,task_id,revision,request_id,requirements_locator,snapshot_id,semantic_digest,authority_schema,original_request_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10) ON CONFLICT DO NOTHING")
            .bind(tenant).bind(workspace_id).bind(revision.task_id).bind(revision.revision).bind(revision.request_id)
            .bind(binding.locator.as_json()).bind(binding.snapshot_id).bind(&binding.semantic_digest).bind(&binding.authority_schema).bind(original_request_digest)
            .execute(&mut **self.transaction()?).await.map_err(matrix_write_error)?;
        let (saved, digest) = self
            .matrix_task_source_by_request(workspace_id, request.request_id)
            .await?
            .ok_or(Error::InternalInvariant)?;
        if digest != original_request_digest
            || saved.revision != revision
            || saved.requirements_binding.as_ref() != Some(binding)
        {
            return Err(Error::InputConflict);
        }
        Ok(saved)
    }

    async fn matrix_task_source(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskSource>> {
        let Some(revision) = self.matrix_task(workspace_id, task_id).await? else {
            return Ok(None);
        };
        let tenant = self.tenant_id()?;
        let row=sqlx::query("SELECT requirements_locator,snapshot_id,semantic_digest,authority_schema,original_request_digest FROM matrix_task_requirements_bindings WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 AND revision=$4")
            .bind(tenant).bind(workspace_id).bind(task_id).bind(revision.revision)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let requirements_binding = row
            .map(decode_binding)
            .transpose()?
            .map(|(binding, _)| binding);
        Ok(Some(MatrixTaskSource {
            revision,
            requirements_binding,
        }))
    }

    async fn record_matrix_task(
        &mut self,
        workspace_id: Uuid,
        principal_id: Uuid,
        session_id: Uuid,
        request: &RecordMatrixTask,
        canonical_input: &serde_json::Value,
        input_digest: &str,
    ) -> Result<MatrixTaskRevision> {
        let tenant_id = self.tenant_id()?;
        let choice_set_json = request
            .choice_set
            .as_ref()
            .map(serde_json::to_value)
            .transpose()
            .map_err(|_| Error::InvalidArguments)?;
        let choice_set_digest = request
            .choice_set
            .as_ref()
            .map(|choice| choice.canonical_digest(&request.input))
            .transpose()?;
        if self.principal_id()? != principal_id {
            return Err(Error::Forbidden);
        }
        if let Some(prior) = self
            .matrix_receipt_by_request(workspace_id, request.request_id)
            .await?
        {
            return self
                .matrix_unbound_replay(workspace_id, prior, request, canonical_input, input_digest)
                .await;
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
                return self
                    .matrix_retry_or_error(
                        workspace_id,
                        request,
                        canonical_input,
                        input_digest,
                        Error::StaleRevision,
                    )
                    .await;
            }
        } else {
            let current: Option<i64> = sqlx::query_scalar(
                "SELECT current_revision FROM matrix_tasks \
                 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE NOWAIT",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(request.task_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(crate::matrix_lock_error)?;
            if current != Some(request.expected_current_revision) {
                return self
                    .matrix_retry_or_error(
                        workspace_id,
                        request,
                        canonical_input,
                        input_digest,
                        Error::StaleRevision,
                    )
                    .await;
            }
        }

        let inserted = sqlx::query(
            "INSERT INTO matrix_task_revisions (tenant_id,workspace_id,task_id,revision,previous_revision,request_id,input_schema,canonical_input,input_digest,recorded_by_principal_id,recorded_by_session_id,choice_set_schema,choice_set,choice_set_digest) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT DO NOTHING",
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
        .bind(request.choice_set.as_ref().map(|_| MATRIX_CHOICE_SET_SCHEMA))
        .bind(&choice_set_json)
        .bind(&choice_set_digest)
        .execute(&mut **self.transaction()?)
        .await
        .map_err(matrix_write_error)?;
        if inserted.rows_affected() != 1 {
            return self
                .matrix_retry_or_error(
                    workspace_id,
                    request,
                    canonical_input,
                    input_digest,
                    Error::InputConflict,
                )
                .await;
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
            choice_set: request.choice_set.clone(),
            choice_set_digest,
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
            "SELECT r.task_id,r.revision,r.request_id,r.input_schema,r.canonical_input,r.input_digest,r.choice_set_schema,r.choice_set,r.choice_set_digest, \
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
        row.map(decode_revision).transpose()
    }

    async fn lock_matrix_task(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
    ) -> Result<Option<MatrixTaskRevision>> {
        let tenant_id = self.tenant_id()?;
        let head: Option<i64> = sqlx::query_scalar(
            "SELECT current_revision FROM matrix_tasks WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE NOWAIT",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(crate::matrix_lock_error)?;
        match head {
            Some(revision) => {
                let current = self.matrix_task(workspace_id, task_id).await?;
                if current
                    .as_ref()
                    .is_none_or(|value| value.revision != revision)
                {
                    return Err(Error::InternalInvariant);
                }
                Ok(current)
            }
            None => Ok(None),
        }
    }
}

#[cfg(test)]
#[path = "matrix_task_store_tests.rs"]
mod tests;
