use super::corpus::SearchResource;
use super::*;
use std::collections::BTreeMap;

pub(crate) async fn search(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    principal: Uuid,
    request: &KnowledgeSearchQuery,
    preflight: &KnowledgeSearchPreflight,
    embedding: Option<&KnowledgeQueryEmbedding>,
) -> Result<KnowledgeSearchResponse> {
    crate::durable_knowledge::require_identity_ready(tx).await?;
    let generation: i64 = sqlx::query_scalar(
        "SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&mut **tx)
    .await
    .map_err(storage_error)?;
    if generation != preflight.workspace_generation {
        return Err(Error::ContextChanged);
    }
    let (corpus, corpus_truncated, corpus_bytes, byte_exhausted) =
        super::corpus::load(tx, tenant, workspace, principal, request).await?;
    let mut selected = Vec::new();
    let mut reasons = BTreeMap::<Uuid, Vec<KnowledgeSearchReason>>::new();
    if let Some(query) = request.query.as_deref().map(normalized) {
        for resource in &corpus {
            if resource.resource_iri == request.query.as_deref().unwrap_or_default() {
                add(
                    &mut selected,
                    &mut reasons,
                    resource.unit_id,
                    KnowledgeSearchReason::EntityMatch {
                        field: KnowledgeSearchEntityField::ResourceIri,
                    },
                );
            }
            if normalized(&resource.title) == query {
                add(
                    &mut selected,
                    &mut reasons,
                    resource.unit_id,
                    KnowledgeSearchReason::EntityMatch {
                        field: KnowledgeSearchEntityField::Title,
                    },
                );
            }
        }
        for (unit, english, russian) in lexical(tx, tenant, workspace, &corpus, &query).await? {
            if english {
                add(
                    &mut selected,
                    &mut reasons,
                    unit,
                    KnowledgeSearchReason::LexicalMatch {
                        configuration: KnowledgeLexicalConfiguration::English,
                    },
                );
            }
            if russian {
                add(
                    &mut selected,
                    &mut reasons,
                    unit,
                    KnowledgeSearchReason::LexicalMatch {
                        configuration: KnowledgeLexicalConfiguration::Russian,
                    },
                );
            }
        }
    }
    let mut vector_status = KnowledgeVectorStatus::NotRequested;
    let mut vector_units = Vec::new();
    if request.mode == KnowledgeSearchMode::SuperWide {
        let ready_now = search_vector_ready(tx).await?;
        if ready_now {
            if let Some(embedding) = embedding.filter(|value| {
                preflight.vector_capability_ready
                    && Some(value.input_digest.as_str()) == preflight.query_input_digest.as_deref()
            }) {
                let vectors = vector(tx, tenant, workspace, &corpus, embedding).await?;
                vector_status = if vectors.len() == corpus.len() {
                    KnowledgeVectorStatus::Ready
                } else {
                    KnowledgeVectorStatus::Partial
                };
                for (unit, distance) in vectors {
                    vector_units.push(unit);
                    add(
                        &mut selected,
                        &mut reasons,
                        unit,
                        KnowledgeSearchReason::VectorSimilarity {
                            cosine_distance: distance,
                        },
                    );
                }
            } else {
                vector_status = KnowledgeVectorStatus::VectorUnavailable;
            }
        } else {
            vector_status = KnowledgeVectorStatus::VectorUnavailable;
        }
    }
    let graph_active = request.mode == KnowledgeSearchMode::GraphSearch
        || request.mode == KnowledgeSearchMode::SuperWide && request.include_graph;
    let mut graph = None;
    if graph_active {
        let mut seeds = request.seeds.clone();
        if request.mode == KnowledgeSearchMode::SuperWide {
            for unit in selected.iter().take(request.limit as usize) {
                if let Some(value) = corpus.iter().find(|value| value.unit_id == *unit) {
                    seeds.push(value.resource_iri.clone());
                }
            }
            seeds.sort();
            seeds.dedup();
        }
        let traversal = super::graph::traverse(
            &corpus,
            &seeds,
            &request.relations,
            request.direction.ok_or(Error::InvalidArguments)?,
            request.effective_depth(),
        );
        for (unit, path) in &traversal.paths {
            add(
                &mut selected,
                &mut reasons,
                *unit,
                KnowledgeSearchReason::GraphPath { path: path.clone() },
            );
        }
        graph = Some(traversal);
    }
    let total = selected.len();
    let vector_slots_exhausted = selected
        .iter()
        .skip(request.limit as usize)
        .any(|unit| vector_units.contains(unit));
    selected.truncate(request.limit as usize);
    let results = selected
        .into_iter()
        .map(|unit| {
            let value = corpus
                .iter()
                .find(|value| value.unit_id == unit)
                .ok_or(Error::InternalInvariant)?;
            Ok(KnowledgeSearchResult {
                unit_id: value.unit_id,
                resource_iri: value.resource_iri.clone(),
                revision: value.revision,
                revision_iri: value.revision_iri.clone(),
                title: value.title.clone(),
                kind: value.kind,
                lifecycle: value.lifecycle,
                source_digests: value.source_digests.clone(),
                freshness_warnings: value.freshness_warnings.clone(),
                reasons: reasons.remove(&unit).ok_or(Error::InternalInvariant)?,
            })
        })
        .collect::<Result<Vec<_>>>()?;
    let metrics = embedding.map(|value| KnowledgeSearchMetrics {
        embedding_cache_hit: value.cache_hit,
        embedding_latency_ms: value.latency_ms,
    });
    let graph_budget_exhausted = graph.as_ref().is_some_and(|value| value.budget_exhausted);
    Ok(KnowledgeSearchResponse {
        bounds: KnowledgeSearchBounds {
            corpus_limit: request.corpus_limit,
            visible_corpus_count: corpus.len().try_into().unwrap_or(u32::MAX),
            corpus_truncated,
            corpus_byte_budget: KNOWLEDGE_SEARCH_CORPUS_BYTE_BUDGET,
            corpus_bytes_inspected: corpus_bytes,
            corpus_byte_budget_exhausted: byte_exhausted,
            result_limit: request.limit,
            results_returned: results.len().try_into().unwrap_or(u32::MAX),
            results_truncated: total > request.limit as usize || graph_budget_exhausted,
            max_depth: if graph_active {
                request.effective_depth()
            } else {
                0
            },
            depth_reached: graph.as_ref().map_or(0, |value| value.depth_reached),
            graph_node_budget: KNOWLEDGE_SEARCH_GRAPH_NODE_BUDGET,
            graph_nodes_visited: graph.as_ref().map_or(0, |value| value.nodes_visited),
            graph_edge_budget: KNOWLEDGE_SEARCH_GRAPH_EDGE_BUDGET,
            graph_edges_visited: graph.as_ref().map_or(0, |value| value.edges_visited),
            graph_budget_exhausted,
            graph_depth_exhausted: graph.as_ref().is_some_and(|value| value.depth_exhausted),
            vector_slots_exhausted,
        },
        results,
        vector_status,
        metrics,
    })
}

fn add(
    selected: &mut Vec<Uuid>,
    reasons: &mut BTreeMap<Uuid, Vec<KnowledgeSearchReason>>,
    unit: Uuid,
    reason: KnowledgeSearchReason,
) {
    if !selected.contains(&unit) {
        selected.push(unit);
    }
    reasons.entry(unit).or_default().push(reason);
}

async fn lexical(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    corpus: &[SearchResource],
    query: &str,
) -> Result<Vec<(Uuid, bool, bool)>> {
    let ids = corpus.iter().map(|value| value.unit_id).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let rows: Vec<(Uuid, bool, bool, f32, String)> = sqlx::query_as(
        "SELECT unit_id,english_document @@ pg_catalog.plainto_tsquery('pg_catalog.english'::pg_catalog.regconfig,$4),russian_document @@ pg_catalog.plainto_tsquery('pg_catalog.russian'::pg_catalog.regconfig,$4),GREATEST(pg_catalog.ts_rank_cd(english_document,pg_catalog.plainto_tsquery('pg_catalog.english'::pg_catalog.regconfig,$4)),pg_catalog.ts_rank_cd(russian_document,pg_catalog.plainto_tsquery('pg_catalog.russian'::pg_catalog.regconfig,$4))),resource_iri FROM knowledge_search_resources WHERE tenant_id=$1 AND workspace_id=$2 AND unit_id=ANY($3) AND (english_document @@ pg_catalog.plainto_tsquery('pg_catalog.english'::pg_catalog.regconfig,$4) OR russian_document @@ pg_catalog.plainto_tsquery('pg_catalog.russian'::pg_catalog.regconfig,$4)) ORDER BY 4 DESC,5",
    ).bind(tenant).bind(workspace).bind(&ids).bind(query).fetch_all(&mut **tx).await.map_err(storage_error)?;
    Ok(rows.into_iter().map(|row| (row.0, row.1, row.2)).collect())
}

async fn vector(
    tx: &mut Transaction<'_, Postgres>,
    tenant: Uuid,
    workspace: Uuid,
    corpus: &[SearchResource],
    embedding: &KnowledgeQueryEmbedding,
) -> Result<Vec<(Uuid, f32)>> {
    let ids = corpus.iter().map(|value| value.unit_id).collect::<Vec<_>>();
    if ids.is_empty() {
        return Ok(Vec::new());
    }
    let literal = format!(
        "[{}]",
        embedding
            .values
            .iter()
            .map(f32::to_string)
            .collect::<Vec<_>>()
            .join(",")
    );
    let rows: Vec<(Uuid, f64)> = sqlx::query_as(
        "SELECT v.unit_id,(v.embedding <=> $4::vector)::double precision AS distance FROM knowledge_search_vectors v JOIN knowledge_search_resources r ON r.tenant_id=v.tenant_id AND r.workspace_id=v.workspace_id AND r.unit_id=v.unit_id JOIN knowledge_unit_heads h ON h.tenant_id=r.tenant_id AND h.workspace_id=r.workspace_id AND h.unit_id=r.unit_id WHERE v.tenant_id=$1 AND v.workspace_id=$2 AND v.unit_id=ANY($3) AND v.model_name=$5 AND v.model_revision=$6 AND v.dimensions=$7 AND v.recipe=$8 AND v.revision=r.revision AND v.access_scope=r.access_scope AND v.input_digest=r.embedding_input_digest AND h.accepted_revision=r.revision AND h.lifecycle='active' AND h.active AND NOT h.payload_erased AND NOT EXISTS(SELECT 1 FROM knowledge_suppression_ledger l WHERE l.tenant_id=h.tenant_id AND l.workspace_id=h.workspace_id AND l.unit_id=h.unit_id) ORDER BY distance,r.resource_iri",
    ).bind(tenant).bind(workspace).bind(&ids).bind(literal)
        .bind(&embedding.model.name).bind(&embedding.model.revision)
        .bind(embedding.model.dimensions as i32).bind(&embedding.model.recipe)
        .fetch_all(&mut **tx).await.map_err(storage_error)?;
    rows.into_iter()
        .map(|(id, value)| {
            if value.is_finite() {
                Ok((id, value as f32))
            } else {
                Err(Error::InternalInvariant)
            }
        })
        .collect()
}
