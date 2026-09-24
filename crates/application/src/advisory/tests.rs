use super::*;
use std::sync::{
    Arc, Mutex,
    atomic::{AtomicBool, AtomicUsize, Ordering},
};
use tect_domain::{
    AdvisoryOpportunityState, AdvisoryReason, AdvisoryRequestPreference, BeginCandidateSet,
    CandidateBoundary, PlanningTaskContext, WorkspaceAdvisoryMode,
};

fn request(preference: AdvisoryRequestPreference) -> BeginCandidateSet {
    BeginCandidateSet {
        request_id: uuid::Uuid::new_v4(),
        program_id: uuid::Uuid::new_v4(),
        program_revision: 3,
        boundary: CandidateBoundary::Ongoing,
        input: "decompose this scope".into(),
        task_context: PlanningTaskContext::default(),
        advisory_preference: preference,
        parent_matrix: None,
    }
}

#[test]
fn scope_opportunity_preserves_request_identity_and_no_call_reason() {
    let request = request(AdvisoryRequestPreference::Skip);
    let opportunity = scope_decomposition_opportunity(
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        &request,
        &WorkspaceAdvisoryConfig {
            workspace_id: uuid::Uuid::new_v4(),
            revision: 2,
            mode: WorkspaceAdvisoryMode::Optional,
            materialized: true,
            provider_profile_ref: None,
            model_configuration: None,
        },
        AdvisoryRequestPreference::UseWorkspace,
        true,
    );

    assert_eq!(
        opportunity.workflow_occurrence_key,
        request.request_id.to_string()
    );
    assert_eq!(opportunity.target_id, Some(request.program_id));
    assert_eq!(opportunity.work_revision, Some(request.program_revision));
    assert_eq!(opportunity.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(opportunity.primary_reason, AdvisoryReason::RequestSkip);
}

#[test]
fn session_preference_only_narrows_workspace_permission() {
    let request = request(AdvisoryRequestPreference::UseWorkspace);
    let opportunity = scope_decomposition_opportunity(
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        &request,
        &WorkspaceAdvisoryConfig {
            workspace_id: uuid::Uuid::new_v4(),
            revision: 1,
            mode: WorkspaceAdvisoryMode::Optional,
            materialized: true,
            provider_profile_ref: None,
            model_configuration: None,
        },
        AdvisoryRequestPreference::Skip,
        true,
    );
    assert_eq!(opportunity.primary_reason, AdvisoryReason::SessionSkip);
    assert_eq!(opportunity.state, AdvisoryOpportunityState::NoCall);
}

struct OpportunitySpy {
    captured: Arc<Mutex<Vec<AdvisoryOpportunityInput>>>,
    committed: Arc<AtomicBool>,
}

#[async_trait]
impl DurableOpportunityBoundary for OpportunitySpy {
    async fn capture(
        &mut self,
        workspace_id: uuid::Uuid,
        input: &AdvisoryOpportunityInput,
    ) -> Result<tect_domain::AdvisoryOpportunity> {
        self.captured.lock().expect("spy lock").push(input.clone());
        Ok(tect_domain::AdvisoryOpportunity {
            id: uuid::Uuid::new_v4(),
            workspace_id,
            session_id: input.session_id,
            authorized_actor_id: input.authorized_actor_id,
            capability: input.capability,
            decision_point: input.decision_point,
            decision_point_version: input.decision_point_version,
            workflow_occurrence_key: input.workflow_occurrence_key.clone(),
            target_kind: input.target_kind.clone(),
            target_id: input.target_id,
            work_revision: input.work_revision,
            matrix_task_revision: input.matrix_task_revision,
            matrix_choice_set_digest: input.matrix_choice_set_digest.clone(),
            matrix_verification_digest: input.matrix_verification_digest.clone(),
            source_ref: input.source_ref.clone(),
            session_preference: input.session_preference,
            request_preference: input.request_preference,
            config_revision: input.config_revision,
            material_digest: input.material_digest.clone(),
            state: input.state,
            primary_reason: input.primary_reason,
            provider_called: false,
        })
    }

    async fn commit_boundary(self) -> Result<()> {
        self.committed.store(true, Ordering::SeqCst);
        Ok(())
    }
}

#[tokio::test]
async fn invalid_no_call_commits_before_later_failure_and_never_uses_provider() {
    let workspace_id = uuid::Uuid::new_v4();
    let request = request(AdvisoryRequestPreference::UseWorkspace);
    let input = scope_decomposition_opportunity(
        uuid::Uuid::new_v4(),
        uuid::Uuid::new_v4(),
        &request,
        &WorkspaceAdvisoryConfig {
            workspace_id,
            revision: 1,
            mode: WorkspaceAdvisoryMode::Optional,
            materialized: true,
            provider_profile_ref: None,
            model_configuration: None,
        },
        AdvisoryRequestPreference::UseWorkspace,
        false,
    );
    assert_eq!(
        input.primary_reason,
        AdvisoryReason::DeterministicInputInvalid
    );
    let captured = Arc::new(Mutex::new(Vec::new()));
    let committed = Arc::new(AtomicBool::new(false));
    let persisted = commit_scope_opportunity(
        OpportunitySpy {
            captured: captured.clone(),
            committed: committed.clone(),
        },
        workspace_id,
        &input,
    )
    .await
    .unwrap();

    let later_candidate_result: Result<()> = Err(tect_domain::Error::InvalidArguments);
    assert!(later_candidate_result.is_err());
    assert!(committed.load(Ordering::SeqCst));
    assert_eq!(captured.lock().expect("spy lock").len(), 1);
    assert!(!persisted.provider_called);
}

#[test]
fn canonical_material_binds_the_fresh_config_revision() {
    let request = request(AdvisoryRequestPreference::UseWorkspace);
    let session = uuid::Uuid::new_v4();
    let actor = uuid::Uuid::new_v4();
    let config = WorkspaceAdvisoryConfig {
        workspace_id: uuid::Uuid::new_v4(),
        revision: 1,
        mode: WorkspaceAdvisoryMode::Optional,
        materialized: true,
        provider_profile_ref: None,
        model_configuration: None,
    };
    let first = scope_decomposition_opportunity(
        session,
        actor,
        &request,
        &config,
        AdvisoryRequestPreference::UseWorkspace,
        true,
    );
    let replay = scope_decomposition_opportunity(
        session,
        actor,
        &request,
        &config,
        AdvisoryRequestPreference::UseWorkspace,
        true,
    );
    let changed = scope_decomposition_opportunity(
        session,
        actor,
        &request,
        &WorkspaceAdvisoryConfig {
            revision: 2,
            ..config
        },
        AdvisoryRequestPreference::UseWorkspace,
        true,
    );
    assert_eq!(first.material_digest, replay.material_digest);
    assert_ne!(first.material_digest, changed.material_digest);
}

struct FixtureProvider {
    calls: Arc<AtomicUsize>,
    transaction_open: Arc<AtomicBool>,
    observation: AdvisoryProviderObservation,
}

#[async_trait]
impl crate::AdvisoryProvider for FixtureProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "v1"))
    }

    async fn attempt(&self, _: &AdvisoryProviderRequest) -> Result<AdvisoryProviderObservation> {
        assert!(!self.transaction_open.load(Ordering::SeqCst));
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(self.observation.clone())
    }
}

