use crate::{Error, KnowledgeBindingTarget, KnowledgeKind, KnowledgeLifecycleState, Result};
use serde::{Deserialize, Deserializer, Serialize, de::DeserializeOwned};
use std::collections::BTreeSet;
use uuid::Uuid;

pub const KNOWLEDGE_SEARCH_DEFAULT_LIMIT: u32 = 20;
pub const KNOWLEDGE_SEARCH_MAX_LIMIT: u32 = 50;
pub const KNOWLEDGE_SEARCH_DEFAULT_CORPUS_LIMIT: u32 = 256;
pub const KNOWLEDGE_SEARCH_MAX_CORPUS_LIMIT: u32 = 512;
pub const KNOWLEDGE_SEARCH_DEFAULT_DEPTH: u32 = 2;
pub const KNOWLEDGE_SEARCH_MAX_DEPTH: u32 = 4;
pub const KNOWLEDGE_SEARCH_MAX_QUERY_BYTES: usize = 2 * 1024;
pub const KNOWLEDGE_SEARCH_MAX_PURPOSE_BYTES: usize = 1024;
pub const KNOWLEDGE_SEARCH_MAX_SEEDS: usize = 16;
pub const KNOWLEDGE_SEARCH_CORPUS_BYTE_BUDGET: u64 = 8 * 1024 * 1024;
pub const KNOWLEDGE_SEARCH_GRAPH_NODE_BUDGET: u32 = 4_096;
pub const KNOWLEDGE_SEARCH_GRAPH_EDGE_BUDGET: u32 = 32_768;
pub const KNOWLEDGE_EMBEDDING_MODEL: &str = "intfloat/multilingual-e5-small";
pub const KNOWLEDGE_EMBEDDING_REVISION: &str = "614241f622f53c4eeff9890bdc4f31cfecc418b3";
pub const KNOWLEDGE_EMBEDDING_RECIPE: &str = "title_v1";
pub const KNOWLEDGE_EMBEDDING_DIMENSIONS: u32 = 384;

fn default_limit() -> u32 {
    KNOWLEDGE_SEARCH_DEFAULT_LIMIT
}

fn default_corpus_limit() -> u32 {
    KNOWLEDGE_SEARCH_DEFAULT_CORPUS_LIMIT
}

fn reject_null_option<'de, D, T>(deserializer: D) -> std::result::Result<Option<T>, D::Error>
where
    D: Deserializer<'de>,
    T: DeserializeOwned,
{
    let value = serde_json::Value::deserialize(deserializer)?;
    if value.is_null() {
        return Err(serde::de::Error::custom("explicit null is not allowed"));
    }
    T::deserialize(value)
        .map(Some)
        .map_err(serde::de::Error::custom)
}

fn text(value: &str, max: usize) -> bool {
    !value.trim().is_empty() && value.len() <= max && !value.as_bytes().contains(&0)
}

fn iri(value: &str) -> bool {
    text(value, 4096) && oxrdf::NamedNode::new(value).is_ok()
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSearchMode {
    Lexical,
    GraphSearch,
    SuperWide,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, PartialOrd, Ord, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSearchRelation {
    Targets,
    DependsOn,
    UsesAsset,
    InEnvironment,
    DerivedFrom,
    BoundTo,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSearchDirection {
    Outgoing,
    Incoming,
    Both,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchQuery {
    pub mode: KnowledgeSearchMode,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "reject_null_option"
    )]
    pub query: Option<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub seeds: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub relations: Vec<KnowledgeSearchRelation>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "reject_null_option"
    )]
    pub direction: Option<KnowledgeSearchDirection>,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "reject_null_option"
    )]
    pub max_depth: Option<u32>,
    #[serde(default)]
    pub include_graph: bool,
    #[serde(default = "default_limit")]
    pub limit: u32,
    #[serde(default = "default_corpus_limit")]
    pub corpus_limit: u32,
    #[serde(
        default,
        skip_serializing_if = "Option::is_none",
        deserialize_with = "reject_null_option"
    )]
    pub binding: Option<KnowledgeBindingTarget>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub kinds: Vec<KnowledgeKind>,
    pub purpose: String,
}

