use super::*;
use tect_application::{PIPELINE_ADVICE_INTERPRETATION_VERSION, PipelineAdviceInterpretation};

pub(crate) async fn disposition_advice(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    opportunity: Uuid,
    dispatch: Uuid,
    digest: &str,
) -> Result<Option<tect_domain::PipelineDispositionAdvice>> {
    let Some(value) = get(uow, workspace, opportunity).await? else {
        return Ok(None);
    };
    if value.dispatch_id != dispatch || value.response_sha256 != digest {
        return Err(Error::InputConflict);
    }
    Ok(Some(match value.ranking {
        tect_domain::PipelineRecommendationRanking::Ranked { ranked_ids } => {
            tect_domain::PipelineDispositionAdvice::Ranked {
                dispatch_id: dispatch,
                ranked_ids,
            }
        }
        tect_domain::PipelineRecommendationRanking::Abstained => {
            tect_domain::PipelineDispositionAdvice::Abstained {
                dispatch_id: dispatch,
            }
        }
    }))
}

pub(crate) async fn get(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    opportunity: Uuid,
) -> Result<Option<PipelineAdviceInterpretation>> {
    let tenant = uow.tenant_id()?;
    let actor = uow.principal_id()?;
    let saved = uow
        .pipeline_recommendation_by_opportunity(workspace, opportunity)
        .await?
        .ok_or(Error::NotFound)?;
    if saved.opportunity.authorized_actor_id != actor {
        return Err(Error::Forbidden);
    }
    let row = sqlx::query("SELECT dispatch_id,manifest_digest,response_sha256,contract_version,ranking FROM pipeline_advice_interpretations WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3")
        .bind(tenant).bind(workspace).bind(opportunity).fetch_optional(&mut **uow.transaction()?).await.map_err(storage_error)?;
    let Some(row) = row else {
        return Ok(None);
    };
    let value = PipelineAdviceInterpretation {
        opportunity_id: opportunity,
        dispatch_id: row.try_get("dispatch_id").map_err(storage_error)?,
        manifest_digest: row.try_get("manifest_digest").map_err(storage_error)?,
        response_sha256: row.try_get("response_sha256").map_err(storage_error)?,
        contract_version: u32::try_from(
            row.try_get::<i32, _>("contract_version")
                .map_err(storage_error)?,
        )
        .map_err(|_| Error::InputConflict)?,
        ranking: serde_json::from_value(row.try_get("ranking").map_err(storage_error)?)
            .map_err(|_| Error::InputConflict)?,
    };
    validate_binding(uow, workspace, &saved, &value).await?;
    Ok(Some(value))
}

async fn validate_binding(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    saved: &PreparedPipelineRecommendation,
    value: &PipelineAdviceInterpretation,
) -> Result<()> {
    if value.contract_version != PIPELINE_ADVICE_INTERPRETATION_VERSION
        || value.opportunity_id != saved.opportunity.id
        || value.manifest_digest != saved.manifest.digest
    {
        return Err(Error::InputConflict);
    }
    value
        .ranking
        .validate(&saved.manifest)
        .map_err(|_| Error::InputConflict)?;
    let tenant = uow.tenant_id()?;
    let bound: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM advisory_dispatch d JOIN advisory_provider_observations r ON (r.tenant_id,r.workspace_id,r.opportunity_id,r.dispatch_id)=(d.tenant_id,d.workspace_id,d.opportunity_id,d.id) JOIN advisory_budget_consumptions c ON (c.tenant_id,c.workspace_id,c.dispatch_id)=(d.tenant_id,d.workspace_id,d.id) WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.opportunity_id=$3 AND d.id=$4 AND d.state='sealed' AND d.send_certainty='sent' AND d.outcome='provider_response' AND d.material_digest=$5 AND d.pipeline_response_sha256=$6 AND encode(sha256(d.response_payload),'hex')=$6 AND r.response_sha256=$6 AND r.response_complete AND r.response_payload=d.response_payload AND NOT c.unknown_usage AND NOT c.exhausted_after_response)")
        .bind(tenant).bind(workspace).bind(value.opportunity_id).bind(value.dispatch_id).bind(&value.manifest_digest).bind(&value.response_sha256).fetch_one(&mut **uow.transaction()?).await.map_err(storage_error)?;
    if !bound {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn insert(
    uow: &mut PgUnitOfWork,
    workspace: Uuid,
    value: &PipelineAdviceInterpretation,
) -> Result<PipelineAdviceInterpretation> {
    let saved = uow
        .pipeline_recommendation_by_opportunity(workspace, value.opportunity_id)
        .await?
        .ok_or(Error::NotFound)?;
    if saved.opportunity.authorized_actor_id != uow.principal_id()? {
        return Err(Error::Forbidden);
    }
    validate_binding(uow, workspace, &saved, value).await?;
    if !uow
        .pipeline_recommendation_is_current(workspace, &saved)
        .await?
    {
        return Err(Error::StaleContext);
    }
    let tenant = uow.tenant_id()?;
    let ranking = serde_json::to_value(&value.ranking).map_err(storage_error)?;
    sqlx::query("INSERT INTO pipeline_advice_interpretations (tenant_id,workspace_id,opportunity_id,dispatch_id,manifest_digest,response_sha256,contract_version,ranking) VALUES ($1,$2,$3,$4,$5,$6,$7,$8) ON CONFLICT (tenant_id,workspace_id,opportunity_id) DO NOTHING")
        .bind(tenant).bind(workspace).bind(value.opportunity_id).bind(value.dispatch_id).bind(&value.manifest_digest).bind(&value.response_sha256).bind(i32::try_from(value.contract_version).map_err(|_|Error::InputConflict)?).bind(ranking).execute(&mut **uow.transaction()?).await.map_err(write_error)?;
    let stored = get(uow, workspace, value.opportunity_id)
        .await?
        .ok_or(Error::InputConflict)?;
    if stored != *value {
        return Err(Error::InputConflict);
    }
    Ok(stored)
}
