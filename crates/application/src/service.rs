use crate::{SetupFiles, SourceInspector, Store, TransactionMode, UnitOfWork};
use std::sync::Arc;
use tect_domain::{
    Error, EventKind, HostIdentity, PrincipalRole, RequestContext, Result, Session, Workspace,
    WorkspaceState,
};

pub struct WorkspaceService {
    store: Arc<dyn Store>,
    pub(crate) inspector: Arc<dyn SourceInspector>,
    pub(crate) setup_files: Arc<dyn SetupFiles>,
    #[allow(dead_code)]
    pub(crate) advisory_provider: Arc<dyn crate::AdvisoryProvider>,
    #[allow(dead_code)]
    pub(crate) matrix_advice_provider: Arc<dyn crate::MatrixAdviceProvider>,
    #[allow(dead_code)]
    pub(crate) matrix_budget: Arc<dyn crate::MatrixBudgetPolicy>,
    pub(crate) matrix_evidence_validator: Arc<dyn crate::MatrixEvidenceValidator>,
    pub(crate) pipeline_recommendation_definitions:
        Arc<dyn crate::PipelineRecommendationDefinitionProvider>,
    pub(crate) pipeline_recommendation_provider: Arc<dyn crate::PipelineRecommendationProvider>,
    pub(crate) pipeline_compatibility_policy: Arc<dyn crate::PipelineCompatibilityPolicyProvider>,
    pub(crate) scope_authority: Arc<dyn crate::ScopeAuthorityObserver>,
    pub(crate) scope_manifest_supplier: Arc<dyn crate::ScopeManifestSupplier>,
    pub(crate) scope_budget: Arc<dyn crate::ScopeBudgetPolicy>,
    pub(crate) scope_advice_provider: Arc<dyn crate::ScopeAdviceProvider>,
    pub(crate) anti_bloat_provider: Arc<dyn crate::AntiBloatRankingProvider>,
    #[allow(dead_code)]
    pub(crate) scope_caller: Arc<dyn crate::ScopeCaller>,
    #[allow(dead_code)]
    pub(crate) scope_verifier: Arc<dyn crate::ScopeVerifier>,
    pub(crate) knowledge_embedding_provider: Arc<dyn crate::KnowledgeEmbeddingProvider>,
    model_route_catalogue_provider: Arc<dyn crate::ModelRouteCatalogueProvider>,
    model_route_host_capabilities_provider: Arc<dyn crate::ModelRouteHostCapabilitiesProvider>,
    pub(crate) model_route_ranking_provider: Arc<dyn crate::ModelRouteRankingProvider>,
    pub(crate) query_embedding_cache: std::sync::Mutex<KnowledgeQueryCache>,
}

#[derive(Debug, Clone, PartialEq, Eq)]
pub(crate) struct KnowledgeQueryCacheKey(String);

impl KnowledgeQueryCacheKey {
    pub(crate) fn new(
        workspace: uuid::Uuid,
        principal: uuid::Uuid,
        generation: i64,
        model: &tect_domain::KnowledgeEmbeddingModelIdentity,
        input_digest: &str,
    ) -> Self {
        Self(format!(
            "{workspace}:{principal}:{generation}:{}:{}:{}:{input_digest}",
            model.name, model.revision, model.recipe
        ))
    }
}

pub(crate) struct KnowledgeQueryCache {
    entries: std::collections::VecDeque<(KnowledgeQueryCacheKey, std::time::Instant, Vec<f32>)>,
}

impl KnowledgeQueryCache {
    fn new() -> Self {
        Self {
            entries: std::collections::VecDeque::new(),
        }
    }

    pub(crate) fn get(&mut self, key: &KnowledgeQueryCacheKey) -> Option<Vec<f32>> {
        let now = std::time::Instant::now();
        self.entries
            .retain(|(_, created, _)| now.duration_since(*created).as_secs() <= 60);
        self.entries
            .iter()
            .find(|(candidate, _, _)| candidate == key)
            .map(|(_, _, values)| values.clone())
    }

