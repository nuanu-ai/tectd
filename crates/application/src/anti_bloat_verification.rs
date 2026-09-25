//! Independent readback of the native anti-bloat draft mutation.
use crate::{AntiBloatVerificationMaterial, Sha256ScopeDigest, TransactionMode, WorkspaceService};
use serde::{Deserialize, Serialize};
use sha2::{Digest, Sha256};
use tect_domain::{
    AntiBloatPreservationAttestation, AntiBloatVerificationReason, AntiBloatVerificationVerdict,
    Error, PrincipalRole, RequestContext, Result, check_anti_bloat_delta, review_anti_bloat,
    scope_candidate_material_digest,
};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct VerifyAntiBloatApply {
    pub request_id: Uuid,
    pub review_id: Uuid,
    pub expected_evidence_digest: String,
}

impl AntiBloatVerificationMaterial {
    pub fn digest(&self) -> Result<String> {
        let bytes = serde_json::to_vec(self).map_err(|_| Error::InternalInvariant)?;
        Ok(format!("{:x}", Sha256::digest(bytes)))
    }

    pub fn verdict(&self) -> (AntiBloatVerificationVerdict, AntiBloatVerificationReason) {
        if !self.source_fragments_match {
            return (
                AntiBloatVerificationVerdict::Unknown,
                AntiBloatVerificationReason::SourceEvidenceUnavailable,
            );
        }
        let digest = Sha256ScopeDigest;
        let selected = self.input.manifest.eligible(&self.input.selected_id);
        let valid = self.current_revision == self.receipt.to_revision
            && self.input.selected_revision == self.receipt.from_revision
            && self.review_id == self.receipt.review_id
            && self.review.candidate_set_id == self.receipt.candidate_set_id
            && self.delta.candidate_set_id == self.receipt.candidate_set_id
            && self.delta.expected_revision == self.receipt.from_revision
            && self.delta.idempotency_key == self.receipt.idempotency_key
            && self.receipt.from_revision.checked_add(1) == Some(self.receipt.to_revision)
            && self.receipt.source_digest == self.input.manifest.source.digest
            && self.preservation.source_digest == self.receipt.source_digest
            && self.preservation.before_material_digest == self.receipt.before_material_digest
            && self.preservation.after_material_digest == self.receipt.after_material_digest
            && self.review_actor_id != Uuid::nil()
            && selected.is_some_and(|value| value.material == self.before_saved)
            && self.claimed_after == self.after_saved
            && review_anti_bloat(&digest, &self.input).ok() == Some(self.review.clone())
            && check_anti_bloat_delta(
                &digest,
                &self.input,
                &self.review,
                &self.finding_id,
                self.disposition,
                &self.delta,
                &self.after_saved,
            )
            .ok()
                == Some(self.preservation.clone())
            && scope_candidate_material_digest(&digest, &self.before_saved).ok()
                == Some(self.receipt.before_material_digest.clone())
            && scope_candidate_material_digest(&digest, &self.after_saved).ok()
                == Some(self.receipt.after_material_digest.clone());
        if valid {
            (
                AntiBloatVerificationVerdict::Pass,
                AntiBloatVerificationReason::FullGraphPreserved,
            )
        } else {
            (
                AntiBloatVerificationVerdict::Fail,
                AntiBloatVerificationReason::GraphOrReceiptMismatch,
            )
        }
    }

    fn independent_of(&self, principal_id: Uuid, session_id: Uuid) -> bool {
        principal_id != self.review_actor_id
            && principal_id != self.selected_disposition_actor_id
            && principal_id != self.selected_caller_actor_id
            && session_id != self.selected_caller_session_id
    }
}

impl WorkspaceService {
    pub async fn get_anti_bloat_verification_material(
        &self,
        context: &RequestContext,
        review_id: Uuid,
    ) -> Result<(AntiBloatVerificationMaterial, String)> {
        if review_id.is_nil() {
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
            .anti_bloat_verification_store()
            .ok_or(Error::StorageUnavailable)?
            .anti_bloat_verification_material(workspace.id, review_id, false)
            .await?
            .ok_or(Error::NotFound)?;
        if !material.independent_of(identity.principal_id, session.id) {
            return Err(Error::Forbidden);
        }
        let evidence_digest = material.digest()?;
        tx.commit().await?;
        Ok((material, evidence_digest))
    }

    pub async fn verify_anti_bloat_apply(
        &self,
        context: &RequestContext,
        request: &VerifyAntiBloatApply,
    ) -> Result<AntiBloatPreservationAttestation> {
        if request.request_id.is_nil()
            || request.review_id.is_nil()
            || request.expected_evidence_digest.len() != 64
            || !request
                .expected_evidence_digest
                .bytes()
                .all(|byte| byte.is_ascii_hexdigit() && !byte.is_ascii_uppercase())
        {
            return Err(Error::InvalidArguments);
        }
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
            .anti_bloat_verification_store()
            .ok_or(Error::StorageUnavailable)?;
        if let Some(existing) = store
            .anti_bloat_attestation_by_request(workspace.id, request.request_id)
            .await?
        {
            if existing.review_id != request.review_id
                || existing.evidence_digest != request.expected_evidence_digest
                || existing.verifier_principal_id != identity.principal_id
                || existing.verifier_session_id != session.id
            {
                return Err(Error::InputConflict);
            }
            tx.commit().await?;
            return Ok(existing);
        }
        let material = store
            .anti_bloat_verification_material(workspace.id, request.review_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        if !material.independent_of(identity.principal_id, session.id) {
            return Err(Error::Forbidden);
        }
        if material.current_revision != material.receipt.to_revision {
            return Err(Error::StaleRevision);
        }
        if material.digest()? != request.expected_evidence_digest {
            return Err(Error::InputConflict);
        }
        let (verdict, reason) = material.verdict();
        let attestation = AntiBloatPreservationAttestation {
            request_id: request.request_id,
            workspace_id: workspace.id,
            review_id: request.review_id,
            candidate_set_id: material.receipt.candidate_set_id,
            from_revision: material.receipt.from_revision,
            to_revision: material.receipt.to_revision,
            verifier_principal_id: identity.principal_id,
            verifier_session_id: session.id,
            verdict,
            reason,
            evidence_digest: request.expected_evidence_digest.clone(),
            source_digest: material.receipt.source_digest.clone(),
            before_material_digest: material.receipt.before_material_digest.clone(),
            after_material_digest: material.receipt.after_material_digest.clone(),
            caller_request_id: material.receipt.caller_request_id,
        };
        store.append_anti_bloat_attestation(&attestation).await?;
        tx.commit().await?;
        Ok(attestation)
    }
}
