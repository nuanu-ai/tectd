use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    PipelineOpenEffectAttestation, PipelineOpenEffectMaterial, PipelineOpenEffectStore,
    PipelineOpenEffectVerdict,
};
use tect_domain::{
    Error, OpenSlice, OpenSliceOutcome, PipelineDispositionResult, ResolvedSliceCandidateDraft,
    Result,
};
use uuid::Uuid;

#[async_trait]
impl PipelineOpenEffectStore for PgUnitOfWork {
    async fn pipeline_open_effect(
        &mut self,
        workspace_id: Uuid,
        slice_id: Uuid,
        open_request_id: Uuid,
        for_update: bool,
    ) -> Result<Option<PipelineOpenEffectMaterial>> {
        let tenant = self.tenant_id()?;
        let mut sql = String::from(
            "SELECT s.scope_id,s.candidate_id,s.candidate_revision,s.opening_snapshot_id,s.pipeline,\
                    s.origin_payload,s.origin_result,d.result_payload,d.work_node_id,\
                    d.work_node_revision,d.source_snapshot_id,d.matrix_disposition_id,d.manifest_digest,\
                    d.actor_id,d.session_id,c.source_snapshot_id AS context_source_snapshot_id,\
                    c.candidate_set_id AS context_candidate_set_id,c.opportunity_id AS context_opportunity_id,\
                    c.source_snapshot_digest,c.match_effect_attestation_id,\
                    r.recorded_by_principal_id,dr.payload AS saved_draft \
             FROM native_slices s \
             JOIN pipeline_advice_dispositions d ON (d.tenant_id,d.workspace_id,d.disposition_id)=\
                  (s.tenant_id,s.workspace_id,(s.origin_payload->>'disposition_id')::uuid) \
             JOIN pipeline_advice_contexts c ON (c.tenant_id,c.workspace_id,c.opportunity_id)=\
                  (d.tenant_id,d.workspace_id,d.opportunity_id) \
             JOIN matrix_planning_effect_attestations e ON (e.tenant_id,e.workspace_id,e.id)=\
                  (c.tenant_id,c.workspace_id,c.match_effect_attestation_id) \
             JOIN matrix_planning_selection_links l ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=\
                  (e.tenant_id,e.workspace_id,e.candidate_set_id,e.caller_request_id) \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=\
                  (l.tenant_id,l.workspace_id,l.task_id,l.task_revision) \
             JOIN slice_candidate_drafts dr ON (dr.tenant_id,dr.workspace_id,dr.candidate_set_id,dr.set_revision)=\
                  (e.tenant_id,e.workspace_id,e.candidate_set_id,e.result_revision) \
             WHERE s.tenant_id=$1 AND s.workspace_id=$2 AND s.id=$3 AND s.origin_request_id=$4 \
               AND e.verdict='match' AND l.disposition_id=c.matrix_disposition_id \
               AND e.candidate_set_id=c.candidate_set_id",
        );
        if for_update {
            sql.push_str(" FOR SHARE OF s,d,c,e,l,r,dr");
        }
        let row = sqlx::query(&sql)
            .bind(tenant)
            .bind(workspace_id)
            .bind(slice_id)
            .bind(open_request_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        let Some(row) = row else { return Ok(None) };
        let payload: Value = row.try_get("origin_payload").map_err(storage_error)?;
        let request: OpenSlice =
            serde_json::from_value(payload).map_err(|_| Error::StaleContext)?;
        let receipt: OpenSliceOutcome = serde_json::from_value(
            row.try_get::<Value, _>("origin_result")
                .map_err(storage_error)?,
        )
        .map_err(|_| Error::StaleContext)?;
        let slice = match &receipt {
            OpenSliceOutcome::Created(value) => value.clone(),
            _ => return Err(Error::StaleContext),
        };
        let disposition: PipelineDispositionResult = serde_json::from_value(
            row.try_get::<Value, _>("result_payload")
                .map_err(storage_error)?,
        )
        .map_err(|_| Error::StaleContext)?;
        let draft: ResolvedSliceCandidateDraft = serde_json::from_value(
            row.try_get::<Value, _>("saved_draft")
                .map_err(storage_error)?,
        )
        .map_err(|_| Error::StaleContext)?;
        let work = draft
            .nodes
            .into_iter()
            .find(|n| n.id() == slice.candidate_id)
            .ok_or(Error::StaleContext)?;
        let source_snapshot_id: Uuid = row.try_get("source_snapshot_id").map_err(storage_error)?;
        let matrix_disposition_id: Uuid = row
            .try_get("matrix_disposition_id")
            .map_err(storage_error)?;
        if slice.id != slice_id
            || request.request_id != open_request_id
            || slice.scope_id != row.try_get::<Uuid, _>("scope_id").map_err(storage_error)?
            || slice.candidate_id
                != row
                    .try_get::<Uuid, _>("candidate_id")
                    .map_err(storage_error)?
            || slice.candidate_revision
                != row
                    .try_get::<i64, _>("candidate_revision")
                    .map_err(storage_error)?
            || slice.opening_snapshot_id
                != row
                    .try_get::<Uuid, _>("opening_snapshot_id")
                    .map_err(storage_error)?
            || slice.pipeline.as_str()
                != row
                    .try_get::<String, _>("pipeline")
                    .map_err(storage_error)?
            || request.candidate_set_id != c_id(&row)?
            || disposition.id != request.disposition_id.ok_or(Error::StaleContext)?
            || disposition.request.opportunity_id != c_opportunity(&row)?
            || disposition.work_id
                != row
                    .try_get::<Uuid, _>("work_node_id")
                    .map_err(storage_error)?
            || slice.candidate_revision
                != row
                    .try_get::<i64, _>("work_node_revision")
                    .map_err(storage_error)?
            || source_snapshot_id
                != row
                    .try_get::<Uuid, _>("context_source_snapshot_id")
                    .map_err(storage_error)?
            || disposition.request.manifest_digest
                != row
                    .try_get::<String, _>("manifest_digest")
                    .map_err(storage_error)?
        {
            return Err(Error::StaleContext);
        }
        let material = PipelineOpenEffectMaterial {
            workspace_id,
            slice,
            open_request: request,
            open_receipt: receipt,
            disposition,
            work,
            source_snapshot_id,
            source_snapshot_digest: row
                .try_get("source_snapshot_digest")
                .map_err(storage_error)?,
            matrix_disposition_id,
            matrix_effect_attestation_id: row
                .try_get("match_effect_attestation_id")
                .map_err(storage_error)?,
            manifest_digest: row.try_get("manifest_digest").map_err(storage_error)?,
            matrix_owner_principal_id: row
                .try_get("recorded_by_principal_id")
                .map_err(storage_error)?,
            caller_principal_id: row.try_get("actor_id").map_err(storage_error)?,
            caller_session_id: row.try_get("session_id").map_err(storage_error)?,
        };
        material.validate()?;
        Ok(Some(material))
    }

    async fn pipeline_open_effect_attestation(
        &mut self,
        workspace_id: Uuid,
        request_id: Uuid,
    ) -> Result<Option<PipelineOpenEffectAttestation>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query("SELECT slice_id,open_request_id,effect_digest,verifier_principal_id,verifier_session_id,verdict,summary FROM pipeline_open_effect_attestations WHERE tenant_id=$1 AND workspace_id=$2 AND verifier_request_id=$3")
            .bind(tenant).bind(workspace_id).bind(request_id).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        row.map(|row| {
            let verdict: String = row.try_get("verdict").map_err(storage_error)?;
            Ok(PipelineOpenEffectAttestation {
                request_id,
                workspace_id,
                slice_id: row.try_get("slice_id").map_err(storage_error)?,
                open_request_id: row.try_get("open_request_id").map_err(storage_error)?,
                effect_digest: row.try_get("effect_digest").map_err(storage_error)?,
                verifier_principal_id: row
                    .try_get("verifier_principal_id")
                    .map_err(storage_error)?,
                verifier_session_id: row.try_get("verifier_session_id").map_err(storage_error)?,
                verdict: match verdict.as_str() {
                    "match" => PipelineOpenEffectVerdict::Matches,
                    "reject" => PipelineOpenEffectVerdict::Rejects,
                    _ => return Err(Error::InternalInvariant),
                },
                summary: row.try_get("summary").map_err(storage_error)?,
            })
        })
        .transpose()
    }

