use async_trait::async_trait;
use sqlx::Row;
use tect_application::MatrixVerificationStore;
use tect_domain::{
    EngineeringMatrixInput, Error, EvidenceValidationOutcome, MATRIX_VERIFICATION_SCHEMA,
    MatrixEvidenceBinding, MatrixVerificationRecord, Result, evaluate_matrix_verification,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

const VERIFIED_REASON: &str = "matrix_facts_verified";

fn epoch_seconds() -> Result<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    let seconds = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map_err(|_| Error::InternalInvariant)?
        .as_secs();
    i64::try_from(seconds).map_err(|_| Error::InternalInvariant)
}

fn verification_write_error(error: sqlx::Error) -> Error {
    if error
        .as_database_error()
        .and_then(|database| database.code())
        .is_some_and(|code| code.as_ref() == "42501")
    {
        Error::Forbidden
    } else {
        storage_error(error)
    }
}

impl PgUnitOfWork {
    async fn verification_by_digest(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        digest: &str,
    ) -> Result<Option<MatrixVerificationRecord>> {
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT id,input_digest,schema,owner_principal_id,verifier_principal_id,policy_version,record_digest,verification_reason \
             FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 \
             AND task_revision=$4 AND record_digest=$5",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .bind(revision)
        .bind(digest)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        match row {
            Some(row) => self
                .decode_verification(workspace_id, task_id, revision, row)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    async fn decode_verification(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        row: sqlx::postgres::PgRow,
    ) -> Result<MatrixVerificationRecord> {
        let id: Uuid = row.try_get("id").map_err(storage_error)?;
        let reason: String = row.try_get("verification_reason").map_err(storage_error)?;
        if reason != VERIFIED_REASON {
            return Err(Error::InternalInvariant);
        }
        let tenant_id = self.tenant_id()?;
        let rows = sqlx::query(
            "SELECT fact_path,value_digest,evidence_ref,content_digest,source,subject,observed_at,expires_at,validation_outcome \
             FROM matrix_verification_bindings WHERE tenant_id=$1 AND workspace_id=$2 \
             AND verification_id=$3 ORDER BY fact_path",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(id)
        .fetch_all(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let bindings = rows
            .into_iter()
            .map(|binding| {
                let outcome: String = binding
                    .try_get("validation_outcome")
                    .map_err(storage_error)?;
                if outcome != "accepted" {
                    return Err(Error::InternalInvariant);
                }
                Ok(MatrixEvidenceBinding {
                    fact_path: binding.try_get("fact_path").map_err(storage_error)?,
                    value_digest: binding.try_get("value_digest").map_err(storage_error)?,
                    evidence_ref: binding.try_get("evidence_ref").map_err(storage_error)?,
                    content_digest: binding.try_get("content_digest").map_err(storage_error)?,
                    source: binding.try_get("source").map_err(storage_error)?,
                    subject: binding.try_get("subject").map_err(storage_error)?,
                    observed_at: binding.try_get("observed_at").map_err(storage_error)?,
                    expires_at: binding.try_get("expires_at").map_err(storage_error)?,
                    validation_outcome: EvidenceValidationOutcome::Accepted,
                })
            })
            .collect::<Result<Vec<_>>>()?;
        let owner_id: Uuid = row.try_get("owner_principal_id").map_err(storage_error)?;
        let verifier_id: Uuid = row
            .try_get("verifier_principal_id")
            .map_err(storage_error)?;
        let record = MatrixVerificationRecord {
            schema: row.try_get("schema").map_err(storage_error)?,
            task_id: task_id.to_string(),
            task_revision: revision.to_string(),
            input_digest: row.try_get("input_digest").map_err(storage_error)?,
            owner_principal: owner_id.to_string(),
            verifier_principal: verifier_id.to_string(),
            policy_version: row.try_get("policy_version").map_err(storage_error)?,
            bindings,
            digest: row.try_get("record_digest").map_err(storage_error)?,
        };
        if record.schema != MATRIX_VERIFICATION_SCHEMA
            || record.digest
                != record
                    .canonical_digest()
                    .map_err(|_| Error::InternalInvariant)?
        {
            return Err(Error::InternalInvariant);
        }
        Ok(record)
    }
}

#[async_trait]
impl MatrixVerificationStore for PgUnitOfWork {
    async fn matrix_verification_for_revision(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        input_digest: &str,
    ) -> Result<Option<MatrixVerificationRecord>> {
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT v.id,v.input_digest,v.schema,v.owner_principal_id,v.verifier_principal_id, \
                    v.policy_version,v.record_digest,v.verification_reason \
             FROM matrix_tasks AS t \
             JOIN matrix_verifications AS v ON v.tenant_id=t.tenant_id \
               AND v.workspace_id=t.workspace_id AND v.task_id=t.id \
             WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3 \
               AND t.current_revision=$4 AND v.task_revision=$4 AND v.input_digest=$5 \
             ORDER BY v.verified_at DESC,v.id DESC LIMIT 1",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .bind(revision)
        .bind(input_digest)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        match row {
            Some(row) => self
                .decode_verification(workspace_id, task_id, revision, row)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    async fn append_matrix_verification(
        &mut self,
        workspace_id: Uuid,
        verifier_session_id: Uuid,
        task_id: Uuid,
        expected_revision: i64,
        expected_input_digest: &str,
        record: &MatrixVerificationRecord,
    ) -> Result<()> {
        let tenant_id = self.tenant_id()?;
        let verifier_id = self.principal_id()?;
        if record.task_id != task_id.to_string()
            || record.task_revision != expected_revision.to_string()
            || record.input_digest != expected_input_digest
            || record.verifier_principal != verifier_id.to_string()
        {
            return Err(Error::InvalidArguments);
        }

        // The application took this lock before validation. Reacquiring it
        // guarantees the same head and digest even for a direct store caller.
        let row = sqlx::query(
            "SELECT r.canonical_input,r.input_digest,r.recorded_by_principal_id \
             FROM matrix_tasks AS t JOIN matrix_task_revisions AS r \
               ON r.tenant_id=t.tenant_id AND r.workspace_id=t.workspace_id \
               AND r.task_id=t.id AND r.revision=t.current_revision \
             WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3 \
               AND t.current_revision=$4 FOR UPDATE OF t",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .bind(expected_revision)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?
        .ok_or(Error::StaleRevision)?;
        let actual_digest: String = row.try_get("input_digest").map_err(storage_error)?;
        if actual_digest != expected_input_digest {
            return Err(Error::InputConflict);
        }
        let owner_id: Uuid = row
            .try_get("recorded_by_principal_id")
            .map_err(storage_error)?;
        if record.owner_principal != owner_id.to_string() || owner_id == verifier_id {
            return Err(Error::Forbidden);
        }
        let input_json: serde_json::Value =
            row.try_get("canonical_input").map_err(storage_error)?;
        let input: EngineeringMatrixInput =
            serde_json::from_value(input_json).map_err(storage_error)?;
        evaluate_matrix_verification(
            &record.task_id,
            &record.task_revision,
            &input,
            record,
            epoch_seconds()?,
        )?;

        if let Some(prior) = self
            .verification_by_digest(workspace_id, task_id, expected_revision, &record.digest)
            .await?
        {
            return if prior == *record {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO matrix_verifications \
             (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id, \
              verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12) RETURNING id",
        )
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(task_id)
        .bind(expected_revision)
        .bind(expected_input_digest)
        .bind(&record.schema)
        .bind(owner_id)
        .bind(verifier_id)
        .bind(verifier_session_id)
        .bind(VERIFIED_REASON)
        .bind(&record.policy_version)
        .bind(&record.digest)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(verification_write_error)?;
        for binding in &record.bindings {
            sqlx::query(
                "INSERT INTO matrix_verification_bindings \
                 (tenant_id,workspace_id,verification_id,fact_path,value_digest,evidence_ref, \
                  content_digest,source,subject,observed_at,expires_at,validation_outcome) \
                 VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'accepted')",
            )
            .bind(tenant_id)
            .bind(workspace_id)
            .bind(id)
            .bind(&binding.fact_path)
            .bind(&binding.value_digest)
            .bind(&binding.evidence_ref)
            .bind(&binding.content_digest)
            .bind(&binding.source)
            .bind(&binding.subject)
            .bind(binding.observed_at)
            .bind(binding.expires_at)
            .execute(&mut **self.transaction()?)
            .await
            .map_err(verification_write_error)?;
        }
        Ok(())
    }
}
