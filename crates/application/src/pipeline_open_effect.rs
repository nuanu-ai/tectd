//! Independent observation of an explicitly opened Slice.

use crate::{TransactionMode, WorkspaceService};
use async_trait::async_trait;
use tect_domain::{Error, PrincipalRole, RequestContext, Result};
use uuid::Uuid;

pub use tect_domain::PipelineOpenEffectMaterial;

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

#[cfg(test)]
mod golden {
    use super::*;
    use tect_domain::{
        NativeSlice, OpenSlice, OpenSliceOutcome, PipelineDispositionAdvice,
        PipelineDispositionRequest, PipelineDispositionResult, PipelineKind,
        PipelineRecommendationDisposition, SliceCandidateNode, SliceState,
    };

    #[test]
    fn canonical_wire_and_digest_match_pre_move_golden() {
        let work_id = Uuid::from_u128(4);
        let kind = PipelineKind::LightweightTddDevelopment;
        let work = SliceCandidateNode::Work {
            id: work_id,
            revision: 2,
            model_route_facts: None,
            title: "Work".into(),
            outcome: "Ship".into(),
            includes: vec![],
            excludes: vec![],
            dependencies: vec![],
            proof: vec!["Test".into()],
            pipeline: kind,
            pipeline_reason: "Small".into(),
            why_lightweight_insufficient: None,
            why_further_vertical_split_not_viable: None,
            source_result_ids: vec![],
            source_checkpoint: None,
        };
        let slice = NativeSlice {
            id: Uuid::from_u128(5),
            scope_id: Uuid::from_u128(2),
            revision: 1,
            candidate_id: work_id,
            candidate_revision: 2,
            opening_snapshot_id: Uuid::from_u128(3),
            title: "Slice".into(),
            outcome: "Ship".into(),
            pipeline: kind,
            selected_option_id: Some("option".into()),
            verification_plan_id: None,
            verification_plan_schema: None,
            verification_plan_digest: None,
            verification_plan_source_definition_version: None,
            verification_plan_source_definition_digest: None,
            state: SliceState::Open,
            pipeline_status: "pending".into(),
            pipeline_run_id: None,
            knowledge_change_id: None,
            knowledge_run_id: None,
            knowledge_status: None,
            source_checkpoint: None,
            execution_claimed: false,
        };
        let open_request = OpenSlice {
            request_id: Uuid::from_u128(6),
            scope_id: slice.scope_id,
            scope_revision: 1,
            candidate_set_id: Uuid::from_u128(7),
            candidate_set_revision: 3,
            candidate_snapshot_id: slice.opening_snapshot_id,
            candidate_id: work_id,
            candidate_revision: 2,
            disposition_id: Some(Uuid::from_u128(8)),
        };
        let disposition = PipelineDispositionResult {
            id: Uuid::from_u128(8),
            request: PipelineDispositionRequest {
                request_id: Uuid::from_u128(9),
                opportunity_id: Uuid::from_u128(10),
                expected_work_revision: 2,
                manifest_digest: "a".repeat(64),
                action: PipelineRecommendationDisposition::UseDeterministicChoice,
                rationale: "Selected".into(),
            },
            work_id,
            advice: PipelineDispositionAdvice::NoCall,
            selected_kind: Some(kind),
            selected_option_id: Some("option".into()),
        };
        let material = PipelineOpenEffectMaterial {
            workspace_id: Uuid::from_u128(1),
            slice: slice.clone(),
            open_request,
            open_receipt: OpenSliceOutcome::Created(slice),
            disposition,
            work,
            source_snapshot_id: Uuid::from_u128(11),
            source_snapshot_digest: "b".repeat(64),
            matrix_disposition_id: Uuid::from_u128(12),
            matrix_effect_attestation_id: Uuid::from_u128(13),
            manifest_digest: "a".repeat(64),
            matrix_owner_principal_id: Uuid::from_u128(14),
            caller_principal_id: Uuid::from_u128(15),
            caller_session_id: Uuid::from_u128(16),
        };
        assert_eq!(
            serde_json::to_string(&material).unwrap(),
            include_str!("../../domain/src/pipeline_effect_golden/open.json").trim_end()
        );
        assert_eq!(
            material.digest().unwrap(),
            "bcbac76bff1bb2427df2dea9027c86ad577ec52c5f932e7f3b44631465232b16"
        );
    }
}