    pub(crate) fn put(&mut self, key: KnowledgeQueryCacheKey, values: Vec<f32>) {
        self.entries.retain(|(candidate, _, _)| candidate != &key);
        if self.entries.len() >= 64 {
            self.entries.pop_front();
        }
        self.entries
            .push_back((key, std::time::Instant::now(), values));
    }
}

impl WorkspaceService {
    pub(crate) fn model_route_advisory_inputs(
        &self,
    ) -> (
        &dyn crate::ModelRouteHostCapabilitiesProvider,
        &dyn crate::ModelRouteCatalogueProvider,
    ) {
        (
            &*self.model_route_host_capabilities_provider,
            &*self.model_route_catalogue_provider,
        )
    }
    /// Installs only the authoritative Scope source adapters. The budget stays
    /// deny-by-default and the transport provider stays disabled.
    pub fn new_with_scope_sources(
        store: Arc<dyn Store>,
        inspector: Arc<dyn SourceInspector>,
        setup_files: Arc<dyn SetupFiles>,
        authority: Arc<dyn crate::ScopeAuthorityObserver>,
        supplier: Arc<dyn crate::ScopeManifestSupplier>,
    ) -> Self {
        let mut service = Self::new(store, inspector, setup_files);
        service.scope_authority = authority;
        service.scope_manifest_supplier = supplier;
        service
    }

    /// Explicit local/test composition seam. Default construction remains disabled.
    pub fn with_anti_bloat_provider(
        mut self,
        provider: Arc<dyn crate::AntiBloatRankingProvider>,
    ) -> Self {
        self.anti_bloat_provider = provider;
        self
    }

    pub fn new(
        store: Arc<dyn Store>,
        inspector: Arc<dyn SourceInspector>,
        setup_files: Arc<dyn SetupFiles>,
    ) -> Self {
        Self {
            store,
            inspector,
            setup_files,
            advisory_provider: Arc::new(crate::DisabledAdvisoryProvider),
            matrix_advice_provider: Arc::new(crate::DisabledMatrixAdviceProvider),
            matrix_budget: Arc::new(crate::DenyMatrixBudget),
            matrix_evidence_validator: Arc::new(crate::DisabledMatrixEvidenceValidator),
            pipeline_recommendation_definitions: Arc::new(
                crate::UnavailablePipelineRecommendationDefinitions,
            ),
            pipeline_recommendation_provider: Arc::new(
                crate::DisabledPipelineRecommendationProvider,
            ),
            pipeline_compatibility_policy: Arc::new(crate::UnavailablePipelineCompatibilityPolicy),
            scope_authority: Arc::new(crate::UnavailableScopeAuthorityObserver),
            scope_manifest_supplier: Arc::new(crate::UnavailableScopeManifestSupplier),
            scope_budget: Arc::new(crate::DenyScopeBudget),
            scope_advice_provider: Arc::new(crate::DisabledScopeAdviceProvider),
            anti_bloat_provider: Arc::new(crate::DisabledAntiBloatRankingProvider),
            scope_caller: Arc::new(crate::DisabledScopeCaller),
            scope_verifier: Arc::new(crate::DisabledScopeVerifier),
            knowledge_embedding_provider: Arc::new(crate::DisabledKnowledgeEmbeddingProvider),
            model_route_catalogue_provider: Arc::new(crate::UnavailableModelRouteCatalogue),
            model_route_host_capabilities_provider: Arc::new(
                crate::UnavailableModelRouteHostCapabilities,
            ),
            model_route_ranking_provider: Arc::new(crate::DisabledModelRouteRankingProvider),
            query_embedding_cache: std::sync::Mutex::new(KnowledgeQueryCache::new()),
        }
    }

