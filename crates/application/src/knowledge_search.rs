use crate::{KnowledgeSearchOutputGuard, TransactionMode, WorkspaceService};
use sha2::{Digest, Sha256};
use std::time::Instant;
use tect_domain::*;
use uuid::Uuid;

fn digest(value: &str) -> String {
    format!("{:x}", Sha256::digest(value.as_bytes()))
}

fn normalize_query(value: &str) -> String {
    value
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ")
        .to_lowercase()
}

fn validate_embedding(values: &[f32]) -> Result<()> {
    if values.len() != KNOWLEDGE_EMBEDDING_DIMENSIONS as usize
        || values.iter().any(|value| !value.is_finite())
    {
        return Err(Error::KnowledgeUnavailable);
    }
    let norm = values
        .iter()
        .map(|value| f64::from(*value) * f64::from(*value))
        .sum::<f64>()
        .sqrt();
    ((norm - 1.0).abs() <= 0.001)
        .then_some(())
        .ok_or(Error::KnowledgeUnavailable)
}

impl WorkspaceService {
    pub async fn knowledge_search(
        &self,
        context: &RequestContext,
        query: &KnowledgeSearchQuery,
        guard: &dyn KnowledgeSearchOutputGuard,
    ) -> Result<KnowledgeSearchResponse> {
        query.validate()?;
        let model = (query.mode == KnowledgeSearchMode::SuperWide)
            .then(|| self.knowledge_embedding_provider.model())
            .flatten()
            .filter(|value| value.validate().is_ok());
        let (mut preflight_tx, workspace, session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let principal = preflight_tx.session_principal(session.id).await?;
        let preflight = preflight_tx
            .knowledge_search_preflight(workspace.id, principal, query, model.as_ref())
            .await?;
        preflight_tx.commit().await?;

        let embedding = self
            .query_embedding(workspace.id, principal, query, &preflight, model.as_ref())
            .await;
        let (mut search_tx, current_workspace, current_session) = self
            .native_planning_transaction(context, TransactionMode::ReadOnly)
            .await?;
        let current_principal = search_tx.session_principal(current_session.id).await?;
        if current_workspace.id != workspace.id || current_principal != principal {
            return Err(Error::ContextChanged);
        }
        let response = search_tx
            .knowledge_search(
                workspace.id,
                principal,
                query,
                &preflight,
                embedding.as_ref(),
            )
            .await?;
        guard.check(&response)?;
        search_tx.commit().await?;
        Ok(response)
    }

    async fn query_embedding(
        &self,
        workspace: Uuid,
        principal: Uuid,
        query: &KnowledgeSearchQuery,
        preflight: &KnowledgeSearchPreflight,
        model: Option<&KnowledgeEmbeddingModelIdentity>,
    ) -> Option<KnowledgeQueryEmbedding> {
        if query.mode != KnowledgeSearchMode::SuperWide
            || !preflight.vector_capability_ready
            || model.is_none()
        {
            return None;
        }
        let model = model.expect("checked");
        let normalized = preflight
            .normalized_query
            .clone()
            .unwrap_or_else(|| normalize_query(query.query.as_deref().unwrap_or_default()));
        let input = format!("query: {normalized}");
        let input_digest = digest(&input);
        let key = crate::service::KnowledgeQueryCacheKey::new(
            workspace,
            principal,
            preflight.workspace_generation,
            model,
            &input_digest,
        );
        if let Some(values) = self.query_embedding_cache.lock().ok()?.get(&key) {
            return Some(KnowledgeQueryEmbedding {
                model: model.clone(),
                input_digest,
                values,
                cache_hit: true,
                latency_ms: 0,
            });
        }
        let request = KnowledgeEmbeddingRequest {
            request_id: Uuid::new_v4(),
            purpose: KnowledgeEmbeddingPurpose::Query,
            text: input,
            input_digest: input_digest.clone(),
            model: model.clone(),
        };
        let started = Instant::now();
        let values = self
            .knowledge_embedding_provider
            .embed(&request)
            .await
            .ok()?;
        validate_embedding(&values).ok()?;
        let latency_ms = started.elapsed().as_millis().try_into().unwrap_or(u64::MAX);
        self.query_embedding_cache
            .lock()
            .ok()?
            .put(key, values.clone());
        Some(KnowledgeQueryEmbedding {
            model: model.clone(),
            input_digest,
            values,
            cache_hit: false,
            latency_ms,
        })
    }

    pub async fn process_knowledge_search_jobs(
        &self,
        context: &RequestContext,
        limit: u32,
    ) -> Result<KnowledgeSearchJobProcessOutcome> {
        if limit == 0 || limit > 64 {
            return Err(Error::InvalidArguments);
        }
        let mut outcome = KnowledgeSearchJobProcessOutcome {
            claimed: 0,
            published: 0,
            obsolete: 0,
            failed: 0,
            pending: 0,
        };
        let mut owner_scope = None;
        for _ in 0..limit {
            let (mut claim_tx, workspace, session) = self
                .native_planning_transaction(context, TransactionMode::ReadWrite)
                .await?;
            let principal = claim_tx.session_principal(session.id).await?;
            if !claim_tx.knowledge_owner(principal).await? {
                return Err(Error::Forbidden);
            }
            let Some(model) = self.knowledge_embedding_provider.model() else {
                claim_tx.commit().await?;
                return Ok(outcome);
            };
            model.validate()?;
            owner_scope.get_or_insert((workspace.id, principal));
            let claim = claim_tx
                .claim_knowledge_search_jobs(workspace.id, principal, 1, &model)
                .await?
                .into_iter()
                .next();
            claim_tx.commit().await?;
            let Some(claim) = claim else { break };
            outcome.claimed += 1;
            let input = format!("passage: {}", claim.title);
            if digest(&input) != claim.input_digest || claim.model != model {
                let (mut tx, current_workspace, current_session) = self
                    .native_planning_transaction(context, TransactionMode::ReadWrite)
                    .await?;
                let current_principal = tx.session_principal(current_session.id).await?;
                if current_workspace.id != claim.workspace_id
                    || current_principal != claim.principal_id
                {
                    return Err(Error::ContextChanged);
                }
                let kept = tx
                    .fail_knowledge_search_job(
                        current_workspace.id,
                        current_principal,
                        claim.job_id,
                        claim.lease_token,
                    )
                    .await?;
                tx.commit().await?;
                outcome.failed += u32::from(kept);
                outcome.obsolete += u32::from(!kept);
                continue;
            }
            let request = KnowledgeEmbeddingRequest {
                request_id: Uuid::new_v4(),
                purpose: KnowledgeEmbeddingPurpose::PassageTitle,
                text: input,
                input_digest: claim.input_digest.clone(),
                model: model.clone(),
            };
            let values = self.knowledge_embedding_provider.embed(&request).await;
            let Ok(values) = values.and_then(|values| {
                validate_embedding(&values)?;
                Ok(values)
            }) else {
                let (mut tx, current_workspace, current_session) = self
                    .native_planning_transaction(context, TransactionMode::ReadWrite)
                    .await?;
                let current_principal = tx.session_principal(current_session.id).await?;
                if current_workspace.id != claim.workspace_id
                    || current_principal != claim.principal_id
                {
                    return Err(Error::ContextChanged);
                }
                let kept = tx
                    .fail_knowledge_search_job(
                        current_workspace.id,
                        current_principal,
                        claim.job_id,
                        claim.lease_token,
                    )
                    .await?;
                tx.commit().await?;
                outcome.failed += u32::from(kept);
                outcome.obsolete += u32::from(!kept);
                continue;
            };
            let completion = KnowledgeEmbeddingJobCompletion {
                job_id: claim.job_id,
                lease_token: claim.lease_token,
                input_digest: claim.input_digest,
                model: model.clone(),
                values,
            };
            let (mut tx, current_workspace, current_session) = self
                .native_planning_transaction(context, TransactionMode::ReadWrite)
                .await?;
            let current_principal = tx.session_principal(current_session.id).await?;
            if current_workspace.id != claim.workspace_id || current_principal != claim.principal_id
            {
                return Err(Error::ContextChanged);
            }
            let published = tx
                .complete_knowledge_search_job(current_workspace.id, current_principal, &completion)
                .await?;
            tx.commit().await?;
            outcome.published += u32::from(published);
            outcome.obsolete += u32::from(!published);
        }
        if let Some((workspace, principal)) = owner_scope {
            let Some(model) = self.knowledge_embedding_provider.model() else {
                return Ok(outcome);
            };
            model.validate()?;
            let (mut tx, current_workspace, current_session) = self
                .native_planning_transaction(context, TransactionMode::ReadOnly)
                .await?;
            let current_principal = tx.session_principal(current_session.id).await?;
            if current_workspace.id != workspace || current_principal != principal {
                return Err(Error::ContextChanged);
            }
            outcome.pending = tx
                .pending_knowledge_search_jobs(workspace, principal, &model)
                .await?;
            tx.commit().await?;
        }
        Ok(outcome)
    }
}
