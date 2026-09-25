use super::*;

pub(super) fn stale_reasons(
    context: &CandidateContext,
    material: &tect_domain::CandidateSnapshotMaterial,
) -> Vec<String> {
    let snapshot = &context.snapshot;
    let mut reasons = Vec::new();
    if snapshot.program_revision != material.program.revision
        || snapshot.program_latest_input != material.program.latest_input
    {
        reasons.push("program".into());
    }
    if snapshot.planning_latest_input != context.candidate_set.latest_input {
        reasons.push("planning_inputs".into());
    }
    if snapshot.selected_sources_digest != material.selected_sources_digest {
        reasons.push("selected_sources".into());
    }
    if snapshot.method.revision != material.method.revision
        || snapshot.method.digest != material.method.digest
    {
        reasons.push("method".into());
    }
    if snapshot.registry_revision != material.registry_revision
        || snapshot.registry_digest != material.registry_digest
    {
        reasons.push("rules".into());
    }
    reasons
}

pub(super) fn validate_write(
    id: uuid::Uuid,
    snapshot: uuid::Uuid,
    revision: i64,
    cursor: i64,
    request: uuid::Uuid,
) -> Result<()> {
    if id.is_nil() || snapshot.is_nil() || request.is_nil() || revision < 1 || cursor < 0 {
        Err(Error::InvalidArguments)
    } else {
        Ok(())
    }
}
