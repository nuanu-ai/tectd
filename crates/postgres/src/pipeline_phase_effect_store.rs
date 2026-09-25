use crate::{storage_error, store::PgUnitOfWork};
use async_trait::async_trait;
use serde_json::Value;
use sqlx::Row;
use tect_application::{
    PipelinePhaseEffectAttestation, PipelinePhaseEffectMaterial, PipelinePhaseEffectStore,
    PipelinePhaseEffectVerdict,
};
use tect_domain::{
    Error, PipelineDefinitionSnapshot, PipelineEvidenceRef, PipelineVerificationPlan, Result,
};
use uuid::Uuid;

#[async_trait]
impl PipelinePhaseEffectStore for PgUnitOfWork {
    async fn pipeline_phase_effect(
        &mut self,
        workspace: Uuid,
        run: Uuid,
        attempt: Uuid,
        lock: bool,
    ) -> Result<Option<PipelinePhaseEffectMaterial>> {
        let tenant = self.tenant_id()?;
        let mut sql = String::from(
            "SELECT r.slice_id,r.definition,r.definition_kind,r.definition_version,r.definition_digest,\
                r.selected_option_id,r.verification_plan_id,r.verification_plan_version,r.verification_plan_digest,\
                a.phase_id,a.attempt,a.actor_session_id,a.evidence_refs,a.id AS attempt_id,\
                o.id AS output_id,o.body_digest,o.verdict,to_jsonb(o) AS saved_output,\
                public.pipeline_phase_effect_caller(a.tenant_id,a.workspace_id,a.id) AS caller_principal_id,\
                s.opened_by_principal_id AS slice_opener_principal_id,\
                tr.recorded_by_principal_id AS matrix_owner_principal_id \
             FROM slice_pipeline_phase_attempts a \
             JOIN slice_pipeline_runs r ON (r.tenant_id,r.workspace_id,r.id)=(a.tenant_id,a.workspace_id,a.run_id) \
             JOIN slice_pipeline_phase_outputs o ON (o.tenant_id,o.workspace_id,o.run_id,o.attempt_id)=\
                (a.tenant_id,a.workspace_id,a.run_id,a.id) \
             JOIN native_slices s ON (s.tenant_id,s.workspace_id,s.id)=(r.tenant_id,r.workspace_id,r.slice_id) \
             JOIN pipeline_advice_dispositions d ON (d.tenant_id,d.workspace_id,d.disposition_id)=\
                (s.tenant_id,s.workspace_id,(s.origin_payload->>'disposition_id')::uuid) \
             JOIN pipeline_advice_contexts c ON (c.tenant_id,c.workspace_id,c.opportunity_id)=\
                (d.tenant_id,d.workspace_id,d.opportunity_id) \
             JOIN matrix_planning_effect_attestations me ON (me.tenant_id,me.workspace_id,me.id)=\
                (c.tenant_id,c.workspace_id,c.match_effect_attestation_id) \
             JOIN matrix_planning_selection_links l ON (l.tenant_id,l.workspace_id,l.candidate_set_id,l.caller_request_id)=\
                (me.tenant_id,me.workspace_id,me.candidate_set_id,me.caller_request_id) \
             JOIN matrix_task_revisions tr ON (tr.tenant_id,tr.workspace_id,tr.task_id,tr.revision)=\
                (l.tenant_id,l.workspace_id,l.task_id,l.task_revision) \
             WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.run_id=$3 AND a.id=$4 \
               AND a.outcome='completed' AND a.result_payload IS NOT NULL \
               AND NOT a.payload_erased AND NOT o.payload_erased AND NOT r.payload_erased \
               AND s.origin_result IS NOT NULL AND NOT s.payload_erased \
               AND r.slice_id=s.id AND r.selected_option_id=s.selected_option_id \
               AND r.verification_plan_id=s.verification_plan_id \
               AND r.verification_plan_version=s.verification_plan_source_definition_version \
               AND r.verification_plan_digest=s.verification_plan_digest \
               AND s.opened_by_principal_id IS NOT NULL AND d.actor_id IS NOT NULL \
               AND me.verdict='match' AND l.disposition_id=c.matrix_disposition_id \
               AND o.phase_id=a.phase_id AND o.phase_ordinal=a.phase_ordinal",
        );
        if lock {
            sql.push_str(" FOR SHARE OF a,r,o,s");
        }
        let row = sqlx::query(&sql)
            .bind(tenant)
            .bind(workspace)
            .bind(run)
            .bind(attempt)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        let Some(row) = row else {
            return Ok(None);
        };
        let definition: PipelineDefinitionSnapshot =
            serde_json::from_value(row.try_get("definition").map_err(storage_error)?)
                .map_err(|_| Error::StaleContext)?;
        let plan = PipelineVerificationPlan::from_definition(&definition)
            .map_err(|_| Error::StaleContext)?;
        let plan_id: String = row.try_get("verification_plan_id").map_err(storage_error)?;
        let plan_version: String = row
            .try_get("verification_plan_version")
            .map_err(storage_error)?;
        let plan_digest: String = row
            .try_get("verification_plan_digest")
            .map_err(storage_error)?;
        let phase_id: String = row.try_get("phase_id").map_err(storage_error)?;
        if definition.kind.as_str()
            != row
                .try_get::<String, _>("definition_kind")
                .map_err(storage_error)?
            || definition.version
                != row
                    .try_get::<String, _>("definition_version")
                    .map_err(storage_error)?
            || definition.digest
                != row
                    .try_get::<String, _>("definition_digest")
                    .map_err(storage_error)?
            || plan.id != plan_id
            || plan.source_definition_version != plan_version
            || plan.digest != plan_digest
        {
            return Err(Error::StaleContext);
        }
        let obligation = plan
            .obligations
            .into_iter()
            .find(|v| v.phase_id == phase_id)
            .ok_or(Error::StaleContext)?;
        let obligation_digest = sha(&obligation)?;
        let validator_contracts_digest = sha(&obligation.validator_contracts)?;
        let evidence: Value = row.try_get("evidence_refs").map_err(storage_error)?;
        let evidence_refs: Vec<PipelineEvidenceRef> =
            serde_json::from_value(evidence).map_err(|_| Error::StaleContext)?;
        let material = PipelinePhaseEffectMaterial {
            workspace_id: workspace,
            slice_id: row.try_get("slice_id").map_err(storage_error)?,
            run_id: run,
            attempt_id: attempt,
            phase_id,
            attempt_number: row.try_get("attempt").map_err(storage_error)?,
            selected_option_id: row.try_get("selected_option_id").map_err(storage_error)?,
            verification_plan_id: plan_id,
            verification_plan_version: plan_version,
            verification_plan_digest: plan_digest,
            obligation,
            obligation_digest,
            validator_contracts_digest,
            output_id: row.try_get("output_id").map_err(storage_error)?,
            output_digest: row.try_get("body_digest").map_err(storage_error)?,
            output: row.try_get("saved_output").map_err(storage_error)?,
            caller_verdict: row.try_get("verdict").map_err(storage_error)?,
            evidence_refs,
            caller_principal_id: row.try_get("caller_principal_id").map_err(storage_error)?,
            caller_session_id: row.try_get("actor_session_id").map_err(storage_error)?,
            slice_opener_principal_id: row
                .try_get("slice_opener_principal_id")
                .map_err(storage_error)?,
            matrix_owner_principal_id: row
                .try_get("matrix_owner_principal_id")
                .map_err(storage_error)?,
        };
        material.validate()?;
        Ok(Some(material))
    }

