use crate::{
    Error, PipelineKind, Result, SliceCandidateDraft, SliceCandidateDraftNode, SliceCandidateNode,
    SliceCandidateRef,
};
use std::collections::{BTreeMap, BTreeSet};
use uuid::Uuid;

impl SliceCandidateDraft {
    pub fn validate(&self) -> Result<()> {
        if self.coverage_summary.trim().is_empty() || self.nodes.is_empty() {
            return Err(Error::InvalidArguments);
        }
        let mut locals = BTreeSet::new();
        for node in &self.nodes {
            let (identity, deps, sources) = match node {
                SliceCandidateDraftNode::Work {
                    identity,
                    title,
                    outcome,
                    proof,
                    pipeline,
                    pipeline_reason,
                    why_lightweight_insufficient,
                    why_further_vertical_split_not_viable,
                    dependencies,
                    source_result_ids,
                    ..
                } => {
                    if title.trim().is_empty()
                        || outcome.trim().is_empty()
                        || pipeline_reason.trim().is_empty()
                        || proof.is_empty()
                        || proof.iter().any(|v| v.trim().is_empty())
                    {
                        return Err(Error::InvalidArguments);
                    }
                    if *pipeline == PipelineKind::FullDesignToExecution
                        && (why_lightweight_insufficient
                            .as_deref()
                            .unwrap_or("")
                            .trim()
                            .is_empty()
                            || why_further_vertical_split_not_viable
                                .as_deref()
                                .unwrap_or("")
                                .trim()
                                .is_empty())
                    {
                        return Err(Error::InvalidArguments);
                    }
                    (identity, dependencies, source_result_ids)
                }
                SliceCandidateDraftNode::Decision {
                    identity,
                    title,
                    question,
                    resolution_criteria,
                    dependencies,
                    source_result_ids,
                    ..
                } => {
                    if title.trim().is_empty()
                        || question.trim().is_empty()
                        || resolution_criteria.is_empty()
                        || resolution_criteria.iter().any(|v| v.trim().is_empty())
                    {
                        return Err(Error::InvalidArguments);
                    }
                    (identity, dependencies, source_result_ids)
                }
            };
            match (&identity.local, identity.candidate_id, identity.revision) {
                (Some(local), None, None) if !local.trim().is_empty() => {
                    if !locals.insert(local.clone()) {
                        return Err(Error::InvalidArguments);
                    }
                }
                (None, Some(id), Some(rev)) if !id.is_nil() && rev > 0 => {}
                _ => return Err(Error::InvalidArguments),
            }
            if sources.iter().any(Uuid::is_nil)
                || deps
                    .iter()
                    .any(|d| matches!(d,SliceCandidateRef::Local{local} if local.trim().is_empty()))
            {
                return Err(Error::InvalidArguments);
            }
        }
        Ok(())
    }
}

pub fn validate_slice_graph(nodes: &[SliceCandidateNode]) -> Result<()> {
    let by_id = nodes
        .iter()
        .map(|n| (n.id(), n))
        .collect::<BTreeMap<_, _>>();
    if by_id.len() != nodes.len() || nodes.iter().any(|n| n.id().is_nil() || n.revision() < 1) {
        return Err(Error::InvalidArguments);
    }
    fn visit(
        id: Uuid,
        by: &BTreeMap<Uuid, &SliceCandidateNode>,
        visiting: &mut BTreeSet<Uuid>,
        visited: &mut BTreeSet<Uuid>,
    ) -> Result<()> {
        if visited.contains(&id) {
            return Ok(());
        }
        if !visiting.insert(id) {
            return Err(Error::InvalidArguments);
        }
        let node = by.get(&id).ok_or(Error::InvalidArguments)?;
        for dep in node.dependencies() {
            if !by.contains_key(dep) {
                return Err(Error::InvalidArguments);
            }
            visit(*dep, by, visiting, visited)?
        }
        visiting.remove(&id);
        visited.insert(id);
        Ok(())
    }
    let mut visiting = BTreeSet::new();
    let mut visited = BTreeSet::new();
    for id in by_id.keys().copied() {
        visit(id, &by_id, &mut visiting, &mut visited)?
    }
    Ok(())
}
