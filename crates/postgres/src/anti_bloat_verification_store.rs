use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use sqlx::Row;
use tect_application::{AntiBloatVerificationMaterial, AntiBloatVerificationStore};
use tect_domain::{
    AntiBloatDisposition, AntiBloatInput, AntiBloatPreservationAttestation,
    AntiBloatVerificationReason, AntiBloatVerificationVerdict, Error, Result,
};
use uuid::Uuid;

fn parse<T: serde::de::DeserializeOwned>(value: serde_json::Value) -> Result<T> {
    serde_json::from_value(value).map_err(storage_error)
}

#[async_trait]
impl AntiBloatVerificationStore for PgUnitOfWork {
    async fn anti_bloat_verification_material(
        &mut self,
        workspace: Uuid,
        review_id: Uuid,
        lock: bool,
    ) -> Result<Option<AntiBloatVerificationMaterial>> {
        let tenant = self.tenant_id()?;
        let mut query = String::from(
            "SELECT r.actor_id AS review_actor_id,r.input_payload,r.review_payload, \
                    d.actor_id AS selected_disposition_actor_id, \
                    c.actor_id AS selected_caller_actor_id,c.session_id AS selected_caller_session_id, \
                    l.finding_id,l.disposition,l.preservation_payload,l.delta_payload,l.after_payload, \
                    l.caller_receipt,bd.payload AS before_saved,ad.payload AS after_saved, \
                    s.revision AS current_revision \
             FROM scope_anti_bloat_caller_links l \
             JOIN scope_anti_bloat_reviews r ON (r.tenant_id,r.workspace_id,r.review_id)= \
                 (l.tenant_id,l.workspace_id,l.review_id) \
             JOIN scope_anti_bloat_bindings b ON (b.tenant_id,b.workspace_id,b.candidate_set_id,b.candidate_set_revision)= \
                 (r.tenant_id,r.workspace_id,r.candidate_set_id,r.candidate_set_revision) \
             JOIN advisory_scope_caller_link c ON (c.tenant_id,c.workspace_id,c.link_id)= \
                 (b.tenant_id,b.workspace_id,b.selected_caller_link_id) \
             JOIN advisory_scope_disposition d ON (d.tenant_id,d.workspace_id,d.disposition_id)= \
                 (c.tenant_id,c.workspace_id,c.disposition_id) \
             JOIN scope_candidate_sets s ON (s.tenant_id,s.workspace_id,s.id)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id) \
             JOIN scope_candidate_drafts bd ON (bd.tenant_id,bd.workspace_id,bd.candidate_set_id,bd.set_revision)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id,l.from_revision) \
             JOIN scope_candidate_drafts ad ON (ad.tenant_id,ad.workspace_id,ad.candidate_set_id,ad.set_revision)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id,l.to_revision) \
             JOIN scope_candidate_receipts cr ON (cr.tenant_id,cr.workspace_id,cr.candidate_set_id,cr.operation,cr.request_id,cr.result_revision)= \
                 (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_operation,l.caller_request_id,l.to_revision) \
             WHERE l.tenant_id=$1 AND l.workspace_id=$2 AND l.review_id=$3 \
               AND l.caller_operation='anti_bloat_narrow' \
               AND cr.result_payload=l.caller_receipt \
               AND b.selected_draft_revision=l.from_revision",
        );
        if lock {
            query.push_str(" FOR SHARE OF l,r,b,c,d,s,bd,ad,cr");
        }
        let row = sqlx::query(&query)
            .bind(tenant)
            .bind(workspace)
            .bind(review_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        let Some(row) = row else { return Ok(None) };
        let input: AntiBloatInput = parse(row.try_get("input_payload").map_err(storage_error)?)?;
        let source_fragments_match = match crate::scope_advisory::require_persisted_fragments(
            self.transaction()?,
            tenant,
            workspace,
            &input.manifest.source,
            &input.manifest.obligations,
        )
        .await
        {
            Ok(()) => true,
            Err(Error::InvalidSource) => false,
            Err(error) => return Err(error),
        };
        let disposition: String = row.try_get("disposition").map_err(storage_error)?;
        let material = AntiBloatVerificationMaterial {
            workspace_id: workspace,
            review_id,
            review_actor_id: row.try_get("review_actor_id").map_err(storage_error)?,
            selected_disposition_actor_id: row
                .try_get("selected_disposition_actor_id")
                .map_err(storage_error)?,
            selected_caller_actor_id: row
                .try_get("selected_caller_actor_id")
                .map_err(storage_error)?,
            selected_caller_session_id: row
                .try_get("selected_caller_session_id")
                .map_err(storage_error)?,
            input,
            review: parse(row.try_get("review_payload").map_err(storage_error)?)?,
            finding_id: row.try_get("finding_id").map_err(storage_error)?,
            disposition: match disposition.as_str() {
                "narrow" => AntiBloatDisposition::Narrow,
                _ => return Err(Error::InputConflict),
            },
            preservation: parse(row.try_get("preservation_payload").map_err(storage_error)?)?,
            delta: parse(row.try_get("delta_payload").map_err(storage_error)?)?,
            claimed_after: parse(row.try_get("after_payload").map_err(storage_error)?)?,
            receipt: parse(row.try_get("caller_receipt").map_err(storage_error)?)?,
            before_saved: parse(row.try_get("before_saved").map_err(storage_error)?)?,
            after_saved: parse(row.try_get("after_saved").map_err(storage_error)?)?,
            current_revision: row.try_get("current_revision").map_err(storage_error)?,
            source_fragments_match,
        };
        Ok(Some(material))
    }

    async fn anti_bloat_attestation_by_request(
        &mut self,
        workspace: Uuid,
        request_id: Uuid,
    ) -> Result<Option<AntiBloatPreservationAttestation>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT review_id,candidate_set_id,from_revision,to_revision,verifier_principal_id, \
                    verifier_session_id,verdict,reason,evidence_digest,source_digest, \
                    before_material_digest,after_material_digest,caller_request_id \
             FROM scope_anti_bloat_preservation_attestations \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
        )
        .bind(tenant)
        .bind(workspace)
        .bind(request_id)
        .fetch_optional(&mut **self.transaction()?)
        .await
        .map_err(storage_error)?;
        let Some(row) = row else { return Ok(None) };
        let verdict: String = row.try_get("verdict").map_err(storage_error)?;
        let reason: String = row.try_get("reason").map_err(storage_error)?;
        Ok(Some(AntiBloatPreservationAttestation {
            request_id,
            workspace_id: workspace,
            review_id: row.try_get("review_id").map_err(storage_error)?,
            candidate_set_id: row.try_get("candidate_set_id").map_err(storage_error)?,
            from_revision: row.try_get("from_revision").map_err(storage_error)?,
            to_revision: row.try_get("to_revision").map_err(storage_error)?,
            verifier_principal_id: row
                .try_get("verifier_principal_id")
                .map_err(storage_error)?,
            verifier_session_id: row.try_get("verifier_session_id").map_err(storage_error)?,
            verdict: match verdict.as_str() {
                "pass" => AntiBloatVerificationVerdict::Pass,
                "fail" => AntiBloatVerificationVerdict::Fail,
                "unknown" => AntiBloatVerificationVerdict::Unknown,
                _ => return Err(Error::InternalInvariant),
            },
            reason: match reason.as_str() {
                "full_graph_preserved" => AntiBloatVerificationReason::FullGraphPreserved,
                "graph_or_receipt_mismatch" => AntiBloatVerificationReason::GraphOrReceiptMismatch,
                "source_evidence_unavailable" => {
                    AntiBloatVerificationReason::SourceEvidenceUnavailable
                }
                _ => return Err(Error::InternalInvariant),
            },
            evidence_digest: row.try_get("evidence_digest").map_err(storage_error)?,
            source_digest: row.try_get("source_digest").map_err(storage_error)?,
            before_material_digest: row
                .try_get("before_material_digest")
                .map_err(storage_error)?,
            after_material_digest: row
                .try_get("after_material_digest")
                .map_err(storage_error)?,
            caller_request_id: row.try_get("caller_request_id").map_err(storage_error)?,
        }))
    }

    async fn append_anti_bloat_attestation(
        &mut self,
        value: &AntiBloatPreservationAttestation,
    ) -> Result<()> {
        if !self.is_read_write() || value.verifier_principal_id != self.principal_id()? {
            return Err(Error::Forbidden);
        }
        let material = self
            .anti_bloat_verification_material(value.workspace_id, value.review_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        if material.current_revision != value.to_revision
            || material.digest()? != value.evidence_digest
            || material.verdict() != (value.verdict, value.reason)
            || material.receipt.candidate_set_id != value.candidate_set_id
            || material.receipt.from_revision != value.from_revision
            || material.receipt.to_revision != value.to_revision
            || material.receipt.caller_request_id != value.caller_request_id
            || material.receipt.source_digest != value.source_digest
            || material.receipt.before_material_digest != value.before_material_digest
            || material.receipt.after_material_digest != value.after_material_digest
        {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        let (verdict, reason) = match (value.verdict, value.reason) {
            (
                AntiBloatVerificationVerdict::Pass,
                AntiBloatVerificationReason::FullGraphPreserved,
            ) => ("pass", "full_graph_preserved"),
            (
                AntiBloatVerificationVerdict::Fail,
                AntiBloatVerificationReason::GraphOrReceiptMismatch,
            ) => ("fail", "graph_or_receipt_mismatch"),
            (
                AntiBloatVerificationVerdict::Unknown,
                AntiBloatVerificationReason::SourceEvidenceUnavailable,
            ) => ("unknown", "source_evidence_unavailable"),
            _ => return Err(Error::InvalidArguments),
        };
        sqlx::query(
            "INSERT INTO scope_anti_bloat_preservation_attestations \
             (tenant_id,workspace_id,request_id,review_id,candidate_set_id,from_revision,to_revision, \
              caller_request_id,verifier_principal_id,verifier_session_id,verdict,reason, \
              evidence_digest,source_digest,before_material_digest,after_material_digest) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16)",
        )
        .bind(tenant).bind(value.workspace_id).bind(value.request_id).bind(value.review_id)
        .bind(value.candidate_set_id).bind(value.from_revision).bind(value.to_revision)
        .bind(value.caller_request_id).bind(value.verifier_principal_id).bind(value.verifier_session_id)
        .bind(verdict).bind(reason).bind(&value.evidence_digest).bind(&value.source_digest)
        .bind(&value.before_material_digest).bind(&value.after_material_digest)
        .execute(&mut **self.transaction()?).await.map_err(storage_error)?;
        Ok(())
    }
}
