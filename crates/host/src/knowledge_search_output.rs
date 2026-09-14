use crate::responses;
use serde_json::Value;
use tect_application::KnowledgeSearchOutputGuard;
use tect_domain::{Error, KnowledgeSearchResponse, Result};

pub(crate) struct KnowledgeSearchEncoding {
    capacity: usize,
}

impl KnowledgeSearchEncoding {
    pub(crate) const fn new(capacity: usize) -> Self {
        Self { capacity }
    }
}

impl KnowledgeSearchOutputGuard for KnowledgeSearchEncoding {
    fn check(&self, response: &KnowledgeSearchResponse) -> Result<()> {
        encode(response.clone(), self.capacity).map(drop)
    }
}

pub(crate) fn encode(response: KnowledgeSearchResponse, capacity: usize) -> Result<Value> {
    let mut response = response;
    loop {
        response.bounds.results_returned = response
            .results
            .len()
            .try_into()
            .map_err(|_| Error::InternalInvariant)?;
        let data = serde_json::to_value(&response).map_err(|_| Error::TransportUnavailable)?;
        let value = responses::with_actions(data, Vec::new(), None);
        if responses::encoded_len(&value)? <= capacity {
            return Ok(value);
        }
        if response.results.pop().is_none() {
            return Err(Error::RequestTooLarge);
        }
        response.bounds.results_truncated = true;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use tect_domain::{
        KnowledgeBindingPurpose, KnowledgeBindingVersion, KnowledgeGraphBindingQualifier,
        KnowledgeGraphHop, KnowledgeGraphPath, KnowledgeKind, KnowledgeLifecycleState,
        KnowledgeSearchBounds, KnowledgeSearchEntityField, KnowledgeSearchReason,
        KnowledgeSearchRelation, KnowledgeSearchResult, KnowledgeVectorStatus,
    };
    use uuid::Uuid;

    fn response() -> KnowledgeSearchResponse {
        KnowledgeSearchResponse {
            results: (0..3)
                .map(|index| KnowledgeSearchResult {
                    unit_id: Uuid::new_v4(),
                    resource_iri: format!("urn:unit:{index}"),
                    revision: 1,
                    revision_iri: format!("urn:unit:{index}:revision:1"),
                    title: "x".repeat(512),
                    kind: KnowledgeKind::Constraint,
                    lifecycle: KnowledgeLifecycleState::Active,
                    source_digests: Vec::new(),
                    freshness_warnings: Vec::new(),
                    reasons: vec![
                        KnowledgeSearchReason::EntityMatch {
                            field: KnowledgeSearchEntityField::Title,
                        },
                        KnowledgeSearchReason::GraphPath {
                            path: KnowledgeGraphPath {
                                seed_iri: "urn:seed".into(),
                                hops: vec![KnowledgeGraphHop {
                                    from_iri: "urn:to".into(),
                                    to_iri: "urn:from".into(),
                                    traversed_in_reverse: true,
                                    relation: KnowledgeSearchRelation::DependsOn,
                                    supporting_unit_id: Uuid::new_v4(),
                                    supporting_revision: 1,
                                    supporting_revision_iri: "urn:support:revision:1".into(),
                                    predicate_path: vec!["urn:predicate".into()],
                                    binding: Some(KnowledgeGraphBindingQualifier {
                                        purpose: KnowledgeBindingPurpose::Required,
                                        version_resolution:
                                            KnowledgeBindingVersion::PinnedRevision { revision: 1 },
                                        phase_id: Some("execute".into()),
                                    }),
                                }],
                            },
                        },
                    ],
                })
                .collect(),
            vector_status: KnowledgeVectorStatus::VectorUnavailable,
            bounds: KnowledgeSearchBounds {
                corpus_limit: 256,
                visible_corpus_count: 3,
                corpus_truncated: false,
                corpus_byte_budget: 1_048_576,
                corpus_bytes_inspected: 1_024,
                corpus_byte_budget_exhausted: false,
                result_limit: 20,
                results_returned: 3,
                results_truncated: false,
                max_depth: 0,
                depth_reached: 0,
                graph_node_budget: 4_096,
                graph_nodes_visited: 0,
                graph_edge_budget: 32_768,
                graph_edges_visited: 0,
                graph_budget_exhausted: false,
                graph_depth_exhausted: false,
                vector_slots_exhausted: false,
            },
            metrics: None,
        }
    }

    #[test]
    fn trailing_results_are_trimmed_with_explicit_bounds() {
        let full = encode(response(), usize::MAX).unwrap();
        let full_size = responses::encoded_len(&full).unwrap();
        let trimmed = encode(response(), full_size - 600).unwrap();
        assert!(trimmed["bounds"]["results_truncated"].as_bool().unwrap());
        assert_eq!(
            trimmed["bounds"]["results_returned"].as_u64().unwrap(),
            trimmed["results"].as_array().unwrap().len() as u64
        );
        assert!(trimmed["results"].as_array().unwrap().len() < 3);
        assert_eq!(trimmed["results"][0]["reasons"][0]["kind"], "entity_match");
        assert_eq!(
            trimmed["results"][0]["reasons"][1]["path"]["hops"][0]["traversed_in_reverse"],
            true
        );
        assert_eq!(
            trimmed["results"][0]["reasons"][1]["path"]["hops"][0]["binding"]["phase_id"],
            "execute"
        );
    }

    #[test]
    fn capacity_below_minimal_response_fails() {
        assert_eq!(encode(response(), 8), Err(Error::RequestTooLarge));
    }
}
