//! Independent observation of an explicitly opened Slice.

use crate::{TransactionMode, WorkspaceService};
use async_trait::async_trait;
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tect_domain::{
    Error, NativeSlice, OpenSlice, OpenSliceOutcome, PipelineDispositionResult, PrincipalRole,
    RequestContext, Result, SliceCandidateNode,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelineOpenEffectMaterial {
    pub workspace_id: Uuid,
    pub slice: NativeSlice,
    pub open_request: OpenSlice,
    pub open_receipt: OpenSliceOutcome,
    pub disposition: PipelineDispositionResult,
    pub work: SliceCandidateNode,
    pub source_snapshot_id: Uuid,
    pub source_snapshot_digest: String,
    pub matrix_disposition_id: Uuid,
    pub matrix_effect_attestation_id: Uuid,
    pub manifest_digest: String,
    pub matrix_owner_principal_id: Uuid,
    pub caller_principal_id: Uuid,
    pub caller_session_id: Uuid,
}

impl PipelineOpenEffectMaterial {
    pub fn digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    pub fn validate(&self) -> Result<()> {
        let slice = &self.slice;
        slice.validate_verification_plan_binding()?;
        let request = &self.open_request;
        let disposition = &self.disposition;
        if self.workspace_id.is_nil()
            || request.disposition_id != Some(disposition.id)
            || request.scope_id != slice.scope_id
            || request.candidate_id != slice.candidate_id
            || request.candidate_revision != slice.candidate_revision
            || request.candidate_snapshot_id != slice.opening_snapshot_id
            || disposition.work_id != slice.candidate_id
            || disposition.selected_kind != Some(slice.pipeline)
            || disposition.selected_option_id != slice.selected_option_id
            || disposition.request.expected_work_revision != slice.candidate_revision
            || self.work.id() != slice.candidate_id
            || self.work.revision() != slice.candidate_revision
            || !matches!(&self.work, SliceCandidateNode::Work { .. })
            || !matches!(&self.open_receipt, OpenSliceOutcome::Created(opened) if opened == slice)
            || self.source_snapshot_id.is_nil()
            || self.source_snapshot_digest.len() != 64
            || self.matrix_disposition_id.is_nil()
            || self.matrix_effect_attestation_id.is_nil()
            || self.manifest_digest != disposition.request.manifest_digest
            || self.matrix_owner_principal_id.is_nil()
            || self.caller_principal_id.is_nil()
            || self.caller_session_id.is_nil()
        {
            return Err(Error::StaleContext);
        }
        Ok(())
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelineOpenEffectVerdict {
    Matches,
    Rejects,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelineOpenEffectAttestation {
    pub request_id: Uuid,
    pub workspace_id: Uuid,
    pub slice_id: Uuid,
    pub open_request_id: Uuid,
    pub effect_digest: String,
    pub verifier_principal_id: Uuid,
    pub verifier_session_id: Uuid,
    pub verdict: PipelineOpenEffectVerdict,
    pub summary: String,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyPipelineOpenEffect {
    pub request_id: Uuid,
    pub slice_id: Uuid,
    pub open_request_id: Uuid,
    pub expected_effect_digest: String,
    pub verdict: PipelineOpenEffectVerdict,
    pub summary: String,
}

impl VerifyPipelineOpenEffect {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.slice_id.is_nil()
            || self.open_request_id.is_nil()
            || self.expected_effect_digest.len() != 64
            || !self
                .expected_effect_digest
                .bytes()
                .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
            || self.summary.trim().is_empty()
            || self.summary.trim() != self.summary
            || self.summary.len() > 4096
            || self.summary.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

#[async_trait]
pub trait PipelineOpenEffectStore: Send {
    async fn pipeline_open_effect(
        &mut self,
        workspace_id: Uuid,
        slice_id: Uuid,
        open_request_id: Uuid,
        for_update: bool,
    ) -> Result<Option<PipelineOpenEffectMaterial>>;
    async fn pipeline_open_effect_attestation(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<PipelineOpenEffectAttestation>>;
    async fn append_pipeline_open_effect_attestation(
        &mut self,
        workspace_id: Uuid,
        attestation: &PipelineOpenEffectAttestation,
    ) -> Result<()>;
}

impl WorkspaceService {
    pub async fn get_pipeline_open_effect(
        &self,
        context: &RequestContext,
        slice_id: Uuid,
        open_request_id: Uuid,
    ) -> Result<(PipelineOpenEffectMaterial, String, Uuid, Uuid)> {
        if slice_id.is_nil() || open_request_id.is_nil() {
            return Err(Error::InvalidArguments);
        }
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadOnly)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let material = tx
            .pipeline_open_effect_store()
            .ok_or(Error::StorageUnavailable)?
            .pipeline_open_effect(workspace.id, slice_id, open_request_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        material.validate()?;
        if identity.principal_id == material.caller_principal_id
            || identity.principal_id == material.matrix_owner_principal_id
        {
            return Err(Error::Forbidden);
        }
        let digest = material.digest()?;
        tx.commit().await?;
        Ok((material, digest, identity.principal_id, session.id))
    }

    pub async fn verify_pipeline_open_effect(
        &self,
        context: &RequestContext,
        request: &VerifyPipelineOpenEffect,
    ) -> Result<PipelineOpenEffectAttestation> {
        request.validate()?;
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        if identity.role != PrincipalRole::Verifier {
            return Err(Error::Forbidden);
        }
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
        let store = tx
            .pipeline_open_effect_store()
            .ok_or(Error::StorageUnavailable)?;
        if let Some(existing) = store
            .pipeline_open_effect_attestation(workspace.id, request.request_id)
            .await?
        {
            if existing.workspace_id == workspace.id
                && existing.slice_id == request.slice_id
                && existing.open_request_id == request.open_request_id
                && existing.effect_digest == request.expected_effect_digest
                && existing.verifier_principal_id == identity.principal_id
                && existing.verifier_session_id == session.id
                && existing.verdict == request.verdict
                && existing.summary == request.summary
            {
                tx.commit().await?;
                return Ok(existing);
            }
            return Err(Error::InputConflict);
        }
        let material = store
            .pipeline_open_effect(
                workspace.id,
                request.slice_id,
                request.open_request_id,
                true,
            )
            .await?
            .ok_or(Error::NotFound)?;
        material.validate()?;
        if material.digest()? != request.expected_effect_digest {
            return Err(Error::InputConflict);
        }
        if identity.principal_id == material.caller_principal_id
            || identity.principal_id == material.matrix_owner_principal_id
        {
            return Err(Error::Forbidden);
        }
        let attestation = PipelineOpenEffectAttestation {
            request_id: request.request_id,
            workspace_id: workspace.id,
            slice_id: request.slice_id,
            open_request_id: request.open_request_id,
            effect_digest: request.expected_effect_digest.clone(),
            verifier_principal_id: identity.principal_id,
            verifier_session_id: session.id,
            verdict: request.verdict,
            summary: request.summary.clone(),
        };
        store
            .append_pipeline_open_effect_attestation(workspace.id, &attestation)
            .await?;
        tx.commit().await?;
        Ok(attestation)
    }
}