    /// Explicit composition seam for a host that has selected its own budget
    /// policy and provider. Existing constructors retain their deny/disabled
    /// defaults; the daemon does not call this constructor.
    pub fn new_with_scope_advisory_adapters(
        store: Arc<dyn Store>,
        inspector: Arc<dyn SourceInspector>,
        setup_files: Arc<dyn SetupFiles>,
        authority: Arc<dyn crate::ScopeAuthorityObserver>,
        supplier: Arc<dyn crate::ScopeManifestSupplier>,
        budget: Arc<dyn crate::ScopeBudgetPolicy>,
        provider: Arc<dyn crate::ScopeAdviceProvider>,
    ) -> Self {
        let mut service =
            Self::new_with_scope_sources(store, inspector, setup_files, authority, supplier);
        service.scope_budget = budget;
        service.scope_advice_provider = provider;
        service
    }

    /// Test-only fixture hook for the separate legacy advisory provider.
    #[cfg(test)]
    #[allow(dead_code)]
    pub(crate) fn with_advisory_provider(
        mut self,
        provider: Arc<dyn crate::AdvisoryProvider>,
    ) -> Self {
        self.advisory_provider = provider;
        self
    }

    pub fn with_knowledge_embedding_provider(
        mut self,
        provider: Arc<dyn crate::KnowledgeEmbeddingProvider>,
    ) -> Self {
        self.knowledge_embedding_provider = provider;
        self
    }

    /// Install a host-owned immutable model-route catalogue. This does not
    /// select or dispatch a model and does not supply actual execution evidence.
    pub fn with_model_route_catalogue_provider(
        mut self,
        provider: Arc<dyn crate::ModelRouteCatalogueProvider>,
    ) -> Self {
        self.model_route_catalogue_provider = provider;
        self
    }

    pub fn model_route_catalogue(&self) -> Result<Option<tect_domain::ModelRouteCatalogue>> {
        self.model_route_catalogue_provider.catalogue()
    }

    /// Install a host-owned capability assertion; empty known capability sets
    /// remain distinct from absent evidence. This never dispatches a model.
    pub fn with_model_route_host_capabilities_provider(
        mut self,
        provider: Arc<dyn crate::ModelRouteHostCapabilitiesProvider>,
    ) -> Self {
        self.model_route_host_capabilities_provider = provider;
        self
    }

    pub fn model_route_host_capabilities(
        &self,
    ) -> Result<tect_domain::ModelRouteFact<Vec<String>>> {
        self.model_route_host_capabilities_provider
            .host_capabilities()
    }

    /// Explicit optional adviser injection; the normal constructor remains
    /// disabled. This never installs a model-execution dispatcher.
    pub fn with_model_route_ranking_provider(
        mut self,
        provider: Arc<dyn crate::ModelRouteRankingProvider>,
    ) -> Self {
        self.model_route_ranking_provider = provider;
        self
    }

    /// Explicit Matrix composition seam. Both decisions are selected by the
    /// embedding host; the normal constructor remains disabled and deny-all.
    pub fn with_matrix_advisory_adapters(
        mut self,
        provider: Arc<dyn crate::MatrixAdviceProvider>,
        budget: Arc<dyn crate::MatrixBudgetPolicy>,
    ) -> Self {
        self.matrix_advice_provider = provider;
        self.matrix_budget = budget;
        self
    }

    /// Provider-only composition keeps the independent budget deny-all.
    pub fn with_matrix_advice_provider(
        mut self,
        provider: Arc<dyn crate::MatrixAdviceProvider>,
    ) -> Self {
        self.matrix_advice_provider = provider;
        self
    }

    /// Explicit host composition; normal construction remains deny by default.
    pub fn with_matrix_evidence_validator(
        mut self,
        validator: Arc<dyn crate::MatrixEvidenceValidator>,
    ) -> Self {
        self.matrix_evidence_validator = validator;
        self
    }

    /// Install the immutable pipeline definitions for pre-open advice.
    pub fn with_pipeline_recommendation_definitions(
        mut self,
        provider: Arc<dyn crate::PipelineRecommendationDefinitionProvider>,
    ) -> Self {
        self.pipeline_recommendation_definitions = provider;
        self
    }

