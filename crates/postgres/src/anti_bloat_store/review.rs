use super::*;

pub(super) async fn save_review(
    uow: &mut PgUnitOfWork,
    record: StoredAntiBloatReview,
) -> Result<StoredAntiBloatReview> {
    if !uow.is_read_write() || uow.principal_id()? != record.actor_id {
        return Err(Error::Forbidden);
    }
    let tenant = uow.tenant_id()?;
    let eligible = record
        .review
        .findings
        .iter()
        .filter(|finding| finding.rankable)
        .map(|finding| finding.id.clone())
        .collect::<Vec<_>>();
    let current = uow
        .authoritative_input(
            record.workspace_id,
            record.review.candidate_set_id,
            record.review.plan_revision,
        )
        .await?;
    if current.as_ref() != Some(&record.input)
        || review_anti_bloat(&Sha256ScopeDigest, &record.input)? != record.review
    {
        return Err(Error::InputConflict);
    }
    let snapshot = record.invocation.as_ref();
    if snapshot.is_some_and(|value| value.session_id.is_nil()) {
        return Err(Error::InvalidArguments);
    }
    sqlx::query(
        "INSERT INTO scope_anti_bloat_reviews \
         (tenant_id,workspace_id,review_id,candidate_set_id,candidate_set_revision,actor_id, \
          input_payload,review_payload,state,eligible_ids,origin_session_id,session_preference,request_preference) \
         VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,$13)",
    )
    .bind(tenant)
    .bind(record.workspace_id)
    .bind(record.review_id)
    .bind(record.review.candidate_set_id)
    .bind(record.review.plan_revision)
    .bind(record.actor_id)
    .bind(serde_json::to_value(&record.input).map_err(storage_error)?)
    .bind(serde_json::to_value(&record.review).map_err(storage_error)?)
    .bind(state_name(&record.state))
    .bind(serde_json::to_value(eligible).map_err(storage_error)?)
    .bind(snapshot.map(|value| value.session_id))
    .bind(snapshot.map_or("use_workspace", |value| value.session_preference.as_str()))
    .bind(snapshot.map_or("use_workspace", |value| value.request_preference.as_str()))
    .execute(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    Ok(record)
}

pub(super) async fn review(
    uow: &mut PgUnitOfWork,
    review_id: Uuid,
) -> Result<Option<StoredAntiBloatReview>> {
    let tenant = uow.tenant_id()?;
    type Row = (
        Uuid,
        Uuid,
        serde_json::Value,
        serde_json::Value,
        String,
        Option<serde_json::Value>,
        Option<Uuid>,
        String,
        String,
    );
    let row: Option<Row> = sqlx::query_as(
        "SELECT workspace_id,actor_id,input_payload,review_payload,state,ranked_ids, \
         origin_session_id,session_preference,request_preference \
         FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND review_id=$2",
    )
    .bind(tenant)
    .bind(review_id)
    .fetch_optional(&mut **uow.transaction()?)
    .await
    .map_err(storage_error)?;
    row.map(
        |(
            workspace_id,
            actor_id,
            input,
            review,
            state,
            ranked,
            session,
            session_preference,
            request_preference,
        )| {
            if uow.principal_id()? != actor_id {
                return Err(Error::Forbidden);
            }
            Ok(StoredAntiBloatReview {
                review_id,
                workspace_id,
                actor_id,
                invocation: session
                    .map(|session_id| {
                        Ok(tect_application::AntiBloatInvocationSnapshot {
                            session_id,
                            session_preference: preference(&session_preference)?,
                            request_preference: preference(&request_preference)?,
                        })
                    })
                    .transpose()?,
                input: serde_json::from_value(input).map_err(storage_error)?,
                review: serde_json::from_value(review).map_err(storage_error)?,
                state: parse_state(&state, ranked)?,
            })
        },
    )
    .transpose()
}

fn preference(value: &str) -> Result<AdvisoryRequestPreference> {
    match value {
        "use_workspace" => Ok(AdvisoryRequestPreference::UseWorkspace),
        "skip" => Ok(AdvisoryRequestPreference::Skip),
        _ => Err(Error::InternalInvariant),
    }
}
