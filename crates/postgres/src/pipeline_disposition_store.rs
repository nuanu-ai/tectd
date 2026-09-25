use serde_json::Value;
use sha2::{Digest, Sha256};
use sqlx::Row;
use tect_application::{
    PipelineDispositionBasis, PipelineRecommendationStore, pipeline_recommendation_source_digest,
};
use tect_domain::{
    AdvisoryOpportunityState, Error, OpenSlice, PipelineDispositionAdvice,
    PipelineDispositionResult, PipelineKind, PipelineRecommendationRanking, Result,
    SliceCandidateNode,
};
use uuid::Uuid;

use crate::{storage_error, store::PgUnitOfWork};

fn write_error(error: sqlx::Error) -> Error {
    match error.as_database_error().and_then(|e| e.code()) {
        Some(code) if code == "42501" => Error::Forbidden,
        Some(code) if code == "23505" || code == "23514" => Error::InputConflict,
        _ => storage_error(error),
    }
}

fn sealed_advice(
    dispatch_id: Uuid,
    bytes: &[u8],
    saved_digest: &str,
    manifest: &tect_domain::PipelineRecommendationManifest,
) -> Result<PipelineDispositionAdvice> {
    let actual = format!("{:x}", Sha256::digest(bytes));
    if actual != saved_digest || bytes.is_empty() || bytes.len() > 65536 {
        return Err(Error::InputConflict);
    }
    // The durable disposition accepts only the exact typed ranking wire
    // shape. A provider envelope needs its own saved parser contract first.
    let ranking: PipelineRecommendationRanking =
        serde_json::from_slice(bytes).map_err(|_| Error::InputConflict)?;
    ranking
        .validate(manifest)
        .map_err(|_| Error::InputConflict)?;
    Ok(match ranking {
        PipelineRecommendationRanking::Ranked { ranked_ids } => PipelineDispositionAdvice::Ranked {
            dispatch_id,
            ranked_ids,
        },
        PipelineRecommendationRanking::Abstained => {
            PipelineDispositionAdvice::Abstained { dispatch_id }
        }
    })
}

pub(crate) async fn by_opportunity(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    opportunity_id: Uuid,
) -> Result<Option<PipelineDispositionResult>> {
    let tenant = store.tenant_id()?;
    let row = sqlx::query(
        "SELECT result_payload FROM pipeline_advice_dispositions \
         WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(opportunity_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    row.map(|row| {
        serde_json::from_value(
            row.try_get::<Value, _>("result_payload")
                .map_err(storage_error)?,
        )
        .map_err(|_| Error::InputConflict)
    })
    .transpose()
}