    async fn append_pipeline_open_effect_attestation(
        &mut self,
        workspace_id: Uuid,
        attestation: &PipelineOpenEffectAttestation,
    ) -> Result<()> {
        if attestation.workspace_id != workspace_id
            || attestation.verifier_principal_id != self.principal_id()?
        {
            return Err(Error::Forbidden);
        }
        let current = self
            .pipeline_open_effect(
                workspace_id,
                attestation.slice_id,
                attestation.open_request_id,
                true,
            )
            .await?
            .ok_or(Error::NotFound)?;
        if current.digest()? != attestation.effect_digest {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        sqlx::query("INSERT INTO pipeline_open_effect_attestations (tenant_id,workspace_id,slice_id,open_request_id,effect_digest,verifier_principal_id,verifier_session_id,verdict,summary,verifier_request_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10)")
            .bind(tenant).bind(workspace_id).bind(attestation.slice_id).bind(attestation.open_request_id)
            .bind(&attestation.effect_digest).bind(attestation.verifier_principal_id).bind(attestation.verifier_session_id)
            .bind(match attestation.verdict { PipelineOpenEffectVerdict::Matches => "match", PipelineOpenEffectVerdict::Rejects => "reject" })
            .bind(&attestation.summary).bind(attestation.request_id)
            .execute(&mut **self.transaction()?).await.map_err(|e| match e.as_database_error().and_then(|v| v.code()).as_deref() { Some("42501") => Error::Forbidden, Some("23505") | Some("23514") => Error::InputConflict, _ => storage_error(e) })?;
        Ok(())
    }
}

fn c_id(row: &sqlx::postgres::PgRow) -> Result<Uuid> {
    row.try_get("context_candidate_set_id")
        .map_err(storage_error)
}
fn c_opportunity(row: &sqlx::postgres::PgRow) -> Result<Uuid> {
    row.try_get("context_opportunity_id").map_err(storage_error)
}