impl KnowledgeSearchQuery {
    pub fn validate(&self) -> Result<()> {
        if !(1..=KNOWLEDGE_SEARCH_MAX_LIMIT).contains(&self.limit)
            || !(1..=KNOWLEDGE_SEARCH_MAX_CORPUS_LIMIT).contains(&self.corpus_limit)
            || !text(&self.purpose, KNOWLEDGE_SEARCH_MAX_PURPOSE_BYTES)
            || self.seeds.len() > KNOWLEDGE_SEARCH_MAX_SEEDS
            || self.seeds.iter().any(|value| !iri(value))
            || self.seeds.iter().collect::<BTreeSet<_>>().len() != self.seeds.len()
            || self.relations.iter().collect::<BTreeSet<_>>().len() != self.relations.len()
            || self.kinds.iter().collect::<BTreeSet<_>>().len() != self.kinds.len()
            || self
                .binding
                .as_ref()
                .is_some_and(|binding| !binding.valid())
        {
            return Err(Error::InvalidArguments);
        }
        let query_valid = self
            .query
            .as_deref()
            .is_some_and(|value| text(value, KNOWLEDGE_SEARCH_MAX_QUERY_BYTES));
        let graph_fields_absent = self.seeds.is_empty()
            && self.relations.is_empty()
            && self.direction.is_none()
            && self.max_depth.is_none()
            && !self.include_graph;
        let graph_fields_valid = !self.relations.is_empty()
            && self.direction.is_some()
            && self.max_depth.unwrap_or(KNOWLEDGE_SEARCH_DEFAULT_DEPTH) > 0
            && self.max_depth.unwrap_or(KNOWLEDGE_SEARCH_DEFAULT_DEPTH)
                <= KNOWLEDGE_SEARCH_MAX_DEPTH;
        let valid = match self.mode {
            KnowledgeSearchMode::Lexical => query_valid && graph_fields_absent,
            KnowledgeSearchMode::GraphSearch => {
                self.query.is_none()
                    && !self.seeds.is_empty()
                    && graph_fields_valid
                    && !self.include_graph
            }
            KnowledgeSearchMode::SuperWide => {
                query_valid
                    && ((!self.include_graph && graph_fields_absent)
                        || (self.include_graph && graph_fields_valid))
            }
        };
        valid.then_some(()).ok_or(Error::InvalidArguments)
    }

    pub fn effective_depth(&self) -> u32 {
        self.max_depth.unwrap_or(KNOWLEDGE_SEARCH_DEFAULT_DEPTH)
    }
}

trait ValidBinding {
    fn valid(&self) -> bool;
}

