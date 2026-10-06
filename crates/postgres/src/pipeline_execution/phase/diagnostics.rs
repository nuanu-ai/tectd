//! Timing wrappers preserve each original adapter call and transaction fence.
use super::*;
use tect_application::request_diagnostics::measure;

#[allow(clippy::too_many_arguments)]
pub(super) async fn selected_manifest_validation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    manifest: Option<&PipelineKnowledgeManifest>,
    consumed: Option<&ConsumedKnowledgeManifestRef>,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<()> {
    measure(
        "pg.selected_manifest_validation",
        crate::durable_knowledge::manifest::validate_completion_with_proofs(
            tx, tenant, workspace, run, scope, slice, phase, session, manifest, consumed, proofs,
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn current_backend_knowledge_validation(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    manifest: Option<&PipelineKnowledgeManifest>,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Option<ConsumedKnowledgeManifestRef>> {
    measure(
        "pg.selected_manifest_validation",
        crate::durable_knowledge::manifest::validate_current_backend_knowledge(
            tx, tenant, workspace, run, scope, slice, phase, session, manifest, proofs,
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn next_input_capture(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    run: Uuid,
    revision: i64,
    scope: Uuid,
    slice: Uuid,
    phase: &str,
    session: Uuid,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Option<PipelineKnowledgeManifest>> {
    measure(
        "pg.next_input_capture",
        crate::durable_knowledge::manifest::capture_with_proofs(
            tx, tenant, workspace, run, revision, scope, slice, phase, session, proofs,
        ),
    )
    .await
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn returned_context(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    run: Uuid,
    session: Uuid,
    proofs: &mut crate::knowledge_lifecycle::PublicationProofScope,
) -> Result<Option<PipelineRunContext>> {
    measure(
        "pg.returned_context",
        load_context_with_proofs(tx, tenant, workspace, principal, run, session, proofs),
    )
    .await
}