    async fn pipeline_phase_effect_attestation(
        &mut self,
        workspace: Uuid,
        request: Uuid,
    ) -> Result<Option<PipelinePhaseEffectAttestation>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query("SELECT run_id,attempt_id,effect_digest,output_digest,verifier_principal_id,verifier_session_id,verdict,observation,observation_digest,summary FROM pipeline_phase_effect_attestations WHERE tenant_id=$1 AND workspace_id=$2 AND verifier_request_id=$3")
            .bind(tenant).bind(workspace).bind(request).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        row.map(|row| {
            let verdict: String = row.try_get("verdict").map_err(storage_error)?;
            Ok(PipelinePhaseEffectAttestation {
                request_id: request,
                workspace_id: workspace,
                run_id: row.try_get("run_id").map_err(storage_error)?,
                attempt_id: row.try_get("attempt_id").map_err(storage_error)?,
                effect_digest: row.try_get("effect_digest").map_err(storage_error)?,
                output_digest: row.try_get("output_digest").map_err(storage_error)?,
                verifier_principal_id: row
                    .try_get("verifier_principal_id")
                    .map_err(storage_error)?,
                verifier_session_id: row.try_get("verifier_session_id").map_err(storage_error)?,
                verdict: match verdict.as_str() {
                    "pass" => PipelinePhaseEffectVerdict::Pass,
                    "fail" => PipelinePhaseEffectVerdict::Fail,
                    "unknown" => PipelinePhaseEffectVerdict::Unknown,
                    _ => return Err(Error::InternalInvariant),
                },
                observation: row.try_get("observation").map_err(storage_error)?,
                observation_digest: row.try_get("observation_digest").map_err(storage_error)?,
                summary: row.try_get("summary").map_err(storage_error)?,
            })
        })
        .transpose()
    }

    async fn append_pipeline_phase_effect_attestation(
        &mut self,
        workspace: Uuid,
        value: &PipelinePhaseEffectAttestation,
    ) -> Result<()> {
        if value.workspace_id != workspace || value.verifier_principal_id != self.principal_id()? {
            return Err(Error::Forbidden);
        }
        let material = self
            .pipeline_phase_effect(workspace, value.run_id, value.attempt_id, true)
            .await?
            .ok_or(Error::NotFound)?;
        if material.digest()? != value.effect_digest
            || material.output_digest != value.output_digest
        {
            return Err(Error::InputConflict);
        }
        let tenant = self.tenant_id()?;
        sqlx::query("INSERT INTO pipeline_phase_effect_attestations (tenant_id,workspace_id,slice_id,run_id,attempt_id,output_id,plan_id,plan_version,plan_digest,obligation_digest,validator_contracts_digest,output_digest,effect_digest,verifier_principal_id,verifier_session_id,verdict,observation,observation_digest,summary,verifier_request_id) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16,$17,$18,$19,$20)")
            .bind(tenant).bind(workspace).bind(material.slice_id).bind(value.run_id).bind(value.attempt_id).bind(material.output_id)
            .bind(material.verification_plan_id).bind(material.verification_plan_version).bind(material.verification_plan_digest)
            .bind(material.obligation_digest).bind(material.validator_contracts_digest).bind(value.output_digest.as_str()).bind(value.effect_digest.as_str())
            .bind(value.verifier_principal_id).bind(value.verifier_session_id).bind(value.verdict.as_str())
            .bind(value.observation.as_deref()).bind(value.observation_digest.as_deref()).bind(value.summary.as_str()).bind(value.request_id)
            .execute(&mut **self.transaction()?).await.map_err(|e| match e.as_database_error().and_then(|v| v.code()).as_deref() {
                Some("42501") => Error::Forbidden, Some("23505") | Some("23514") | Some("23503") => Error::InputConflict, _ => storage_error(e)
            })?;
        Ok(())
    }
}

fn sha<T: serde::Serialize + ?Sized>(value: &T) -> Result<String> {
    use sha2::{Digest, Sha256};
    Ok(format!(
        "{:x}",
        Sha256::digest(serde_json::to_vec(value).map_err(|_| Error::InternalInvariant)?)
    ))
}
