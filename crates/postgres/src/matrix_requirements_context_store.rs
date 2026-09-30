use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tect_application::{
    FrozenMatrixRequirementsContext, MatrixRequirementsContextStore, MatrixRequirementsLocator,
    StoredMatrixRequirementsConfirmation, StoredMatrixRequirementsProposal,
};
use tect_domain::*;
use uuid::Uuid;

mod codec;
mod lineage;
mod requests;
use codec::*;
use requests::*;

#[async_trait]
impl MatrixRequirementsContextStore for PgUnitOfWork {
    async fn frozen_matrix_requirements_by_id(
        &mut self,
        workspace: Uuid,
        snapshot_id: Uuid,
    ) -> Result<Option<FrozenMatrixRequirementsContext>> {
        if snapshot_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let tenant = self.tenant_id()?;
        let row=sqlx::query("SELECT anchor,program_id,schema_version,payload,canonical_payload,payload_sha256,semantic_digest FROM matrix_requirements_snapshots WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
            .bind(tenant).bind(workspace).bind(snapshot_id).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let Some(row) = row else { return Ok(None) };
        let anchor: RequirementsAnchor =
            serde_json::from_value(row.try_get("anchor").map_err(storage_error)?)
                .map_err(|_| Error::InternalInvariant)?;
        let program_id: Uuid = row.try_get("program_id").map_err(storage_error)?;
        let schema: String = row.try_get("schema_version").map_err(storage_error)?;
        let payload: Value = row.try_get("payload").map_err(storage_error)?;
        let bytes: Vec<u8> = row.try_get("canonical_payload").map_err(storage_error)?;
        let payload_sha256: String = row.try_get("payload_sha256").map_err(storage_error)?;
        let semantic_digest: String = row.try_get("semantic_digest").map_err(storage_error)?;
        if bytes.is_empty()
            || bytes.len() > 8_388_608
            || format!("{:x}", Sha256::digest(&bytes)) != payload_sha256
            || serde_json::from_slice::<Value>(&bytes).map_err(|_| Error::InternalInvariant)?
                != payload
            || anchor.program_id() != program_id
            || schema != MATRIX_REQUIREMENTS_SCHEMA
        {
            return Err(Error::InternalInvariant);
        }
        let effective: EffectiveMatrixRequirements =
            serde_json::from_value(payload).map_err(|_| Error::InternalInvariant)?;
        let canonical = serde_json::to_vec(&effective).map_err(|_| Error::InternalInvariant)?;
        let semantic: std::collections::BTreeMap<_, _> = effective
            .values()
            .iter()
            .map(|(path, resolved)| (*path, &resolved.value))
            .collect();
        let semantic_value = serde_json::to_value((effective.schema(), semantic))
            .map_err(|_| Error::InternalInvariant)?;
        let semantic_bytes =
            serde_json::to_vec(&semantic_value).map_err(|_| Error::InternalInvariant)?;
        let expected_semantic = format!("{:x}", Sha256::digest(semantic_bytes));
        if canonical != bytes
            || effective.schema() != schema
            || effective.program_id() != program_id
            || effective.semantic_digest() != semantic_digest
            || semantic_digest != expected_semantic
        {
            return Err(Error::InternalInvariant);
        }
        Ok(Some(FrozenMatrixRequirementsContext {
            id: snapshot_id,
            anchor,
            effective,
            payload_sha256,
        }))
    }
    async fn matrix_requirements_lineage(
        &mut self,
        workspace: Uuid,
        principal: Uuid,
        locator: &MatrixRequirementsLocator,
        for_write: bool,
    ) -> Result<Vec<RequirementsAnchor>> {
        lineage::resolve(self, workspace, principal, locator, for_write).await
    }
    async fn matrix_requirements_proposal_by_request(
        &mut self,
        workspace: Uuid,
        request: Uuid,
    ) -> Result<Option<StoredMatrixRequirementsProposal>> {
        let tenant = self.tenant_id()?;
        sqlx::query("SELECT request_payload,proposal_payload,recorded_at_epoch_seconds FROM matrix_requirements_proposals WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
            .bind(tenant).bind(workspace).bind(request).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .map(proposal_record).transpose()
    }
    async fn matrix_requirements_confirmation_by_request(
        &mut self,
        workspace: Uuid,
        request: Uuid,
    ) -> Result<Option<StoredMatrixRequirementsConfirmation>> {
        let tenant = self.tenant_id()?;
        sqlx::query("SELECT c.request_payload,c.confirmation_payload,c.recorded_at_epoch_seconds,p.proposal_payload FROM matrix_requirements_confirmations c JOIN matrix_requirements_proposals p ON (p.tenant_id,p.workspace_id,p.anchor,p.context_revision)=(c.tenant_id,c.workspace_id,c.anchor,c.proposal_revision) WHERE c.tenant_id=$1 AND c.workspace_id=$2 AND c.request_id=$3")
            .bind(tenant).bind(workspace).bind(request).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .map(confirmation_record).transpose()
    }
    async fn lock_matrix_requirements_head(
        &mut self,
        workspace: Uuid,
        anchor: RequirementsAnchor,
    ) -> Result<u64> {
        if !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        // Covers the initially empty context namespace without mutable head rows.
        let key = format!(
            "matrix-requirements:{tenant}:{workspace}:{}",
            json(&anchor)?
        );
        let locked: bool = sqlx::query_scalar(
            "SELECT pg_catalog.pg_try_advisory_xact_lock(pg_catalog.hashtextextended($1,0))",
        )
        .bind(key)
        .fetch_one(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        if !locked {
            self.abort_matrix_lock_contention().await?;
            return Err(Error::StaleRevision);
        }
        let head: i64=sqlx::query_scalar("SELECT COALESCE(max(context_revision),0) FROM matrix_requirements_proposals WHERE tenant_id=$1 AND workspace_id=$2 AND anchor=$3")
            .bind(tenant).bind(workspace).bind(json(&anchor)?).fetch_one(&mut **self.transaction()?).await.map_err(storage_error)?;
        u64::try_from(head).map_err(|_| Error::InputConflict)
    }
    async fn matrix_requirements_revisions(
        &mut self,
        workspace: Uuid,
        anchors: &[RequirementsAnchor],
    ) -> Result<Vec<MatrixRequirementsRevision>> {
        let tenant = self.tenant_id()?;
        let mut revisions = Vec::new();
        for anchor in anchors {
            let rows=sqlx::query("SELECT p.proposal_payload,c.confirmation_payload FROM matrix_requirements_proposals p LEFT JOIN matrix_requirements_confirmations c ON (c.tenant_id,c.workspace_id,c.anchor,c.proposal_revision)=(p.tenant_id,p.workspace_id,p.anchor,p.context_revision) WHERE p.tenant_id=$1 AND p.workspace_id=$2 AND p.anchor=$3 ORDER BY p.context_revision")
                .bind(tenant).bind(workspace).bind(json(anchor)?).fetch_all(&mut **self.transaction()?).await.map_err(storage_error)?;
            for row in rows {
                let proposal =
                    decode_proposal(row.try_get("proposal_payload").map_err(storage_error)?)?;
                if proposal.anchor() != *anchor {
                    return Err(Error::InputConflict);
                }
                let confirmation = row
                    .try_get::<Option<Value>, _>("confirmation_payload")
                    .map_err(storage_error)?
                    .map(|value| decode_confirmation(value, &proposal))
                    .transpose()?;
                revisions.push(MatrixRequirementsRevision {
                    proposal,
                    confirmation,
                });
            }
        }
        Ok(revisions)
    }
    async fn append_matrix_requirements_proposal(
        &mut self,
        workspace: Uuid,
        expected: u64,
        record: &StoredMatrixRequirementsProposal,
    ) -> Result<()> {
        if !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        let proposal = decode_proposal(json(&record.proposal)?)?;
        if proposal.revision() != expected.checked_add(1).ok_or(Error::InvalidArguments)?
            || proposal
                != MatrixRequirementsProposal::new(
                    proposal.anchor(),
                    proposal.revision(),
                    record.request.patches.clone(),
                    proposal.recorder().clone(),
                )?
            || record.request.expected_context_revision != expected
        {
            return Err(Error::InputConflict);
        }
        let revision = signed(proposal.revision())?;
        if self
            .lock_matrix_requirements_head(workspace, proposal.anchor())
            .await?
            != expected
        {
            return Err(Error::StaleRevision);
        }
        let (principal, session) = recorder_ids(self, proposal.recorder()).await?;
        let tenant = self.tenant_id()?;
        let (scope, set, work) = anchor_ids(proposal.anchor());
        sqlx::query("INSERT INTO matrix_requirements_proposals(tenant_id,workspace_id,request_id,anchor,program_id,scope_id,candidate_set_id,work_candidate_id,context_revision,proposal_digest,request_payload,proposal_payload,recorded_by_principal_id,recorded_by_session_id,recorded_at_epoch_seconds) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15)")
            .bind(tenant).bind(workspace).bind(record.request.request_id).bind(json(&proposal.anchor())?).bind(proposal.anchor().program_id())
            .bind(scope).bind(set).bind(work).bind(revision).bind(proposal.digest()).bind(proposal_request_json(&record.request)?).bind(json(&proposal)?)
            .bind(principal).bind(session).bind(record.recorded_at_epoch_seconds).execute(&mut **self.transaction()?).await.map_err(write_error)?;
        Ok(())
    }
    async fn append_matrix_requirements_confirmation(
        &mut self,
        workspace: Uuid,
        record: &StoredMatrixRequirementsConfirmation,
    ) -> Result<()> {
        if !self.is_read_write() {
            return Err(Error::Forbidden);
        }
        let value = json(&record.confirmation)?;
        let anchor: RequirementsAnchor =
            serde_json::from_value(value["anchor"].clone()).map_err(|_| Error::InputConflict)?;
        let revision = signed(record.request.proposal_revision)?;
        if self
            .lock_matrix_requirements_head(workspace, anchor)
            .await?
            != record.request.proposal_revision
        {
            return Err(Error::StaleRevision);
        }
        let revisions = self
            .matrix_requirements_revisions(workspace, &[anchor])
            .await?;
        let proposal = revisions
            .iter()
            .find(|r| r.proposal.revision() == record.request.proposal_revision)
            .ok_or(Error::NotFound)?;
        let confirmation = decode_confirmation(value, &proposal.proposal)?;
        if proposal.confirmation.is_some()
            || record.request.proposal_digest != proposal.proposal.digest()
            || record.request.owner_response_ref != confirmation.owner_response_ref()
        {
            return Err(Error::InputConflict);
        }
        let (principal, session) = recorder_ids(self, confirmation.recorder()).await?;
        if confirmation.owner_principal() != principal.to_string() {
            return Err(Error::Forbidden);
        }
        let tenant = self.tenant_id()?;
        sqlx::query("INSERT INTO matrix_requirements_confirmations(tenant_id,workspace_id,request_id,anchor,program_id,proposal_revision,proposal_digest,owner_response_ref,request_payload,confirmation_payload,recorded_by_principal_id,recorded_by_session_id,recorded_at_epoch_seconds) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)")
            .bind(tenant).bind(workspace).bind(record.request.request_id).bind(json(&anchor)?).bind(anchor.program_id()).bind(revision)
            .bind(proposal.proposal.digest()).bind(confirmation.owner_response_ref()).bind(confirmation_request_json(&record.request)?).bind(json(&confirmation)?)
            .bind(principal).bind(session).bind(record.recorded_at_epoch_seconds).execute(&mut **self.transaction()?).await.map_err(write_error)?;
        Ok(())
    }
    async fn append_frozen_matrix_requirements(
        &mut self,
        workspace: Uuid,
        snapshot: &FrozenMatrixRequirementsContext,
    ) -> Result<FrozenMatrixRequirementsContext> {
        if !self.is_read_write() || snapshot.id.is_nil() {
            return Err(Error::Forbidden);
        }
        let anchor = snapshot.anchor;
        let anchors = match anchor {
            RequirementsAnchor::Program { .. } => vec![anchor],
            RequirementsAnchor::Scope { program_id, .. } => {
                vec![RequirementsAnchor::Program { program_id }, anchor]
            }
            RequirementsAnchor::Slice {
                program_id,
                scope_id,
                ..
            } => vec![
                RequirementsAnchor::Program { program_id },
                RequirementsAnchor::Scope {
                    program_id,
                    scope_id,
                },
                anchor,
            ],
        };
        let revisions = self
            .matrix_requirements_revisions(workspace, &anchors)
            .await?;
        let trusted =
            resolve_matrix_requirements(&anchors, &revisions, MATRIX_REQUIREMENTS_SCHEMA)?;
        if trusted != snapshot.effective {
            return Err(Error::StaleContext);
        }
        let bytes = serde_json::to_vec(&trusted).map_err(storage_error)?;
        let hash = format!("{:x}", Sha256::digest(&bytes));
        if hash != snapshot.payload_sha256 {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        let existing:Option<(Uuid,Vec<u8>)>=sqlx::query_as("SELECT id,canonical_payload FROM matrix_requirements_snapshots WHERE tenant_id=$1 AND workspace_id=$2 AND anchor=$3 AND payload_sha256=$4")
            .bind(tenant).bind(workspace).bind(json(&anchor)?).bind(&hash).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        if let Some((id, saved)) = existing {
            if saved != bytes {
                return Err(Error::InputConflict);
            }
            return Ok(FrozenMatrixRequirementsContext {
                id,
                anchor,
                effective: trusted,
                payload_sha256: hash,
            });
        }
        sqlx::query("INSERT INTO matrix_requirements_snapshots(tenant_id,workspace_id,id,anchor,program_id,schema_version,payload,canonical_payload,payload_sha256,semantic_digest) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(tenant).bind(workspace).bind(snapshot.id).bind(json(&anchor)?).bind(anchor.program_id()).bind(trusted.schema())
            .bind(json(&trusted)?).bind(bytes).bind(hash).bind(trusted.semantic_digest()).execute(&mut **self.transaction()?).await.map_err(write_error)?;
        Ok(snapshot.clone())
    }
}
