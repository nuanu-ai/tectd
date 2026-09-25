use super::*;

pub(super) async fn build_history(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    set_id: Uuid,
    current: Option<&ResolvedSliceCandidateDraft>,
    slices: &[NativeSlice],
) -> Result<Vec<SliceCandidateHistoryEntry>> {
    let payloads:Vec<serde_json::Value>=sqlx::query_scalar("SELECT payload FROM slice_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 ORDER BY set_revision")
        .bind(tenant).bind(workspace).bind(set_id).fetch_all(&mut **tx).await.map_err(storage_error)?;
    let mut versions: BTreeMap<(Uuid, i64), String> = BTreeMap::new();
    let mut superseded: BTreeMap<Uuid, (String, Vec<Uuid>)> = BTreeMap::new();
    for payload in payloads {
        let d: ResolvedSliceCandidateDraft = decode(payload)?;
        for n in d.nodes {
            versions.insert((n.id(), n.revision()), n.title().into());
        }
        for s in d.supersessions {
            superseded.insert(s.candidate_id, (s.reason, s.replacement_candidate_ids));
        }
    }
    let current_ids = current
        .map(|d| {
            d.nodes
                .iter()
                .map(SliceCandidateNode::id)
                .collect::<BTreeSet<_>>()
        })
        .unwrap_or_default();
    let opened = slices
        .iter()
        .map(|s| s.candidate_id)
        .collect::<BTreeSet<_>>();
    Ok(versions
        .into_iter()
        .map(|((id, rev), title)| {
            let (status, reason, replacements) = if opened.contains(&id) {
                (SliceCandidateHistoryStatus::Opened, None, Vec::new())
            } else if let Some((reason, replacements)) = superseded.get(&id) {
                (
                    SliceCandidateHistoryStatus::Superseded,
                    Some(reason.clone()),
                    replacements.clone(),
                )
            } else if current_ids.contains(&id) {
                (SliceCandidateHistoryStatus::Active, None, Vec::new())
            } else {
                (SliceCandidateHistoryStatus::Prior, None, Vec::new())
            };
            SliceCandidateHistoryEntry {
                candidate_id: id,
                candidate_revision: rev,
                title,
                status,
                superseded_reason: reason,
                replacement_candidate_ids: replacements,
            }
        })
        .collect())
}