/// Resolve a captured planning choice inside the same transaction that opens
/// the Slice. The currentness read locks the Work/Matrix/source basis until the
/// open insert; no provider content or caller-supplied kind is trusted here.
pub(crate) async fn selection_for_open(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    session_id: Uuid,
    request: &OpenSlice,
    replay: bool,
) -> Result<PipelineKind> {
    let disposition_id = request.disposition_id.ok_or(Error::InvalidArguments)?;
    let tenant = store.tenant_id()?;
    let actor_id = store.principal_id()?;
    let row = sqlx::query(
        "SELECT result_payload,opportunity_id,actor_id,session_id,work_node_id,\
                work_node_revision,manifest_digest,matrix_disposition_id,source_snapshot_id, \
                selected_option_id,verification_plan_id,verification_plan_version, \
                verification_plan_digest,verification_plan_source_definition_digest \
         FROM pipeline_advice_dispositions WHERE tenant_id=$1 AND workspace_id=$2 \
           AND disposition_id=$3",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(disposition_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?
    .ok_or(Error::NotFound)?;
    let result: PipelineDispositionResult = serde_json::from_value(
        row.try_get::<Value, _>("result_payload")
            .map_err(storage_error)?,
    )
    .map_err(|_| Error::InputConflict)?;
    let opportunity_id: Uuid = row.try_get("opportunity_id").map_err(storage_error)?;
    if result.id != disposition_id
        || result.request.opportunity_id != opportunity_id
        || row.try_get::<Uuid, _>("actor_id").map_err(storage_error)? != actor_id
        || row
            .try_get::<Uuid, _>("session_id")
            .map_err(storage_error)?
            != session_id
        || row
            .try_get::<Uuid, _>("work_node_id")
            .map_err(storage_error)?
            != request.candidate_id
        || row
            .try_get::<i64, _>("work_node_revision")
            .map_err(storage_error)?
            != request.candidate_revision
    {
        return Err(Error::Forbidden);
    }
    let selected = result.selected_kind.ok_or(Error::Forbidden)?;
    let selected_option_id = result
        .selected_option_id
        .as_deref()
        .ok_or(Error::Forbidden)?;
    let persisted_option_id: Option<String> =
        row.try_get("selected_option_id").map_err(storage_error)?;
    let plan_id: Option<String> = row.try_get("verification_plan_id").map_err(storage_error)?;
    let plan_version: Option<String> = row
        .try_get("verification_plan_version")
        .map_err(storage_error)?;
    let plan_digest: Option<String> = row
        .try_get("verification_plan_digest")
        .map_err(storage_error)?;
    let source_definition_digest: Option<String> = row
        .try_get("verification_plan_source_definition_digest")
        .map_err(storage_error)?;
    if persisted_option_id.as_deref() != Some(selected_option_id)
        || plan_id.as_deref() != selected_option_id.split_once('+').map(|(_, id)| id)
        || !selected_option_id.starts_with(&format!("{}+", selected.as_str()))
        || plan_digest.as_deref()
            != plan_id
                .as_deref()
                .and_then(|id| id.strip_prefix("verification-plan:"))
        || plan_version.as_deref().is_none_or(str::is_empty)
        || source_definition_digest
            .as_deref()
            .is_none_or(str::is_empty)
    {
        return Err(Error::InputConflict);
    }
    if replay {
        return Ok(selected);
    }
    let basis = load_basis(store, workspace_id, opportunity_id)
        .await?
        .ok_or(Error::StaleContext)?;
    let context = &basis.prepared.context;
    let manifest = &basis.prepared.manifest;
    let opportunity = &basis.prepared.opportunity;
    if !is_current(store, workspace_id, &basis).await?
        || opportunity.authorized_actor_id != actor_id
        || opportunity.session_id != session_id
        || context.scope_id != request.scope_id
        || context.candidate_set_id != request.candidate_set_id
        || context.candidate_set_revision != request.candidate_set_revision
        || context.planning_snapshot_id != request.candidate_snapshot_id
        || context.work_node_id != request.candidate_id
        || context.work_node_revision != request.candidate_revision
        || context.matrix_disposition_id
            != row
                .try_get::<Uuid, _>("matrix_disposition_id")
                .map_err(storage_error)?
        || context.source_snapshot_id
            != row
                .try_get::<Uuid, _>("source_snapshot_id")
                .map_err(storage_error)?
        || manifest.digest
            != row
                .try_get::<String, _>("manifest_digest")
                .map_err(storage_error)?
        || result
            .request
            .resolve(result.id, manifest, &basis.saved_work, &basis.advice)?
            != result
        || !manifest.options.iter().any(|option| {
            option.id == selected_option_id
                && option.kind == selected
                && Some(option.verification_plan.id.as_str()) == plan_id.as_deref()
                && Some(option.verification_plan.digest.as_str()) == plan_digest.as_deref()
                && Some(option.verification_plan.source_definition_version.as_str())
                    == plan_version.as_deref()
                && Some(option.verification_plan.source_definition_digest.as_str())
                    == source_definition_digest.as_deref()
        })
    {
        return Err(Error::StaleContext);
    }
    Ok(selected)
}

pub(crate) async fn load_basis(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    opportunity_id: Uuid,
) -> Result<Option<PipelineDispositionBasis>> {
    let tenant = store.tenant_id()?;
    let row = sqlx::query(
        "SELECT o.state,d.id AS dispatch_id,d.state AS dispatch_state, \
                d.send_certainty,d.outcome,d.response_payload, \
                d.pipeline_response_sha256 \
         FROM advisory_opportunity o LEFT JOIN advisory_dispatch d ON \
              (d.tenant_id,d.workspace_id,d.opportunity_id)= \
              (o.tenant_id,o.workspace_id,o.id) \
         WHERE o.tenant_id=$1 AND o.workspace_id=$2 AND o.id=$3 \
           AND o.capability='pipeline_recommendation' FOR UPDATE OF o",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(opportunity_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    let Some(row) = row else { return Ok(None) };
    let prepared = store
        .pipeline_recommendation_by_opportunity(workspace_id, opportunity_id)
        .await?
        .ok_or(Error::InputConflict)?;
    let dispatch_id: Option<Uuid> = row.try_get("dispatch_id").map_err(storage_error)?;
    let state: String = row.try_get("state").map_err(storage_error)?;
    let advice = match (state.as_str(), dispatch_id) {
        ("no_call", None) => PipelineDispositionAdvice::NoCall,
        ("awaiting_response" | "advised", Some(dispatch_id)) => {
            let dispatch_state: Option<String> =
                row.try_get("dispatch_state").map_err(storage_error)?;
            let certainty: Option<String> = row.try_get("send_certainty").map_err(storage_error)?;
            let outcome: Option<String> = row.try_get("outcome").map_err(storage_error)?;
            if dispatch_state.as_deref() != Some("sealed")
                || certainty.as_deref() != Some("sent")
                || outcome.as_deref() != Some("provider_response")
            {
                return Err(Error::InputConflict);
            }
            let bytes: Vec<u8> = row
                .try_get::<Option<Vec<u8>>, _>("response_payload")
                .map_err(storage_error)?
                .ok_or(Error::InputConflict)?;
            let digest: String = row
                .try_get::<Option<String>, _>("pipeline_response_sha256")
                .map_err(storage_error)?
                .ok_or(Error::InputConflict)?;
            sealed_advice(dispatch_id, &bytes, &digest, &prepared.manifest)?
        }
        _ => return Err(Error::InputConflict),
    };
    let context = &prepared.context;
    let basis = store
        .load_pipeline_recommendation_basis(
            workspace_id,
            context.candidate_set_id,
            context.work_node_id,
            true,
        )
        .await?
        .ok_or(Error::StaleContext)?;
    let saved_work = basis.source.work;
    Ok(Some(PipelineDispositionBasis {
        prepared,
        saved_work,
        advice,
    }))
}

pub(crate) async fn is_current(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    saved: &PipelineDispositionBasis,
) -> Result<bool> {
    let opportunity = &saved.prepared.opportunity;
    let context = &saved.prepared.context;
    let manifest = &saved.prepared.manifest;
    let current = store
        .pipeline_recommendation_by_opportunity(workspace_id, opportunity.id)
        .await?;
    let Some(current) = current else {
        return Ok(false);
    };
    if current != saved.prepared
        || manifest.validate_digest().is_err()
        || opportunity.material_digest != manifest.digest
        || context.verification_contract_digest != manifest.digest
        || context.compatibility_policy_digest != manifest.compatibility_policy_digest
        || context.eligible_option_ids
            != manifest
                .options
                .iter()
                .map(|option| option.id.clone())
                .collect::<Vec<_>>()
        || !matches!(
            (&saved.advice, opportunity.state),
            (
                PipelineDispositionAdvice::NoCall,
                AdvisoryOpportunityState::NoCall
            ) | (
                PipelineDispositionAdvice::Ranked { .. }
                    | PipelineDispositionAdvice::Abstained { .. },
                AdvisoryOpportunityState::AwaitingResponse | AdvisoryOpportunityState::Advised
            )
        )
    {
        return Ok(false);
    }
    let tenant = store.tenant_id()?;
    let revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM advisory_workspace_config \
         WHERE tenant_id=$1 AND workspace_id=$2 FOR SHARE",
    )
    .bind(tenant)
    .bind(workspace_id)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(storage_error)?;
    if revision != Some(opportunity.config_revision) {
        return Ok(false);
    }
    let basis = match store
        .load_pipeline_recommendation_basis(
            workspace_id,
            context.candidate_set_id,
            context.work_node_id,
            true,
        )
        .await
    {
        Ok(Some(basis)) => basis,
        Ok(None) | Err(Error::StaleContext) => return Ok(false),
        Err(error) => return Err(error),
    };
    let source = &basis.source;
    let SliceCandidateNode::Work { revision, .. } = &source.work else {
        return Ok(false);
    };
    Ok(saved.saved_work == source.work
        && basis.scope_id == context.scope_id
        && basis.candidate_set_id == context.candidate_set_id
        && basis.candidate_set_revision == context.candidate_set_revision
        && basis.planning_snapshot_id == context.planning_snapshot_id
        && basis.source_snapshot_id == context.source_snapshot_id
        && basis.source_candidate_set_revision.to_string() == context.source_snapshot_revision
        && pipeline_recommendation_source_digest(
            basis.source_snapshot_id,
            basis.source_candidate_set_revision,
            &basis.selected_sources_digest,
        )? == context.source_snapshot_digest
        && *revision == context.work_node_revision
        && basis.matrix_disposition_id == context.matrix_disposition_id
        && basis.match_effect_attestation_id == context.match_effect_attestation_id
        && source.catalogue.revision == context.catalogue_revision
        && source.catalogue.digest == context.catalogue_digest
        && manifest.matrix_task_id == source.matrix.composition.task_id
        && manifest.matrix_task_revision == source.matrix.composition.task_revision
        && manifest.selected_choice_id == source.matrix.selected_choice_id
        && manifest.matrix_choice_set_digest == source.matrix.choice_set_digest
        && manifest.matrix_verification_digest == source.matrix.verification_digest
        && manifest.mandatory_card_ids == source.matrix.saved_mandatory_card_ids)
}

pub(crate) async fn capture(
    store: &mut PgUnitOfWork,
    workspace_id: Uuid,
    result: &PipelineDispositionResult,
) -> Result<PipelineDispositionResult> {
    let tenant = store.tenant_id()?;
    let prepared = store
        .pipeline_recommendation_by_opportunity(workspace_id, result.request.opportunity_id)
        .await?
        .ok_or(Error::InputConflict)?;
    let (kind, dispatch_id) = match &result.advice {
        PipelineDispositionAdvice::NoCall => ("no_call", None),
        PipelineDispositionAdvice::Ranked { dispatch_id, .. } => ("ranked", Some(*dispatch_id)),
        PipelineDispositionAdvice::Abstained { dispatch_id } => ("abstained", Some(*dispatch_id)),
    };
    let basis = load_basis(store, workspace_id, result.request.opportunity_id)
        .await?
        .ok_or(Error::InputConflict)?;
    if !is_current(store, workspace_id, &basis).await?
        || basis.advice != result.advice
        || result.request.resolve(
            result.id,
            &prepared.manifest,
            &basis.saved_work,
            &basis.advice,
        )? != *result
    {
        return Err(Error::InputConflict);
    }
    let inserted = sqlx::query_scalar::<_, Uuid>(
        "INSERT INTO pipeline_advice_dispositions \
         (tenant_id,workspace_id,disposition_id,opportunity_id,request_id,actor_id,session_id, \
          work_node_id,work_node_revision,manifest_digest,matrix_disposition_id,source_snapshot_id, \
          advice_kind,dispatch_id,request_payload,result_payload) VALUES \
         ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13,$14,$15,$16) \
         ON CONFLICT DO NOTHING RETURNING disposition_id",
    )
    .bind(tenant)
    .bind(workspace_id)
    .bind(result.id)
    .bind(result.request.opportunity_id)
    .bind(result.request.request_id)
    .bind(prepared.opportunity.authorized_actor_id)
    .bind(prepared.opportunity.session_id)
    .bind(result.work_id)
    .bind(result.request.expected_work_revision)
    .bind(&result.request.manifest_digest)
    .bind(prepared.context.matrix_disposition_id)
    .bind(prepared.context.source_snapshot_id)
    .bind(kind)
    .bind(dispatch_id)
    .bind(serde_json::to_value(&result.request).map_err(|_| Error::InvalidArguments)?)
    .bind(serde_json::to_value(result).map_err(|_| Error::InvalidArguments)?)
    .fetch_optional(&mut **store.transaction()?)
    .await
    .map_err(write_error)?;
    if inserted.is_none() {
        // The opportunity and request identities may conflict independently.
        let by_request: Option<Value> = sqlx::query_scalar(
            "SELECT result_payload FROM pipeline_advice_dispositions \
             WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3",
        )
        .bind(tenant)
        .bind(workspace_id)
        .bind(result.request.request_id)
        .fetch_optional(&mut **store.transaction()?)
        .await
        .map_err(storage_error)?;
        if let Some(saved) = by_request
            .and_then(|value| serde_json::from_value::<PipelineDispositionResult>(value).ok())
        {
            if saved.request == result.request
                && saved.work_id == result.work_id
                && saved.advice == result.advice
                && saved.selected_kind == result.selected_kind
                && saved.selected_option_id == result.selected_option_id
            {
                return Ok(saved);
            }
        }
        return Err(Error::InputConflict);
    }
    by_opportunity(store, workspace_id, result.request.opportunity_id)
        .await?
        .filter(|saved| saved == result)
        .ok_or(Error::InputConflict)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sealed_parser_rejects_non_json_and_bad_digest() {
        let bytes = b"not json";
        let manifest = tect_domain::PipelineRecommendationManifest {
            schema: String::new(),
            work_id: Uuid::nil(),
            work_revision: 0,
            matrix_task_id: String::new(),
            matrix_task_revision: String::new(),
            selected_choice_id: String::new(),
            matrix_choice_set_digest: String::new(),
            matrix_verification_digest: String::new(),
            matrix_input_digest: String::new(),
            selected_candidate_digest: String::new(),
            compatibility_policy_digest: String::new(),
            mandatory_card_ids: vec![],
            deterministic_kind: PipelineKind::LightweightTddDevelopment,
            deterministic_option_id: None,
            catalogue_revision: String::new(),
            catalogue_digest: String::new(),
            options: vec![],
            excluded: vec![],
            evidence_refs: vec![],
            digest: String::new(),
        };
        assert_eq!(
            sealed_advice(Uuid::new_v4(), bytes, "wrong", &manifest),
            Err(Error::InputConflict)
        );
    }
}
