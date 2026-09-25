use super::*;

pub(super) async fn begin_send(
    uow: &mut PgUnitOfWork,
    saved: &StoredAntiBloatReview,
    prepared: &AntiBloatPreparedRequest,
) -> Result<Option<AntiBloatSendPermit>> {
    if !uow.is_read_write() || uow.principal_id()? != saved.actor_id {
        return Err(Error::Forbidden);
    }
    let tenant = uow.tenant_id()?;
    let locked_revision: Option<i64> = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 \
         AND workspace_id=$2 AND id=$3 FOR SHARE",
    )
    .bind(tenant)
    .bind(saved.workspace_id)
    .bind(saved.review.candidate_set_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if locked_revision != Some(saved.review.plan_revision) {
        return Err(Error::InputConflict);
    }
    if uow
        .authoritative_input(
            saved.workspace_id,
            saved.review.candidate_set_id,
            saved.review.plan_revision,
        )
        .await?
        .as_ref()
        != Some(&saved.input)
    {
        return Err(Error::InputConflict);
    }
    let eligible = saved
        .review
        .findings
        .iter()
        .filter(|item| item.rankable)
        .map(|item| item.id.clone())
        .collect::<Vec<_>>();
    if eligible.is_empty() || saved.state != AntiBloatAttemptState::Prepared {
        return Ok(None);
    }
    if digest(&prepared.bytes) != prepared.sha256 {
        return Err(Error::InputConflict);
    }
    let expected = serde_json::to_vec(&serde_json::json!({
        "review": &saved.review, "eligible_ids": &eligible
    }))
    .map_err(storage_error)?;
    if prepared.bytes != expected {
        return Err(Error::InputConflict);
    }
    let result = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state='sending',request_bytes=$5, \
         request_sha256=$6,send_started_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3 AND actor_id=$4 \
           AND state='prepared' AND input_payload=$7 AND review_payload=$8",
    )
    .bind(tenant)
    .bind(saved.workspace_id)
    .bind(saved.review_id)
    .bind(saved.actor_id)
    .bind(&prepared.bytes)
    .bind(&prepared.sha256)
    .bind(serde_json::to_value(&saved.input).map_err(storage_error)?)
    .bind(serde_json::to_value(&saved.review).map_err(storage_error)?)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    Ok((result.rows_affected() == 1).then(|| AntiBloatSendPermit {
        review_id: saved.review_id,
        request: prepared.clone(),
    }))
}

pub(super) async fn mark_send_unknown(uow: &mut PgUnitOfWork, review_id: Uuid) -> Result<()> {
    let tenant = uow.tenant_id()?;
    let actor = uow.principal_id()?;
    sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state='send_unknown' \
                 WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 AND state='sending'",
    )
    .bind(tenant)
    .bind(review_id)
    .bind(actor)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    Ok(())
}

pub(super) async fn seal_response(
    uow: &mut PgUnitOfWork,
    permit: &AntiBloatSendPermit,
    raw_response: &[u8],
    response_sha256: &str,
) -> Result<()> {
    if !uow.is_read_write() || digest(raw_response) != response_sha256 {
        return Err(Error::InputConflict);
    }
    let result = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET raw_response=$5,response_sha256=$6, \
         response_sealed_at=pg_catalog.clock_timestamp() \
         WHERE tenant_id=$1 AND review_id=$2 AND actor_id=$3 AND state='sending' \
           AND request_bytes=$4 AND request_sha256=$7 AND raw_response IS NULL",
    )
    .bind(uow.tenant_id()?)
    .bind(permit.review_id)
    .bind(uow.principal_id()?)
    .bind(&permit.request.bytes)
    .bind(raw_response)
    .bind(response_sha256)
    .bind(&permit.request.sha256)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}

pub(super) async fn seal_ranked(
    uow: &mut PgUnitOfWork,
    review_id: Uuid,
    ranked_ids: &[String],
) -> Result<()> {
    let tenant = uow.tenant_id()?;
    let actor = uow.principal_id()?;
    let value = serde_json::to_value(ranked_ids).map_err(storage_error)?;
    let result = sqlx::query(
        "UPDATE scope_anti_bloat_reviews SET state='ranked',ranked_ids=$4, \
         sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND review_id=$2 \
         AND actor_id=$3 AND state='sending' AND raw_response IS NOT NULL \
         AND eligible_ids @> $4::jsonb AND $4::jsonb @> eligible_ids",
    )
    .bind(tenant)
    .bind(review_id)
    .bind(actor)
    .bind(value)
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    if result.rows_affected() != 1 {
        return Err(Error::InputConflict);
    }
    Ok(())
}
