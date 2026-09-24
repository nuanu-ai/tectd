use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{Row, postgres::PgRow};
use tect_application::{
    GuardedMatrixAdviceOutcome, GuardedMatrixAdviceRecord, MatrixAdviceStore,
    MatrixProviderBinding, MatrixTaskStore, StoredGuardedMatrixAdviceRecord,
    canonical_matrix_advice_digest, canonical_matrix_input_digest,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, EngineeringChoiceSet,
    EngineeringMatrixInput, Error, OwnerReportedEngineeringMatrixFacts, Result,
    compose_owner_reported_engineering_matrix, matrix_evaluation_digest,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

fn decode_advice(
    row: PgRow,
    binding: MatrixProviderBinding,
    raw: Vec<u8>,
) -> Result<StoredGuardedMatrixAdviceRecord> {
    let kind: String = row.try_get("kind").map_err(storage_error)?;
    let ranks: Option<serde_json::Value> =
        row.try_get("ranked_choice_ids").map_err(storage_error)?;
    let reason: Option<String> = row.try_get("reason").map_err(storage_error)?;
    let outcome = match (kind.as_str(), ranks, reason) {
        ("ranked", Some(ranks), None) => GuardedMatrixAdviceOutcome::Ranked {
            ranked_choice_ids: serde_json::from_value(ranks)
                .map_err(|_| Error::InternalInvariant)?,
        },
        ("abstained", None, reason) => GuardedMatrixAdviceOutcome::Abstained { reason },
        ("rejected", None, Some(reason)) => GuardedMatrixAdviceOutcome::Rejected { reason },
        _ => return Err(Error::InternalInvariant),
    };
    let profile: String = row.try_get("provider_profile_ref").map_err(storage_error)?;
    let model: serde_json::Value = row.try_get("model_configuration").map_err(storage_error)?;
    let model_configuration: AdvisoryModelConfiguration =
        serde_json::from_value(model).map_err(|_| Error::InternalInvariant)?;
    let record = GuardedMatrixAdviceRecord {
        opportunity_id: row.try_get("opportunity_id").map_err(storage_error)?,
        dispatch_id: row.try_get("dispatch_id").map_err(storage_error)?,
        opportunity_material_digest: binding.evaluation_digest.clone(),
        binding,
        provider_profile_ref: AdvisoryProviderProfileRef { id: profile },
        model_configuration,
        raw_response_payload: raw,
        response_payload_sha256: row
            .try_get("response_payload_sha256")
            .map_err(storage_error)?,
        advice_digest: row.try_get("advice_digest").map_err(storage_error)?,
        outcome,
    };
    if record.response_payload_sha256
        != format!("{:x}", Sha256::digest(&record.raw_response_payload))
        || record.advice_digest != canonical_matrix_advice_digest(&record.binding, &record.outcome)?
        || row.try_get::<Uuid, _>("task_id").map_err(storage_error)? != record.binding.task_id
        || row
            .try_get::<i64, _>("matrix_task_revision")
            .map_err(storage_error)?
            != record.binding.task_revision
        || row
            .try_get::<String, _>("matrix_choice_set_digest")
            .map_err(storage_error)?
            != record.binding.choice_set_digest
    {
        return Err(Error::InternalInvariant);
    }
    Ok(StoredGuardedMatrixAdviceRecord {
        advice_id: row.try_get("advice_id").map_err(storage_error)?,
        record,
    })
}

fn outcome_columns(
    outcome: &GuardedMatrixAdviceOutcome,
) -> (&'static str, Option<serde_json::Value>, Option<String>) {
    match outcome {
        GuardedMatrixAdviceOutcome::Ranked { ranked_choice_ids } => {
            ("ranked", Some(serde_json::json!(ranked_choice_ids)), None)
        }
        GuardedMatrixAdviceOutcome::Abstained { reason } => ("abstained", None, reason.clone()),
        GuardedMatrixAdviceOutcome::Rejected { reason } => ("rejected", None, Some(reason.clone())),
    }
}

#[async_trait]
impl MatrixAdviceStore for PgUnitOfWork {
    async fn guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        opportunity_id: Uuid,
    ) -> Result<Option<StoredGuardedMatrixAdviceRecord>> {
        let tenant = self.tenant_id()?;
        let row = sqlx::query(
            "SELECT a.advice_id,a.opportunity_id,a.dispatch_id,a.task_id,a.matrix_task_revision, \
                    a.matrix_choice_set_digest,a.kind,a.ranked_choice_ids,a.reason,a.advice_digest, \
                    a.provider_profile_ref,a.model_configuration,a.response_payload_sha256, \
                    o.material_digest,o.matrix_verification_digest,r.input_digest,r.canonical_input,r.choice_set,d.response_payload \
             FROM advisory_matrix_advice a \
             JOIN advisory_opportunity o ON (o.tenant_id,o.workspace_id,o.id)=(a.tenant_id,a.workspace_id,a.opportunity_id) \
             JOIN matrix_task_revisions r ON (r.tenant_id,r.workspace_id,r.task_id,r.revision)=(a.tenant_id,a.workspace_id,a.task_id,a.matrix_task_revision) \
             JOIN advisory_dispatch d ON (d.tenant_id,d.workspace_id,d.id)=(a.tenant_id,a.workspace_id,a.dispatch_id) \
             WHERE a.tenant_id=$1 AND a.workspace_id=$2 AND a.opportunity_id=$3"
        )
            .bind(tenant)
            .bind(workspace_id)
            .bind(opportunity_id)
            .fetch_optional(&mut **self.transaction()?)
            .await
            .map_err(storage_error)?;
        row.map(|row| {
            let choice_json: serde_json::Value =
                row.try_get("choice_set").map_err(storage_error)?;
            let choice: EngineeringChoiceSet =
                serde_json::from_value(choice_json).map_err(|_| Error::InternalInvariant)?;
            let input_json: serde_json::Value =
                row.try_get("canonical_input").map_err(storage_error)?;
            let input: EngineeringMatrixInput =
                serde_json::from_value(input_json.clone()).map_err(|_| Error::InternalInvariant)?;
            let raw: Vec<u8> = row
                .try_get::<Option<Vec<u8>>, _>("response_payload")
                .map_err(storage_error)?
                .ok_or(Error::InternalInvariant)?;
            let binding = MatrixProviderBinding {
                task_id: row.try_get("task_id").map_err(storage_error)?,
                task_revision: row.try_get("matrix_task_revision").map_err(storage_error)?,
                input_digest: row.try_get("input_digest").map_err(storage_error)?,
                choice_set_id: choice.choice_set_id.clone(),
                choice_set_version: choice.version,
                choice_set_digest: row
                    .try_get("matrix_choice_set_digest")
                    .map_err(storage_error)?,
                evaluation_digest: row.try_get("material_digest").map_err(storage_error)?,
                verification_digest: row
                    .try_get("matrix_verification_digest")
                    .map_err(storage_error)?,
            };
            if canonical_matrix_input_digest(&input_json)? != binding.input_digest
                || choice.canonical_digest(&input)? != binding.choice_set_digest
            {
                return Err(Error::InternalInvariant);
            }
            let eligibility = choice
                .validate(&input)
                .map_err(|_| Error::InternalInvariant)?;
            let advice = decode_advice(row, binding, raw)?;
            advice
                .record
                .validate_for(
                    advice.record.opportunity_id,
                    advice.record.dispatch_id,
                    &advice.record.binding,
                    &advice.record.provider_profile_ref,
                    &advice.record.model_configuration,
                    &eligibility,
                )
                .map_err(|_| Error::InternalInvariant)?;
            Ok(advice)
        })
        .transpose()
    }

    async fn persist_guarded_matrix_advice(
        &mut self,
        workspace_id: Uuid,
        record: &GuardedMatrixAdviceRecord,
    ) -> Result<StoredGuardedMatrixAdviceRecord> {
        let tenant = self.tenant_id()?;
        // Lock the occurrence and dispatch before configuration and the Matrix head.
        let opportunity = sqlx::query(
            "SELECT work_item_id,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,material_digest,config_revision,capability,decision_point,state,primary_reason \
             FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE"
        ).bind(tenant).bind(workspace_id).bind(record.opportunity_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::NotFound)?;
        let dispatch = sqlx::query(
            "SELECT opportunity_id,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,(sealed_at IS NOT NULL) AS is_sealed \
             FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 FOR UPDATE"
        ).bind(tenant).bind(workspace_id).bind(record.dispatch_id)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::InputConflict)?;
        if let Some(prior) = self
            .guarded_matrix_advice(workspace_id, record.opportunity_id)
            .await?
        {
            return if prior.record == *record {
                Ok(prior)
            } else {
                Err(Error::InputConflict)
            };
        }
        let task_id: Option<Uuid> = opportunity.try_get("work_item_id").map_err(storage_error)?;
        let revision: Option<i64> = opportunity
            .try_get("matrix_task_revision")
            .map_err(storage_error)?;
        let digest: Option<String> = opportunity
            .try_get("matrix_choice_set_digest")
            .map_err(storage_error)?;
        let verification_digest: Option<String> = opportunity
            .try_get("matrix_verification_digest")
            .map_err(storage_error)?;
        let material: String = opportunity
            .try_get("material_digest")
            .map_err(storage_error)?;
        let config_revision: i64 = opportunity
            .try_get("config_revision")
            .map_err(storage_error)?;
        let snapshot: serde_json::Value = dispatch
            .try_get("configuration_snapshot")
            .map_err(storage_error)?;
        let request_payload: Vec<u8> =
            dispatch.try_get("request_payload").map_err(storage_error)?;
        let response_payload: Option<Vec<u8>> = dispatch
            .try_get("response_payload")
            .map_err(storage_error)?;
        let response_sha = format!("{:x}", Sha256::digest(&record.raw_response_payload));
        let config_sha = format!(
            "{:x}",
            Sha256::digest(serde_json::to_vec(&snapshot).map_err(storage_error)?)
        );
        if task_id != Some(record.binding.task_id)
            || revision != Some(record.binding.task_revision)
            || digest.as_deref() != Some(record.binding.choice_set_digest.as_str())
            || verification_digest != record.binding.verification_digest
            || material != record.binding.evaluation_digest
            || material != record.opportunity_material_digest
            || opportunity
                .try_get::<String, _>("capability")
                .map_err(storage_error)?
                != "engineering_profile"
            || opportunity
                .try_get::<String, _>("decision_point")
                .map_err(storage_error)?
                != "engineering.profile.before_selection"
            || dispatch
                .try_get::<Uuid, _>("opportunity_id")
                .map_err(storage_error)?
                != record.opportunity_id
            || dispatch
                .try_get::<String, _>("material_digest")
                .map_err(storage_error)?
                != material
            || dispatch
                .try_get::<String, _>("provider")
                .map_err(storage_error)?
                != record.provider_profile_ref.id
            || dispatch
                .try_get::<String, _>("model")
                .map_err(storage_error)?
                != record.model_configuration.model
            || dispatch
                .try_get::<String, _>("configuration_digest")
                .map_err(storage_error)?
                != config_sha
            || dispatch
                .try_get::<String, _>("payload_digest")
                .map_err(storage_error)?
                != format!("{:x}", Sha256::digest(&request_payload))
            || snapshot.get("provider_profile_ref")
                != Some(&serde_json::json!(record.provider_profile_ref))
            || snapshot.get("model_configuration")
                != Some(&serde_json::json!(record.model_configuration))
            || snapshot.get("request_body_length")
                != Some(&serde_json::json!(request_payload.len()))
            || snapshot.get("request_body_sha256")
                != Some(&serde_json::json!(format!(
                    "{:x}",
                    Sha256::digest(&request_payload)
                )))
            || dispatch
                .try_get::<String, _>("state")
                .map_err(storage_error)?
                != "sealed"
            || dispatch
                .try_get::<String, _>("send_certainty")
                .map_err(storage_error)?
                != "sent"
            || !dispatch
                .try_get::<bool, _>("is_sealed")
                .map_err(storage_error)?
            || response_payload.as_deref() != Some(record.raw_response_payload.as_slice())
            || record.response_payload_sha256 != response_sha
        {
            return Err(Error::InputConflict);
        }

        let usable = !matches!(record.outcome, GuardedMatrixAdviceOutcome::Rejected { .. });
        let expected_outcome = if usable {
            "provider_response"
        } else {
            "provider_failure"
        };
        let expected_state = if usable { "advised" } else { "failed" };
        let expected_reason = if usable {
            "provider_response"
        } else {
            "provider_failure"
        };
        if dispatch
            .try_get::<Option<String>, _>("outcome")
            .map_err(storage_error)?
            .as_deref()
            != Some(expected_outcome)
            || opportunity
                .try_get::<String, _>("state")
                .map_err(storage_error)?
                != expected_state
            || opportunity
                .try_get::<String, _>("primary_reason")
                .map_err(storage_error)?
                != expected_reason
        {
            return Err(Error::InputConflict);
        }

        let config = sqlx::query("SELECT revision,mode,provider_profile_ref,model_configuration FROM advisory_workspace_config WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
            .bind(tenant).bind(workspace_id).fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?
            .ok_or(Error::StaleRevision)?;
        let current = self
            .lock_matrix_task(workspace_id, record.binding.task_id)
            .await?
            .ok_or(Error::StaleRevision)?;
        if current.revision != record.binding.task_revision
            || current.input_digest != record.binding.input_digest
            || current.choice_set_digest.as_deref()
                != Some(record.binding.choice_set_digest.as_str())
            || config
                .try_get::<i64, _>("revision")
                .map_err(storage_error)?
                != config_revision
            || config.try_get::<String, _>("mode").map_err(storage_error)? != "optional"
            || config
                .try_get::<Option<String>, _>("provider_profile_ref")
                .map_err(storage_error)?
                .as_deref()
                != Some(record.provider_profile_ref.id.as_str())
            || config
                .try_get::<Option<serde_json::Value>, _>("model_configuration")
                .map_err(storage_error)?
                != Some(serde_json::json!(record.model_configuration))
        {
            return Err(Error::StaleRevision);
        }
        let choice = current.choice_set.as_ref().ok_or(Error::InputConflict)?;
        let eligibility = choice.validate(&current.input)?;
        let canonical = serde_json::to_value(&current.input).map_err(storage_error)?;
        if canonical_matrix_input_digest(&canonical)? != record.binding.input_digest
            || choice.choice_set_id != record.binding.choice_set_id
            || choice.version != record.binding.choice_set_version
            || choice.canonical_digest(&current.input)? != record.binding.choice_set_digest
        {
            return Err(Error::InputConflict);
        }
        let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
            current.task_id.to_string(),
            current.revision.to_string(),
            current.input.clone(),
        )?;
        let composition = compose_owner_reported_engineering_matrix(&reported);
        if matrix_evaluation_digest(&current.input, &composition, choice)?.as_deref()
            != Some(record.binding.evaluation_digest.as_str())
        {
            return Err(Error::InputConflict);
        }
        record.validate_for(
            record.opportunity_id,
            record.dispatch_id,
            &record.binding,
            &record.provider_profile_ref,
            &record.model_configuration,
            &eligibility,
        )?;
        let (kind, ranks, reason) = outcome_columns(&record.outcome);
        let inserted: Option<Uuid> = sqlx::query_scalar(
            "INSERT INTO advisory_matrix_advice (tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision,matrix_choice_set_digest,dispatch_id,kind,ranked_choice_ids,reason,advice_digest,provider_profile_ref,model_configuration,response_payload_sha256) \
             VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14) ON CONFLICT DO NOTHING RETURNING advice_id"
        ).bind(tenant).bind(workspace_id).bind(record.opportunity_id).bind(record.binding.task_id)
            .bind(record.binding.task_revision).bind(&record.binding.choice_set_digest)
            .bind(record.dispatch_id).bind(kind).bind(ranks).bind(reason)
            .bind(&record.advice_digest).bind(&record.provider_profile_ref.id)
            .bind(serde_json::json!(record.model_configuration)).bind(&record.response_payload_sha256)
            .fetch_optional(&mut **self.transaction()?).await.map_err(storage_error)?;
        let advice_id = inserted.ok_or(Error::InputConflict)?;
        Ok(StoredGuardedMatrixAdviceRecord {
            advice_id,
            record: record.clone(),
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn rejected_outcome_has_no_ranking_or_usable_kind() {
        let (kind, ranks, reason) = outcome_columns(&GuardedMatrixAdviceOutcome::Rejected {
            reason: "guard rejected response".into(),
        });
        assert_eq!(kind, "rejected");
        assert!(ranks.is_none());
        assert_eq!(reason.as_deref(), Some("guard rejected response"));
    }
}
