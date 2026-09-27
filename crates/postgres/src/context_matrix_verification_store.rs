use async_trait::async_trait;
use sqlx::Row;
use tect_application::{ContextMatrixVerificationStore, MatrixRequirementsContextStore};
use tect_domain::{
    CONTEXT_MATRIX_VERIFICATION_SCHEMA, ContextMatrixVerificationRecord, EngineeringMatrixInput,
    Error, EvidenceValidationOutcome, MatrixEvidenceBinding, Result,
    evaluate_context_matrix_verification,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

const REASON: &str = "operating_facts_verified";

fn epoch_seconds() -> Result<i64> {
    use std::time::{SystemTime, UNIX_EPOCH};
    i64::try_from(
        SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .map_err(|_| Error::InternalInvariant)?
            .as_secs(),
    )
    .map_err(|_| Error::InternalInvariant)
}

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" || code == "23514" || code == "23503" => Error::InputConflict,
        _ => storage_error(error),
    }
}

impl PgUnitOfWork {
    pub(crate) async fn decode_context_verification(
        &mut self,
        task_id: Uuid,
        revision: i64,
        workspace_id: Uuid,
        row: sqlx::postgres::PgRow,
    ) -> Result<ContextMatrixVerificationRecord> {
        let id: Uuid = row.try_get("id").map_err(storage_error)?;
        let reason: String = row.try_get("verification_reason").map_err(storage_error)?;
        let schema: String = row.try_get("schema").map_err(storage_error)?;
        if schema != CONTEXT_MATRIX_VERIFICATION_SCHEMA || reason != REASON {
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
        let owner: Uuid = row.try_get("owner_principal_id").map_err(storage_error)?;
        let verifier: Uuid = row
            .try_get("verifier_principal_id")
            .map_err(storage_error)?;
        let snapshot: Uuid = row.try_get("frozen_snapshot_id").map_err(storage_error)?;
        let record = ContextMatrixVerificationRecord {
            schema,
            task_id: task_id.to_string(),
            task_revision: revision.to_string(),
            frozen_snapshot_id: snapshot.to_string(),
            authority_schema: row.try_get("authority_schema").map_err(storage_error)?,
            input_digest: row.try_get("input_digest").map_err(storage_error)?,
            requirements_semantic_digest: row
                .try_get("requirements_semantic_digest")
                .map_err(storage_error)?,
            owner_principal: owner.to_string(),
            verifier_principal: verifier.to_string(),
            policy_version: row.try_get("policy_version").map_err(storage_error)?,
            bindings,
            digest: row.try_get("record_digest").map_err(storage_error)?,
        };
        if record.digest
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
impl ContextMatrixVerificationStore for PgUnitOfWork {
    async fn context_matrix_verification_for_revision(
        &mut self,
        workspace_id: Uuid,
        task_id: Uuid,
        revision: i64,
        input_digest: &str,
        frozen_snapshot_id: Uuid,
    ) -> Result<Option<ContextMatrixVerificationRecord>> {
        if frozen_snapshot_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let tenant_id = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT v.id,v.input_digest,v.schema,v.owner_principal_id,v.verifier_principal_id, \
                    v.policy_version,v.record_digest,v.verification_reason,v.frozen_snapshot_id, \
                    v.requirements_semantic_digest,v.authority_schema \
             FROM matrix_tasks t JOIN matrix_verifications v \
               ON (v.tenant_id,v.workspace_id,v.task_id)=(t.tenant_id,t.workspace_id,t.id) \
             JOIN matrix_task_requirements_bindings b \
               ON (b.tenant_id,b.workspace_id,b.task_id,b.revision,b.snapshot_id,b.semantic_digest,b.authority_schema)= \
                  (v.tenant_id,v.workspace_id,v.task_id,v.task_revision,v.frozen_snapshot_id,v.requirements_semantic_digest,v.authority_schema) \
             WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3 \
               AND t.current_revision=$4 AND v.task_revision=$4 AND v.input_digest=$5 \
               AND v.frozen_snapshot_id=$6 AND v.schema='tect.context-matrix-verification/1' \
             ORDER BY v.verified_at DESC,v.id DESC LIMIT 1",
        )
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(revision)
        .bind(input_digest).bind(frozen_snapshot_id)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        match row {
            Some(row) => self
                .decode_context_verification(task_id, revision, workspace_id, row)
                .await
                .map(Some),
            None => Ok(None),
        }
    }

    async fn append_context_matrix_verification(
        &mut self,
        workspace_id: Uuid,
        verifier_session_id: Uuid,
        task_id: Uuid,
        expected_revision: i64,
        expected_input_digest: &str,
        record: &ContextMatrixVerificationRecord,
    ) -> Result<()> {
        if !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        let tenant_id = self.tenant_id()?;
        let verifier_id = self.principal_id()?;
        let snapshot_id =
            Uuid::parse_str(&record.frozen_snapshot_id).map_err(|_| Error::InvalidArguments)?;
        if snapshot_id.is_nil()
            || record.frozen_snapshot_id != snapshot_id.to_string()
            || record.task_id != task_id.to_string()
            || record.task_revision != expected_revision.to_string()
            || record.input_digest != expected_input_digest
            || record.verifier_principal != verifier_id.to_string()
            || record.schema != CONTEXT_MATRIX_VERIFICATION_SCHEMA
            || record.digest != record.canonical_digest()?
        {
            return Err(Error::InvalidArguments);
        }
        // Keep the same task head locked through all validation and append.
        let row = sqlx::query(
            "SELECT r.canonical_input,r.input_digest,r.recorded_by_principal_id, \
                    b.snapshot_id,b.semantic_digest,b.authority_schema \
             FROM matrix_tasks t JOIN matrix_task_revisions r \
               ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=(t.tenant_id,t.workspace_id,t.id,t.current_revision) \
             JOIN matrix_task_requirements_bindings b \
               ON (b.tenant_id,b.workspace_id,b.task_id,b.revision)=(r.tenant_id,r.workspace_id,r.task_id,r.revision) \
             WHERE t.tenant_id=$1 AND t.workspace_id=$2 AND t.id=$3 AND t.current_revision=$4 \
             FOR UPDATE OF t",
        )
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(expected_revision)
        .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
        .ok_or(Error::StaleRevision)?;
        let actual_digest: String = row.try_get("input_digest").map_err(storage_error)?;
        if actual_digest != expected_input_digest {
            return Err(Error::InputConflict);
        }
        let owner_id: Uuid = row
            .try_get("recorded_by_principal_id")
            .map_err(storage_error)?;
        if owner_id == verifier_id || record.owner_principal != owner_id.to_string() {
            return Err(Error::Forbidden);
        }
        let bound_snapshot_id: Uuid = row.try_get("snapshot_id").map_err(storage_error)?;
        let semantic: String = row.try_get("semantic_digest").map_err(storage_error)?;
        let authority_schema: String = row.try_get("authority_schema").map_err(storage_error)?;
        if bound_snapshot_id != snapshot_id
            || semantic != record.requirements_semantic_digest
            || authority_schema != record.authority_schema
        {
            return Err(Error::StaleContext);
        }
        let input_json: serde_json::Value =
            row.try_get("canonical_input").map_err(storage_error)?;
        let input: EngineeringMatrixInput =
            serde_json::from_value(input_json).map_err(|_| Error::InternalInvariant)?;
        let frozen = MatrixRequirementsContextStore::frozen_matrix_requirements_by_id(
            self,
            workspace_id,
            snapshot_id,
        )
        .await?
        .ok_or(Error::StaleContext)?;
        if frozen.effective.semantic_digest() != semantic
            || frozen.effective.schema() != authority_schema
        {
            return Err(Error::StaleContext);
        }
        evaluate_context_matrix_verification(
            &record.task_id,
            &record.task_revision,
            &record.frozen_snapshot_id,
            &input,
            &frozen.effective,
            record,
            epoch_seconds()?,
        )?;
        let prior = sqlx::query(
            "SELECT id,input_digest,schema,owner_principal_id,verifier_principal_id,policy_version,record_digest,verification_reason,frozen_snapshot_id,requirements_semantic_digest,authority_schema \
             FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3 \
             AND task_revision=$4 AND record_digest=$5 AND schema='tect.context-matrix-verification/1'",
        )
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(expected_revision)
        .bind(&record.digest).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        if let Some(prior) = prior {
            let prior = self
                .decode_context_verification(task_id, expected_revision, workspace_id, prior)
                .await?;
            return if prior == *record {
                Ok(())
            } else {
                Err(Error::InputConflict)
            };
        }
        let id: Uuid = sqlx::query_scalar(
            "INSERT INTO matrix_verifications \
             (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id, \
              verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest, \
              frozen_snapshot_id,requirements_semantic_digest,authority_schema) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15) RETURNING id",
        )
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(expected_revision)
        .bind(expected_input_digest).bind(&record.schema).bind(owner_id).bind(verifier_id)
        .bind(verifier_session_id).bind(REASON).bind(&record.policy_version).bind(&record.digest)
        .bind(snapshot_id).bind(&semantic).bind(&authority_schema)
        .fetch_one(&mut **self.transaction()?).await.map_err(write_error)?;
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
            .map_err(write_error)?;
        }
        Ok(())
    }
}
