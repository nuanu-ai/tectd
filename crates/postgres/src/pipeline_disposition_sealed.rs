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
    // Legacy typed wire only; a present interpretation never falls back here.
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