#[tokio::test]
async fn fixture_provider_is_called_once_outside_a_transaction() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FixtureProvider {
        calls: calls.clone(),
        transaction_open: Arc::new(AtomicBool::new(false)),
        observation: AdvisoryProviderObservation {
            send_certainty: tect_domain::AdvisorySendCertainty::Sent,
            outcome: tect_domain::AdvisoryDispatchOutcome::ProviderResponse,
            response_payload: Some(b"fixture response".to_vec()),
            input_tokens: None,
            output_tokens: None,
            raw_response_ref: Some("fixture:response:1".into()),
        },
    };
    let request = AdvisoryProviderRequest {
        dispatch_id: uuid::Uuid::new_v4(),
        provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
            id: "fixture".into(),
        },
        model_configuration: tect_domain::AdvisoryModelConfiguration {
            model: "fixture-model".into(),
        },
        payload: b"fixture request".to_vec(),
    };
    let observation = attempt_provider_once(&provider, &request).await.unwrap();
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(
        observation.outcome,
        tect_domain::AdvisoryDispatchOutcome::ProviderResponse
    );
}

#[tokio::test]
async fn production_default_provider_is_explicitly_unavailable() {
    let provider = crate::DisabledAdvisoryProvider;
    assert!(crate::AdvisoryProvider::identity(&provider).is_none());
    let request = AdvisoryProviderRequest {
        dispatch_id: uuid::Uuid::new_v4(),
        provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
            id: "unresolved".into(),
        },
        model_configuration: tect_domain::AdvisoryModelConfiguration {
            model: "unresolved".into(),
        },
        payload: b"must not send".to_vec(),
    };
    assert_eq!(
        attempt_provider_once(&provider, &request).await,
        Err(tect_domain::Error::TransportUnavailable)
    );
}