    /// Install an explicit provider; the normal constructor cannot dispatch.
    pub fn with_pipeline_recommendation_provider(
        mut self,
        provider: Arc<dyn crate::PipelineRecommendationProvider>,
    ) -> Self {
        self.pipeline_recommendation_provider = provider;
        self
    }

    pub fn with_pipeline_compatibility_policy(
        mut self,
        provider: Arc<dyn crate::PipelineCompatibilityPolicyProvider>,
    ) -> Self {
        self.pipeline_compatibility_policy = provider;
        self
    }

    pub(crate) fn pipeline_policy_matches(&self, digest: &str) -> Result<bool> {
        Ok(self.current_pipeline_policy()?.digest()? == digest)
    }

    pub(crate) fn current_pipeline_policy(
        &self,
    ) -> Result<tect_domain::PipelineCompatibilityPolicy> {
        Ok(self
            .pipeline_compatibility_policy
            .policy()?
            .unwrap_or_else(tect_domain::PipelineCompatibilityPolicy::unavailable))
    }

    pub(crate) async fn authorized(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(Box<dyn UnitOfWork>, HostIdentity)> {
        let (tx, identity) = self.authenticated(context, mode).await?;
        if identity.role != PrincipalRole::Owner {
            return Err(Error::Forbidden);
        }
        Ok((tx, identity))
    }

    pub(crate) async fn authenticated(
        &self,
        context: &RequestContext,
        mode: TransactionMode,
    ) -> Result<(Box<dyn UnitOfWork>, HostIdentity)> {
        context.validate()?;
        let mut tx = self.store.begin(mode).await?;
        let identity = tx.authenticate(&context.auth).await?;
        tx.set_tenant(identity.tenant_id).await?;
        Ok((tx, identity))
    }

    pub(crate) async fn validate_binding(
        tx: &mut dyn UnitOfWork,
        context: &RequestContext,
        identity: &HostIdentity,
        session: &Session,
    ) -> Result<Workspace> {
        if session.revoked {
            return Err(Error::SessionRevoked);
        }
        let workspace = tx
            .workspace(session.workspace_id)
            .await?
            .ok_or(Error::Forbidden)?;
        if workspace.key != context.workspace_key {
            return Err(Error::SessionWorkspaceMismatch);
        }
        if !tx.is_member(workspace.id, identity.principal_id).await? {
            return Err(Error::Forbidden);
        }
        Ok(workspace)
    }

    pub(crate) async fn state(
        tx: &mut dyn UnitOfWork,
        workspace: Workspace,
        session: Session,
    ) -> Result<WorkspaceState> {
        let selected_worktrees = tx
            .selected_worktrees(workspace.id, session.host_id, session.id)
            .await?;
        let mut state = WorkspaceState::opened(workspace, session);
        state.selected_worktrees = selected_worktrees;
        let entries = tx
            .list_programs(state.workspace.as_ref().expect("opened").id, None, 26)
            .await?;
        let page = crate::programs::bounded_program_list(entries, 25);
        state.setup_context = tx
            .setup_context(
                state.workspace.as_ref().expect("opened").id,
                state.session.as_ref().expect("opened").host_id,
                state.session.as_ref().expect("opened").id,
            )
            .await?;
        state.candidate_sets = tx
            .candidate_heads(state.workspace.as_ref().expect("opened").id, 25)
            .await?;
        state.native_planning = tx
            .native_planning_summaries(state.workspace.as_ref().expect("opened").id, 25)
            .await?;
        state.next_action = Some(
            if let Some(native) = state.native_planning.first() {
                match native.candidate_set_status {
                    tect_domain::SliceCandidateSetStatus::Ready
                        if !native.eligible_work.is_empty() =>
                    {
                        "slice_open"
                    }
                    _ if !native.slices_needing_result.is_empty() => "slice_result_record",
                    _ if !native.pipeline_runs.is_empty() => "slice_pipeline_context",
                    _ => "slice_candidate_context",
                }
            } else if !state.candidate_sets.is_empty() {
                "candidate_context"
            } else if state
                .setup_context
                .as_ref()
                .and_then(|context| context.setup.as_ref())
                .is_some_and(|setup| setup.status == tect_domain::SetupStatus::Draft)
            {
                "get_setup"
            } else if page.programs.is_empty() {
                "inspect_setup"
            } else {
                "get_program"
            }
            .into(),
        );
        state.programs = page.programs;
        state.next_after = page.next_after;
        Ok(state)
    }

    pub(crate) async fn bound_session(
        tx: &mut dyn UnitOfWork,
        context: &RequestContext,
        identity: &HostIdentity,
    ) -> Result<(Workspace, Session)> {
        if identity.role != PrincipalRole::Owner {
            return Err(Error::Forbidden);
        }
        let session = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
            .ok_or(Error::WorkspaceNotOpen)?;
        let workspace = Self::validate_binding(tx, context, identity, &session).await?;
        Ok((workspace, session))
    }

    pub async fn get_state(&self, context: &RequestContext) -> Result<WorkspaceState> {
        let (mut tx, identity) = self.authorized(context, TransactionMode::ReadOnly).await?;
        let state = match tx
            .session(identity.host_id, &context.native_session_id)
            .await?
        {
            Some(session) => {
                let workspace =
                    Self::validate_binding(&mut *tx, context, &identity, &session).await?;
                Self::state(&mut *tx, workspace, session).await?
            }
            None => WorkspaceState::unopened(),
        };
        tx.commit().await?;
        Ok(state)
    }

    /// Authenticate a native host call without requiring or creating workspace state.
    pub async fn authenticate_host(&self, context: &RequestContext) -> Result<()> {
        let (tx, _) = self.authorized(context, TransactionMode::ReadOnly).await?;
        tx.commit().await
    }

    pub async fn open_workspace(&self, context: &RequestContext) -> Result<WorkspaceState> {
        let (mut tx, identity) = self
            .authenticated(context, TransactionMode::ReadWrite)
            .await?;
        tx.lock_native_session(identity.host_id, &context.native_session_id)
            .await?;
        if let Some(session) = tx
            .session(identity.host_id, &context.native_session_id)
            .await?
        {
            let workspace = Self::validate_binding(&mut *tx, context, &identity, &session).await?;
            let state = if identity.role == PrincipalRole::Verifier {
                verifier_opened_state(workspace, session)
            } else {
                Self::state(&mut *tx, workspace, session).await?
            };
            tx.commit().await?;
            return Ok(state);
        }
        if identity.role == PrincipalRole::Verifier {
            let workspace = tx
                .workspace_by_key(&context.workspace_key)
                .await?
                .ok_or(Error::Forbidden)?;
            if !tx.is_member(workspace.id, identity.principal_id).await? {
                return Err(Error::Forbidden);
            }
            let session = tx
                .ensure_session(identity.host_id, workspace.id, &context.native_session_id)
                .await?;
            if session.value.revoked || session.value.workspace_id != workspace.id {
                return Err(Error::Forbidden);
            }
            let state = verifier_opened_state(workspace, session.value);
            tx.commit().await?;
            return Ok(state);
        }
        let workspace = tx.ensure_workspace(&context.workspace_key).await?;
        tx.ensure_membership(workspace.value.id, identity.principal_id)
            .await?;
        let session = tx
            .ensure_session(
                identity.host_id,
                workspace.value.id,
                &context.native_session_id,
            )
            .await?;
        if workspace.created {
            tx.append_creation_event(
                workspace.value.id,
                EventKind::WorkspaceOpened,
                workspace.value.id,
            )
            .await?;
        }
        if session.created {
            tx.append_creation_event(
                workspace.value.id,
                EventKind::SessionOpened,
                session.value.id,
            )
            .await?;
        }
        let state = Self::state(&mut *tx, workspace.value, session.value).await?;
        tx.commit().await?;
        Ok(state)
    }
}

fn verifier_opened_state(workspace: Workspace, session: Session) -> WorkspaceState {
    let mut state = WorkspaceState::opened(workspace, session);
    state.next_action = None;
    state
}
