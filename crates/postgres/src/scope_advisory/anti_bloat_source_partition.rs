/// Re-read the exact frozen source and its native citation kinds. The source
/// digest covers IDs and bodies; the immutable typed partition also prevents
/// a changed kind from silently converting required context into a goal.
pub(crate) async fn trusted_non_goal_source_obligation_ids(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    manifest: &ScopeConstructorManifest,
    boundary: tect_domain::CandidateBoundary,
) -> Result<Vec<String>> {
    let fragments = load_persisted_fragments(
        tx,
        tenant,
        workspace,
        manifest.source.candidate_set_id,
        manifest.source.snapshot_id,
    )
    .await?;
    let (inputs, obligations) =
        source_inputs_and_obligations(fragments.clone(), manifest.source.snapshot_id)?;
    if inputs != manifest.source.inputs || obligations != manifest.obligations {
        return Err(Error::InvalidSource);
    }
    let mut non_goal = Vec::new();
    for fragment in fragments {
        if fragment.body.trim().is_empty() {
            continue;
        }
        if !source_ref_can_anchor_goal(boundary, &fragment.kind)? {
            non_goal.push(fragment.id.to_string());
        }
    }
    non_goal.sort();
    Ok(non_goal)
}

fn source_ref_can_anchor_goal(
    boundary: tect_domain::CandidateBoundary,
    kind: &str,
) -> Result<bool> {
    if !matches!(kind, "planning_input" | "program_success" | "program_field") {
        return Err(Error::InvalidSource);
    }
    Ok(kind
        == match boundary {
            tect_domain::CandidateBoundary::Ongoing => "planning_input",
            tect_domain::CandidateBoundary::Finite => "program_success",
        })
}