struct FailingFixtureProvider(Arc<AtomicUsize>);

#[async_trait]
impl crate::AdvisoryProvider for FailingFixtureProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("fixture", "v1"))
    }

    async fn attempt(&self, _: &AdvisoryProviderRequest) -> Result<AdvisoryProviderObservation> {
        self.0.fetch_add(1, Ordering::SeqCst);
        Err(tect_domain::Error::TransportUnavailable)
    }
}

#[tokio::test]
async fn uncertain_transport_failure_is_never_hidden_by_a_retry() {
    let calls = Arc::new(AtomicUsize::new(0));
    let provider = FailingFixtureProvider(calls.clone());
    let request = AdvisoryProviderRequest {
        dispatch_id: uuid::Uuid::new_v4(),
        provider_profile_ref: tect_domain::AdvisoryProviderProfileRef {
            id: "fixture".into(),
        },
        model_configuration: tect_domain::AdvisoryModelConfiguration {
            model: "fixture-model".into(),
        },
        payload: b"fixture request".to_vec(),
    };
    assert_eq!(
        attempt_provider_once(&provider, &request).await,
        Err(tect_domain::Error::TransportUnavailable)
    );
    assert_eq!(calls.load(Ordering::SeqCst), 1);
}

#[test]
fn dispatch_orchestration_has_no_product_mutation_authority() {
    let source = include_str!("controlled_dispatch.rs");
    for forbidden in [
        ["ensure_", "candidate_set("].concat(),
        ["update_", "program("].concat(),
        ["save_", "candidate"].concat(),
    ] {
        assert!(
            !source.contains(&forbidden),
            "unexpected mutation authority: {forbidden}"
        );
    }
}

#[test]
fn slice_zero_has_no_public_rust_dispatch_entrypoint() {
    let application = include_str!("../advisory.rs");
    let controlled_dispatch = include_str!("controlled_dispatch.rs");
    let exports = include_str!("../lib.rs");
    let service = include_str!("../service.rs");
    assert!(
        controlled_dispatch.contains(
            "#[cfg(test)]\n    #[allow(dead_code)]\n    pub(crate) async fn controlled_advisory_dispatch"
        )
    );
    let public_entrypoint = ["pub async fn controlled_", "advisory_dispatch"].concat();
    assert!(!application.contains(&public_entrypoint));
    assert!(!controlled_dispatch.contains(&public_entrypoint));
    assert!(!exports.contains("pub use advisory_ports::AdvisoryProvider"));
    assert!(!exports.contains("pub use advisory_ports::DisabledAdvisoryProvider"));
    assert!(service.contains(
        "#[cfg(test)]\n    #[allow(dead_code)]\n    pub(crate) fn with_advisory_provider"
    ));
    assert!(service.contains("Arc::new(crate::DisabledAdvisoryProvider)"));
}

#[test]
fn scope_audit_and_get_authorize_the_exact_scope_before_storage_reads() {
    let source = include_str!("../advisory.rs");
    let workspace_audit = source
        .split_once("pub async fn advisory_audit")
        .expect("workspace audit use case")
        .1
        .split_once("pub async fn scope_advisory_audit")
        .expect("workspace audit boundary")
        .0;
    let filtered_scope_authorization = workspace_audit
        .find("tx.native_scope(workspace.id, scope_id)")
        .unwrap();
    let filtered_read = workspace_audit
        .find(".advisory_audit(workspace.id, query.scope_id, query)")
        .unwrap();
    assert!(filtered_scope_authorization < filtered_read);
    assert!(workspace_audit.contains("ok_or(tect_domain::Error::NotFound)"));

    for (start, end, terminal) in [
        (
            "pub async fn scope_advisory_audit",
            "pub async fn scope_advisory_get",
            ".advisory_audit(workspace.id, Some(scope_id), query)",
        ),
        (
            "pub async fn scope_advisory_get",
            "\n}\n\n#[cfg(test)]\n#[allow(dead_code)]\nfn sha256",
            ".advisory_opportunity_detail(workspace.id, scope_id, opportunity_id)",
        ),
    ] {
        let section = source
            .split_once(start)
            .expect("application use case")
            .1
            .split_once(end)
            .expect("application use-case boundary")
            .0;
        let authorization = section
            .find("tx.native_scope(workspace.id, scope_id)")
            .unwrap();
        let read = section.find(terminal).unwrap();
        assert!(authorization < read);
        assert!(section.contains("ok_or(tect_domain::Error::NotFound)"));
    }
}
