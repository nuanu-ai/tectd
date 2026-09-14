use super::corpus::{SearchEdge, SearchResource};
use super::*;
use std::collections::{BTreeMap, BTreeSet, VecDeque};

pub(super) struct GraphTraversal {
    pub paths: Vec<(Uuid, KnowledgeGraphPath)>,
    pub depth_reached: u32,
    pub nodes_visited: u32,
    pub edges_visited: u32,
    pub budget_exhausted: bool,
    pub depth_exhausted: bool,
}

type Adjacent = BTreeMap<String, Vec<(String, KnowledgeGraphHop)>>;

pub(super) fn traverse(
    corpus: &[SearchResource],
    seeds: &[String],
    relations: &[KnowledgeSearchRelation],
    direction: KnowledgeSearchDirection,
    max_depth: u32,
) -> GraphTraversal {
    let visible = corpus
        .iter()
        .flat_map(|resource| {
            resource
                .visible_internal_endpoints
                .iter()
                .map(String::as_str)
        })
        .collect::<BTreeSet<_>>();
    let by_iri = corpus
        .iter()
        .map(|resource| (resource.resource_iri.as_str(), resource))
        .collect::<BTreeMap<_, _>>();
    let adjacent = adjacency(corpus, relations, direction, &visible);
    let mut ordered_seeds = seeds.to_vec();
    ordered_seeds.sort();
    ordered_seeds.dedup();
    let mut queue = VecDeque::new();
    let mut visited = BTreeSet::new();
    for seed in ordered_seeds {
        if visited.len() >= KNOWLEDGE_SEARCH_GRAPH_NODE_BUDGET as usize {
            break;
        }
        if visited.insert(seed.clone()) {
            queue.push_back((seed.clone(), seed, Vec::<KnowledgeGraphHop>::new()));
        }
    }
    let mut paths = BTreeMap::<Uuid, KnowledgeGraphPath>::new();
    let mut depth_reached = 0;
    let mut edges_visited = 0u32;
    let mut budget_exhausted = queue.len() < seeds.len();
    let mut depth_exhausted = false;
    while let Some((seed, node, path)) = queue.pop_front() {
        let depth = u32::try_from(path.len()).unwrap_or(u32::MAX);
        depth_reached = depth_reached.max(depth);
        if let Some(resource) = by_iri.get(node.as_str()) {
            paths
                .entry(resource.unit_id)
                .or_insert_with(|| KnowledgeGraphPath {
                    seed_iri: seed.clone(),
                    hops: path.clone(),
                });
        }
        let Some(next) = adjacent.get(&node) else {
            continue;
        };
        if depth >= max_depth {
            depth_exhausted |= next.iter().any(|(target, _)| !visited.contains(target));
            continue;
        }
        for (target, hop) in next {
            if edges_visited >= KNOWLEDGE_SEARCH_GRAPH_EDGE_BUDGET {
                budget_exhausted = true;
                break;
            }
            edges_visited += 1;
            if visited.contains(target) {
                continue;
            }
            if visited.len() >= KNOWLEDGE_SEARCH_GRAPH_NODE_BUDGET as usize {
                budget_exhausted = true;
                break;
            }
            visited.insert(target.clone());
            let mut target_path = path.clone();
            target_path.push(hop.clone());
            queue.push_back((seed.clone(), target.clone(), target_path));
        }
        if budget_exhausted {
            break;
        }
    }
    let resource_iri = corpus
        .iter()
        .map(|value| (value.unit_id, value.resource_iri.as_str()))
        .collect::<BTreeMap<_, _>>();
    let mut paths = paths.into_iter().collect::<Vec<_>>();
    paths.sort_by(|(left_id, left), (right_id, right)| {
        left.hops
            .len()
            .cmp(&right.hops.len())
            .then_with(|| resource_iri.get(left_id).cmp(&resource_iri.get(right_id)))
    });
    GraphTraversal {
        paths,
        depth_reached,
        nodes_visited: u32::try_from(visited.len()).unwrap_or(u32::MAX),
        edges_visited,
        budget_exhausted,
        depth_exhausted,
    }
}

