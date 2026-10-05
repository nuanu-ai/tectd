use super::*;

#[allow(clippy::too_many_arguments)]
pub(super) async fn verify_replay_current_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    session: Uuid,
    run: Uuid,
    outcome: PipelineMutationOutcome,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<PipelineMutationOutcome> {
    // Preserve the original application preflight behavior: full current
    // authorization and proof errors propagate, while status values alone do
    // not introduce a new replay policy. Never revalidate old consumed inputs.
    let _ = diagnostics::returned_context(tx, tenant, workspace, principal, run, session, proofs)
        .await?
        .ok_or(Error::NotFound)?;
    Ok(outcome)
}
