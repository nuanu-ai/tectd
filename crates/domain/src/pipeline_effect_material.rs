//! Canonical, transport-neutral material for independent pipeline effect observation.
use crate::{
    Error, NativeSlice, OpenSlice, OpenSliceOutcome, PipelineDispositionResult,
    PipelineEvidenceRef, PipelineVerificationObligation, Result, SliceCandidateNode,
};
use serde::{Deserialize, Serialize};
use serde_json::Value;
use sha2::{Digest, Sha256};
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

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct PipelinePhaseEffectMaterial {
    pub workspace_id: Uuid,
    pub slice_id: Uuid,
    pub run_id: Uuid,
    pub attempt_id: Uuid,
    pub phase_id: String,
    pub attempt_number: i64,
    pub selected_option_id: String,
    pub verification_plan_id: String,
    pub verification_plan_version: String,
    pub verification_plan_digest: String,
    pub obligation: PipelineVerificationObligation,
    pub obligation_digest: String,
    pub validator_contracts_digest: String,
    pub output_id: Uuid,
    pub output_digest: String,
    pub output: Value,
    pub caller_verdict: Option<String>,
    pub evidence_refs: Vec<PipelineEvidenceRef>,
    pub caller_principal_id: Uuid,
    pub caller_session_id: Uuid,
    pub slice_opener_principal_id: Uuid,
    pub matrix_owner_principal_id: Uuid,
}

impl PipelinePhaseEffectMaterial {
    pub fn digest(&self) -> Result<String> {
        digest(self)
    }
    pub fn validate(&self) -> Result<()> {
        if self.workspace_id.is_nil()
            || self.slice_id.is_nil()
            || self.run_id.is_nil()
            || self.attempt_id.is_nil()
            || self.output_id.is_nil()
            || self.attempt_number < 1
            || self.phase_id != self.obligation.phase_id
            || self.verification_plan_id
                != format!("verification-plan:{}", self.verification_plan_digest)
            || !hex_digest(&self.verification_plan_digest)
            || !hex_digest(&self.output_digest)
            || self.obligation_digest != digest(&self.obligation)?
            || self.validator_contracts_digest != digest(&self.obligation.validator_contracts)?
            || self.caller_principal_id.is_nil()
            || self.caller_session_id.is_nil()
            || self.slice_opener_principal_id.is_nil()
            || self.matrix_owner_principal_id.is_nil()
            || self.output.get("body_digest").and_then(Value::as_str) != Some(&self.output_digest)
        {
            return Err(Error::StaleContext);
        }
        Ok(())
    }
}

fn digest<T: Serialize + ?Sized>(value: &T) -> Result<String> {
    let bytes = serde_json::to_vec(value).map_err(|_| Error::InternalInvariant)?;
    Ok(format!("{:x}", Sha256::digest(bytes)))
}

fn hex_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
}
