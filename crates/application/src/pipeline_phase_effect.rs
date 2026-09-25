//! Independent observation of one saved, completed pipeline phase attempt.
use crate::{TransactionMode, WorkspaceService};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use tect_domain::{Error, PrincipalRole, RequestContext, Result};
use uuid::Uuid;

pub use tect_domain::PipelinePhaseEffectMaterial;
fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PipelinePhaseEffectVerdict {
    Pass,
    Fail,
    Unknown,
}
impl PipelinePhaseEffectVerdict {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Pass => "pass",
            Self::Fail => "fail",
            Self::Unknown => "unknown",
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct VerifyPipelinePhaseEffect {
    pub request_id: Uuid,
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub expected_effect_digest: String,
    pub verdict: PipelinePhaseEffectVerdict,
    pub observation: Option<String>,
    pub observed_output_digest: Option<String>,
    pub summary: String,
}
impl VerifyPipelinePhaseEffect {
    pub fn validate(&self) -> Result<()> {
        if self.request_id.is_nil()
            || self.run_id.is_nil()
            || self.attempt_id.is_nil()
            || !hex_digest(&self.expected_effect_digest)
            || self.summary.trim().is_empty()
            || self.summary.trim() != self.summary
            || self.summary.len() > 4096
            || self.summary.contains('\0')
        {
            return Err(Error::InvalidArguments);
        }
        match self.verdict {
            PipelinePhaseEffectVerdict::Pass | PipelinePhaseEffectVerdict::Fail => {
                let observation = self.observation.as_deref().ok_or(Error::InvalidArguments)?;
                if observation.trim().len() < 32
                    || observation.trim() != observation
                    || observation.len() > 16384
                    || observation.contains('\0')
                    || !self
                        .observed_output_digest
                        .as_deref()
                        .is_some_and(hex_digest)
                {
                    return Err(Error::InvalidArguments);
                }
            }
            PipelinePhaseEffectVerdict::Unknown => {
                if self.observation.is_some() || self.observed_output_digest.is_some() {
                    return Err(Error::InvalidArguments);
                }
            }
        }
        Ok(())
    }
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct PipelinePhaseEffectAttestation {
    pub request_id: Uuid,
    pub workspace_id: Uuid,
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub effect_digest: String,
    pub output_digest: String,
    pub verifier_principal_id: Uuid,
    pub verifier_session_id: Uuid,
    pub verdict: PipelinePhaseEffectVerdict,
    pub observation: Option<String>,
    pub observation_digest: Option<String>,
    pub summary: String,
}

#[async_trait]
pub trait PipelinePhaseEffectStore: Send {
    async fn pipeline_phase_effect(
        &mut self,
        workspace: Uuid,
        run: Uuid,
        attempt: Uuid,
        lock: bool,
    ) -> Result<Option<PipelinePhaseEffectMaterial>>;
    async fn pipeline_phase_effect_attestation(
        &mut self,
        workspace: Uuid,
        request: Uuid,
    ) -> Result<Option<PipelinePhaseEffectAttestation>>;
    async fn append_pipeline_phase_effect_attestation(
        &mut self,
        workspace: Uuid,
        value: &PipelinePhaseEffectAttestation,
    ) -> Result<()>;
}

impl WorkspaceService {
    pub async fn get_pipeline_phase_effect(
        &self,
        context: &RequestContext,
        run: Uuid,
        attempt: Uuid,
    ) -> Result<(PipelinePhaseEffectMaterial, String, Uuid, Uuid)> {
        if run.is_nil() || attempt.is_nil() {
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
            .pipeline_phase_effect_store()
            .ok_or(Error::StorageUnavailable)?
            .pipeline_phase_effect(workspace.id, run, attempt, false)
            .await?
            .ok_or(Error::NotFound)?;
        material.validate()?;
        if forbidden(&material, identity.principal_id, session.id) {
            return Err(Error::Forbidden);
        }
        let digest = material.digest()?;
        tx.commit().await?;
        Ok((material, digest, identity.principal_id, session.id))
    }

    pub async fn verify_pipeline_phase_effect(
        &self,
        context: &RequestContext,
        request: &VerifyPipelinePhaseEffect,
    ) -> Result<PipelinePhaseEffectAttestation> {
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
            .pipeline_phase_effect_store()
            .ok_or(Error::StorageUnavailable)?;
        if let Some(existing) = store
            .pipeline_phase_effect_attestation(workspace.id, request.request_id)
            .await?
        {
            if existing.run_id == request.run_id
                && existing.attempt_id == request.attempt_id
                && existing.effect_digest == request.expected_effect_digest
                && existing.verifier_principal_id == identity.principal_id
                && existing.verifier_session_id == session.id
                && existing.verdict == request.verdict
                && existing.observation == request.observation
                && existing.output_digest
                    == request
                        .observed_output_digest
                        .clone()
                        .unwrap_or(existing.output_digest.clone())
                && existing.summary == request.summary
            {
                tx.commit().await?;
                return Ok(existing);
            }
            return Err(Error::InputConflict);
        }
        let material = store
            .pipeline_phase_effect(workspace.id, request.run_id, request.attempt_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        material.validate()?;
        if material.digest()? != request.expected_effect_digest {
            return Err(Error::InputConflict);
        }
        if forbidden(&material, identity.principal_id, session.id) {
            return Err(Error::Forbidden);
        }
        if request.verdict != PipelinePhaseEffectVerdict::Unknown
            && request.observed_output_digest.as_deref() != Some(&material.output_digest)
        {
            return Err(Error::InputConflict);
        }
        let attestation = PipelinePhaseEffectAttestation {
            request_id: request.request_id,
            workspace_id: workspace.id,
            run_id: request.run_id,
            attempt_id: request.attempt_id,
            effect_digest: request.expected_effect_digest.clone(),
            output_digest: material.output_digest,
            verifier_principal_id: identity.principal_id,
            verifier_session_id: session.id,
            verdict: request.verdict,
            observation: request.observation.clone(),
            observation_digest: request
                .observation
                .as_ref()
                .map(|content| format!("{:x}", Sha256::digest(content.as_bytes()))),
            summary: request.summary.clone(),
        };
        store
            .append_pipeline_phase_effect_attestation(workspace.id, &attestation)
            .await?;
        tx.commit().await?;
        Ok(attestation)
    }
}

fn forbidden(material: &PipelinePhaseEffectMaterial, principal: Uuid, session: Uuid) -> bool {
    principal == material.caller_principal_id
        || principal == material.slice_opener_principal_id
        || principal == material.matrix_owner_principal_id
        || session == material.caller_session_id
}

#[cfg(test)]
mod golden {
    use super::*;
    use tect_domain::PipelineVerificationObligation;

    #[test]
    fn canonical_wire_and_digest_match_pre_move_golden() {
        let obligation = PipelineVerificationObligation {
            phase_id: "phase-1".into(),
            required_fields: vec!["result".into()],
            required_artifacts: vec![],
            validator_contracts: vec![],
            output_constraints: vec![],
            allowed_verdicts: vec!["pass".into()],
            verdict_routes: vec![],
            disposition_required: false,
            required_dispositions: vec![],
            fresh_reviewer_input: false,
            output_contract: "result".into(),
        };
        let material = PipelinePhaseEffectMaterial {
            workspace_id: Uuid::from_u128(1),
            slice_id: Uuid::from_u128(2),
            run_id: Uuid::from_u128(3),
            attempt_id: Uuid::from_u128(4),
            phase_id: "phase-1".into(),
            attempt_number: 1,
            selected_option_id: "option".into(),
            verification_plan_id: format!("verification-plan:{}", "a".repeat(64)),
            verification_plan_version: "v1".into(),
            verification_plan_digest: "a".repeat(64),
            obligation_digest: "602275499b013cf785f1a182e8ca9fceb39e683b14f4e2b18aff008d8d35852f"
                .into(),
            validator_contracts_digest:
                "4f53cda18c2baa0c0354bb5f9a3ecbe5ed12ab4d8e11ba873c2f11161202b945".into(),
            obligation,
            output_id: Uuid::from_u128(5),
            output_digest: "b".repeat(64),
            output: serde_json::json!({"body_digest": "b".repeat(64), "result": "ok"}),
            caller_verdict: Some("pass".into()),
            evidence_refs: vec![],
            caller_principal_id: Uuid::from_u128(6),
            caller_session_id: Uuid::from_u128(7),
            slice_opener_principal_id: Uuid::from_u128(8),
            matrix_owner_principal_id: Uuid::from_u128(9),
        };
        assert!(material.validate().is_ok());
        assert_eq!(
            serde_json::to_string(&material).unwrap(),
            include_str!("../../domain/src/pipeline_effect_golden/phase.json").trim_end()
        );
        assert_eq!(
            material.digest().unwrap(),
            "70334d8dbde94018c59bedbc90f7a391b70adc4c122d875f0867e7cc8085909e"
        );
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(verdict: PipelinePhaseEffectVerdict) -> VerifyPipelinePhaseEffect {
        VerifyPipelinePhaseEffect {
            request_id: Uuid::new_v4(),
            run_id: Uuid::new_v4(),
            attempt_id: Uuid::new_v4(),
            expected_effect_digest: "a".repeat(64),
            verdict,
            observation: None,
            observed_output_digest: None,
            summary: "Checked saved output".into(),
        }
    }

    #[test]
    fn pass_and_fail_require_a_content_observation_bound_to_saved_output() {
        for verdict in [
            PipelinePhaseEffectVerdict::Pass,
            PipelinePhaseEffectVerdict::Fail,
        ] {
            let mut value = request(verdict);
            assert!(value.validate().is_err());
            value.observation =
                Some("I independently inspected the saved phase output content.".into());
            assert!(value.validate().is_err());
            value.observed_output_digest = Some("b".repeat(64));
            assert!(value.validate().is_ok());
        }
    }

    #[test]
    fn unknown_is_the_only_verdict_without_observation() {
        let mut value = request(PipelinePhaseEffectVerdict::Unknown);
        assert!(value.validate().is_ok());
        value.observation =
            Some("I independently inspected the saved phase output content.".into());
        assert!(value.validate().is_err());
    }
}