fn adjacency(
    corpus: &[SearchResource],
    relations: &[KnowledgeSearchRelation],
    direction: KnowledgeSearchDirection,
    visible: &BTreeSet<&str>,
) -> Adjacent {
    let mut result = Adjacent::new();
    for resource in corpus {
        for edge in &resource.edges {
            if !relations.contains(&edge.relation)
                || hidden_knowledge_endpoint(&edge.from, visible)
                || hidden_knowledge_endpoint(&edge.to, visible)
            {
                continue;
            }
            if matches!(
                direction,
                KnowledgeSearchDirection::Outgoing | KnowledgeSearchDirection::Both
            ) {
                result
                    .entry(edge.from.clone())
                    .or_default()
                    .push((edge.to.clone(), hop(resource, edge, false)));
            }
            if matches!(
                direction,
                KnowledgeSearchDirection::Incoming | KnowledgeSearchDirection::Both
            ) {
                result
                    .entry(edge.to.clone())
                    .or_default()
                    .push((edge.from.clone(), hop(resource, edge, true)));
            }
        }
    }
    for values in result.values_mut() {
        values.sort_by(|left, right| {
            left.0
                .cmp(&right.0)
                .then_with(|| {
                    left.1
                        .supporting_revision_iri
                        .cmp(&right.1.supporting_revision_iri)
                })
                .then_with(|| left.1.predicate_path.cmp(&right.1.predicate_path))
        });
    }
    result
}

fn hidden_knowledge_endpoint(value: &str, visible: &BTreeSet<&str>) -> bool {
    value.starts_with("urn:tect:dk:unit:") && !visible.contains(value)
}

fn hop(resource: &SearchResource, edge: &SearchEdge, reverse: bool) -> KnowledgeGraphHop {
    KnowledgeGraphHop {
        from_iri: edge.from.clone(),
        to_iri: edge.to.clone(),
        traversed_in_reverse: reverse,
        relation: edge.relation,
        supporting_unit_id: resource.unit_id,
        supporting_revision: resource.revision,
        supporting_revision_iri: resource.revision_iri.clone(),
        predicate_path: edge.predicate_path.clone(),
        binding: edge.binding.clone(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn resource(unit: Uuid, iri: &str, edge: Option<SearchEdge>) -> SearchResource {
        SearchResource {
            unit_id: unit,
            resource_iri: iri.into(),
            revision: 1,
            revision_iri: format!("{iri}:revision:1"),
            title: iri.into(),
            canonical_text: iri.into(),
            kind: KnowledgeKind::Procedure,
            lifecycle: KnowledgeLifecycleState::Active,
            access_scope: KnowledgeAccessScope::WorkspaceMembers,
            contract_version: "dk-2".into(),
            verified_payload_bytes: 1,
            source_digests: vec!["a".repeat(64)],
            freshness_warnings: Vec::new(),
            edges: edge.into_iter().collect(),
            visible_internal_endpoints: vec![iri.into()],
        }
    }

    #[test]
    fn incoming_hop_preserves_asserted_direction() {
        let source = "urn:example:source";
        let target = "urn:example:target";
        let supporting = Uuid::new_v4();
        let target_unit = Uuid::new_v4();
        let edge = SearchEdge {
            from: source.into(),
            to: target.into(),
            relation: KnowledgeSearchRelation::Targets,
            predicate_path: vec!["urn:tect:dk:v2:targets".into()],
            binding: None,
        };
        let corpus = vec![
            resource(supporting, source, Some(edge)),
            resource(target_unit, target, None),
        ];
        let traversal = traverse(
            &corpus,
            &[target.into()],
            &[KnowledgeSearchRelation::Targets],
            KnowledgeSearchDirection::Incoming,
            1,
        );
        let path = traversal
            .paths
            .iter()
            .find(|(unit, _)| *unit == supporting)
            .map(|(_, path)| path)
            .expect("incoming resource");
        assert_eq!(path.hops.len(), 1);
        assert_eq!(path.hops[0].from_iri, source);
        assert_eq!(path.hops[0].to_iri, target);
        assert!(path.hops[0].traversed_in_reverse);
    }
}