impl ValidBinding for KnowledgeBindingTarget {
    fn valid(&self) -> bool {
        match self {
            Self::Workspace => true,
            Self::Program { program_id } => !program_id.is_nil(),
            Self::Scope { scope_id } => !scope_id.is_nil(),
            Self::Slice { scope_id, slice_id } => !scope_id.is_nil() && !slice_id.is_nil(),
            Self::SlicePhase {
                scope_id,
                slice_id,
                phase_id,
            } => !scope_id.is_nil() && !slice_id.is_nil() && text(phase_id, 256),
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeSearchEntityField {
    ResourceIri,
    Title,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeLexicalConfiguration {
    English,
    Russian,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGraphHop {
    pub from_iri: String,
    pub to_iri: String,
    pub traversed_in_reverse: bool,
    pub relation: KnowledgeSearchRelation,
    pub supporting_unit_id: Uuid,
    pub supporting_revision: i64,
    pub supporting_revision_iri: String,
    pub predicate_path: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub binding: Option<KnowledgeGraphBindingQualifier>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGraphBindingQualifier {
    pub purpose: crate::KnowledgeBindingPurpose,
    pub version_resolution: crate::KnowledgeBindingVersion,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub phase_id: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeGraphPath {
    pub seed_iri: String,
    pub hops: Vec<KnowledgeGraphHop>,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind", rename_all = "snake_case", deny_unknown_fields)]
pub enum KnowledgeSearchReason {
    EntityMatch {
        field: KnowledgeSearchEntityField,
    },
    LexicalMatch {
        configuration: KnowledgeLexicalConfiguration,
    },
    VectorSimilarity {
        cosine_distance: f32,
    },
    GraphPath {
        path: KnowledgeGraphPath,
    },
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchResult {
    pub unit_id: Uuid,
    pub resource_iri: String,
    pub revision: i64,
    pub revision_iri: String,
    pub title: String,
    pub kind: KnowledgeKind,
    pub lifecycle: KnowledgeLifecycleState,
    pub source_digests: Vec<String>,
    pub freshness_warnings: Vec<String>,
    pub reasons: Vec<KnowledgeSearchReason>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeVectorStatus {
    NotRequested,
    VectorUnavailable,
    Ready,
    Partial,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchBounds {
    pub corpus_limit: u32,
    pub visible_corpus_count: u32,
    pub corpus_truncated: bool,
    pub corpus_byte_budget: u64,
    pub corpus_bytes_inspected: u64,
    pub corpus_byte_budget_exhausted: bool,
    pub result_limit: u32,
    pub results_returned: u32,
    pub results_truncated: bool,
    pub max_depth: u32,
    pub depth_reached: u32,
    pub graph_node_budget: u32,
    pub graph_nodes_visited: u32,
    pub graph_edge_budget: u32,
    pub graph_edges_visited: u32,
    pub graph_budget_exhausted: bool,
    pub graph_depth_exhausted: bool,
    pub vector_slots_exhausted: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchMetrics {
    pub embedding_cache_hit: bool,
    pub embedding_latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchResponse {
    pub results: Vec<KnowledgeSearchResult>,
    pub vector_status: KnowledgeVectorStatus,
    pub bounds: KnowledgeSearchBounds,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub metrics: Option<KnowledgeSearchMetrics>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEmbeddingModelIdentity {
    pub name: String,
    pub revision: String,
    pub dimensions: u32,
    pub recipe: String,
}

impl KnowledgeEmbeddingModelIdentity {
    pub fn pinned() -> Self {
        Self {
            name: KNOWLEDGE_EMBEDDING_MODEL.into(),
            revision: KNOWLEDGE_EMBEDDING_REVISION.into(),
            dimensions: KNOWLEDGE_EMBEDDING_DIMENSIONS,
            recipe: KNOWLEDGE_EMBEDDING_RECIPE.into(),
        }
    }

    pub fn validate(&self) -> Result<()> {
        (*self == Self::pinned())
            .then_some(())
            .ok_or(Error::InvalidConfiguration)
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum KnowledgeEmbeddingPurpose {
    Query,
    PassageTitle,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeEmbeddingRequest {
    pub request_id: Uuid,
    pub purpose: KnowledgeEmbeddingPurpose,
    pub text: String,
    pub input_digest: String,
    pub model: KnowledgeEmbeddingModelIdentity,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeSearchPreflight {
    pub workspace_generation: i64,
    pub vector_capability_ready: bool,
    pub normalized_query: Option<String>,
    pub query_input_digest: Option<String>,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeQueryEmbedding {
    pub model: KnowledgeEmbeddingModelIdentity,
    pub input_digest: String,
    pub values: Vec<f32>,
    pub cache_hit: bool,
    pub latency_ms: u64,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub struct KnowledgeEmbeddingJobClaim {
    pub job_id: Uuid,
    pub lease_token: Uuid,
    pub workspace_id: Uuid,
    pub principal_id: Uuid,
    pub unit_id: Uuid,
    pub revision: i64,
    pub title: String,
    pub input_digest: String,
    pub model: KnowledgeEmbeddingModelIdentity,
}

#[derive(Debug, Clone, PartialEq)]
pub struct KnowledgeEmbeddingJobCompletion {
    pub job_id: Uuid,
    pub lease_token: Uuid,
    pub input_digest: String,
    pub model: KnowledgeEmbeddingModelIdentity,
    pub values: Vec<f32>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct KnowledgeSearchJobProcessOutcome {
    pub claimed: u32,
    pub published: u32,
    pub obsolete: u32,
    pub failed: u32,
    pub pending: u32,
}
