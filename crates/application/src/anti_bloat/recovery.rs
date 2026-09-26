use super::*;

pub(super) async fn load_saved_response(
    store: &mut dyn AntiBloatStore,
    provider: &dyn AntiBloatRankingProvider,
    review_id: Uuid,
) -> Result<Option<crate::AntiBloatSealedResponse>> {
    let review = store.review(review_id).await?.ok_or(Error::NotFound)?;
    if review.review_id != review_id || review.state != AntiBloatAttemptState::Sending {
        return Err(Error::InputConflict);
    }
    require_current(store, &review).await?;
    if let Some(profile) = provider.required_profile()
        && !store
            .provider_profile_matches(review.workspace_id, profile)
            .await?
    {
        return Err(Error::InputConflict);
    }
    if !provider.available() {
        return Err(Error::InvalidConfiguration);
    }
    let Some(saved) = store.saved_sealed_response(review_id).await? else {
        return Ok(None);
    };
    let eligible = review
        .review
        .findings
        .iter()
        .filter(|f| f.rankable)
        .map(|f| f.id.clone())
        .collect::<Vec<_>>();
    let bytes = provider.prepare(&crate::AntiBloatRankingMaterial {
        saved: &review,
        eligible_ids: &eligible,
    })?;
    if saved.permit.review_id != review_id
        || saved.permit.request.bytes != bytes
        || saved.permit.request.sha256 != format!("{:x}", Sha256::digest(&bytes))
        || saved.permit.request.adapter_identity != provider.adapter_identity()
        || saved.permit.request.material_sha256 != crate::anti_bloat_material_sha256(&review)?
    {
        return Err(Error::InputConflict);
    }
    Ok(Some(saved))
}
