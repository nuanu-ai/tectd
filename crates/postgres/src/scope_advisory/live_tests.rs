use super::live_support::{D, manifest, reseal_manifest, rw, set_config};
use super::*;
use crate::{PgStore, admin};
use tect_application::{
    DenyScopeBudget, PreparedScopeAdviceAttempt, ScopeAdviceProvider, ScopeAdviceProviderError,
    ScopeAdviceProviderObservation, ScopeAdviceProviderRequest, ScopeBudgetPolicy,
    ScopeBudgetPolicyEvaluation, ScopeBudgetRequest, SetupFiles, SourceInspector,
    StartedScopeDispatchPermit, WorkspaceService,
};

struct SyntheticPositiveBudget;

#[async_trait::async_trait]
impl ScopeBudgetPolicy for SyntheticPositiveBudget {
    async fn evaluate(
        &self,
        _: &ScopeBudgetRequest,
    ) -> Result<Option<ScopeBudgetPolicyEvaluation>> {
        Ok(Some(ScopeBudgetPolicyEvaluation {
            policy_id: "test-only-synthetic-positive".into(),
        }))
    }
}

async fn fake_jev_once() -> (
    reqwest::Url,
    tokio::sync::oneshot::Sender<()>,
    tokio::task::JoinHandle<(Vec<u8>, bool)>,
) {
    use tokio::io::{AsyncReadExt, AsyncWriteExt};
    let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = reqwest::Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let (done_sender, done_receiver) = tokio::sync::oneshot::channel();
    let server = tokio::spawn(async move {
        let (mut socket, _) = listener.accept().await.unwrap();
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let header_end = loop {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(position) = request.windows(4).position(|part| part == b"\r\n\r\n") {
                break position + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..header_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < header_end + length {
            let read = socket.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        let body = request[header_end..].to_vec();
        let parsed: serde_json::Value = serde_json::from_slice(&body).unwrap();
        let mut answers = serde_json::Map::new();
        for alternative in parsed["state"]["alternatives"].as_array().unwrap() {
            let id = alternative["id"].as_str().unwrap();
            answers.insert(
                format!("choice_{id}"),
                serde_json::json!({
                    "type":"choice", "choice":"PREFERRED", "confidence":0.8,
                    "probabilities":{"NON_PREFERRED":0.2,"PREFERRED":0.8}
                }),
            );
            answers.insert(
                format!("score_{id}"),
                serde_json::json!({
                    "type":"score", "score":2.4, "confidence":0.7,
                    "legend":{"0":"conflict","1":"weak_fit","2":"fit","3":"strong_fit"},
                    "probabilities":{"0":0.05,"1":0.1,"2":0.55,"3":0.3}
                }),
            );
        }
        let response = serde_json::to_vec(&serde_json::json!({
            "model":"jev", "answers":answers,
            "usage":{"input_tokens":11,"output_tokens":5}
        }))
        .unwrap();
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        socket.write_all(head.as_bytes()).await.unwrap();
        socket.write_all(&response).await.unwrap();
        let second_call = tokio::select! {
            biased;
            accepted = listener.accept() => accepted.is_ok(),
            _ = done_receiver => false,
        };
        (body, second_call)
    });
    (endpoint, done_sender, server)
}

struct CountingCapableProvider(std::sync::Arc<std::sync::atomic::AtomicUsize>);

#[async_trait::async_trait]
impl ScopeAdviceProvider for CountingCapableProvider {
    fn identity(&self) -> Option<(&'static str, &'static str)> {
        Some(("test-only", "fixture"))
    }

    fn prepare(
        &self,
        request: &tect_domain::ScopeAdviceRequest,
    ) -> std::result::Result<PreparedScopeAdviceAttempt, ScopeAdviceProviderError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        PreparedScopeAdviceAttempt::new(
            request.clone(),
            b"{}".to_vec(),
            "test-only".into(),
            "fixture".into(),
            "https://fixture.invalid".into(),
            "fixture.v1".into(),
        )
    }

    async fn attempt_prepared(
        &self,
        _: &ScopeAdviceProviderRequest,
        _: PreparedScopeAdviceAttempt,
        _: StartedScopeDispatchPermit,
    ) -> std::result::Result<ScopeAdviceProviderObservation, ScopeAdviceProviderError> {
        self.0.fetch_add(1, std::sync::atomic::Ordering::SeqCst);
        Err(ScopeAdviceProviderError::ProvenNotSent)
    }
}

struct UnusedHostAdapters;

#[async_trait::async_trait]
impl SourceInspector for UnusedHostAdapters {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<tect_domain::SourceLocation> {
        Err(Error::InternalInvariant)
    }
}

impl SetupFiles for UnusedHostAdapters {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<tect_domain::SetupDirectory> {
        Err(Error::InternalInvariant)
    }

    fn inspect(
        &self,
        _: &tect_domain::SetupDirectory,
        _: usize,
    ) -> Result<tect_domain::FileObservation> {
        Err(Error::InternalInvariant)
    }

    fn publish(
        &self,
        _: &tect_domain::SetupDirectory,
        _: &str,
    ) -> Result<tect_domain::FilePublication> {
        Err(Error::InternalInvariant)
    }
}

struct FixtureCandidateGuidance;

struct FixtureCandidateOutputGuard;

impl tect_application::CandidateOutputGuard for FixtureCandidateOutputGuard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        Ok(input.len() as i64)
    }
    fn check_material(&self, _: &CandidateSnapshotMaterial) -> Result<()> {
        Ok(())
    }
    fn check_draft(&self, _: &ResolvedCandidateDraft) -> Result<()> {
        Ok(())
    }
    fn check_stored(&self, _: &StoredCandidateContext) -> Result<()> {
        Ok(())
    }
    fn check_begin(&self, _: &BeginCandidateSetOutcome) -> Result<()> {
        Ok(())
    }
}

impl tect_application::CandidateGuidance for FixtureCandidateGuidance {
    fn snapshot(
        &self,
        program: Program,
        selected_worktrees: Vec<WorktreeSummary>,
    ) -> Result<CandidateSnapshotMaterial> {
        Ok(CandidateSnapshotMaterial {
            program,
            selected_worktrees,
            selected_sources_digest: D.into(),
            method: CandidateMethodSnapshot {
                id: "m".into(),
                revision: "4".into(),
                digest: D.into(),
                body: "body".into(),
                origin_refs: vec![],
            },
            registry_revision: "3".into(),
            registry_digest: D.into(),
            rules: vec![],
        })
    }
}

#[tokio::test]
#[ignore = "requires disposable PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn seven_aggregate_vertical_rejects_wrong_candidate_unresolved_partial_lineage_and_identity()
{
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let actor = enrollment.principal_id;
    let workspace = Uuid::new_v4();
    let session = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let snapshot = Uuid::new_v4();
    let mut source_refs = [Uuid::new_v4(), Uuid::new_v4()];
    source_refs.sort();
    let opportunity = Uuid::new_v4();
    let dispatch = Uuid::new_v4();
    let request_key = format!("request-{opportunity}");
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("scope-live-{workspace}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(tenant)
        .bind(workspace)
        .bind(actor)
        .execute(&pool)
        .await
        .unwrap();
    for id in [session, verifier_session] {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(id).bind(tenant).bind(enrollment.auth.host_id).bind(workspace).bind(id.to_string())
            .execute(&pool).await.unwrap();
    }
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',4,'p','i','b','finite','c','s','ready',2,2,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,revision,status,boundary,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}',3,'ready','finite',2,2,4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4()).execute(&pool).await.unwrap();
    let frozen_program_body = serde_json::json!({
        "id": program,
        "workspace_id": workspace,
        "status": "open",
        "revision": 4,
        "name": "p",
        "intent": "i",
        "basis": "b",
        "boundaries": "finite",
        "constraints": "c",
        "success": "s",
        "working_notes": null,
        "pending_question": null,
        "current_step": "ready",
        "input_cursor": 2,
        "latest_input": 2
    })
    .to_string();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,$4)")
        .bind(tenant).bind(workspace).bind(D).bind(frozen_program_body).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,1,4,2,2,$5,'{}',$5,'m','4',$5,'body','[]','3',$5,'[]')")
        .bind(snapshot).bind(tenant).bind(workspace).bind(candidate).bind(D).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_field','intent',$6,'intent')")
        .bind(source_refs[0]).bind(tenant).bind(workspace).bind(candidate).bind(snapshot)
        .bind(D).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(id,tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,body_digest,label) VALUES($1,$2,$3,$4,$5,'program_success',$6,'success')")
        .bind(source_refs[1]).bind(tenant).bind(workspace).bind(candidate).bind(snapshot)
        .bind(D).execute(&pool).await.unwrap();
    let blank_digest = "cccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccccc";
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'   ')")
        .bind(tenant).bind(workspace).bind(blank_digest).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','name',$5,'name')")
        .bind(tenant).bind(workspace).bind(candidate).bind(snapshot).bind(blank_digest)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(snapshot)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,0,NULL,'disabled',NULL,NULL,$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,1,0,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config(tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES($1,$2,1,'optional','fixture','{\"model\":\"jev\"}',$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&request_key).bind(D).execute(&pool).await.unwrap();
    let preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(preselection, (true, 0));

    let manifest = manifest(
        candidate,
        snapshot,
        program,
        &[(source_refs[0], D), (source_refs[1], D)],
    );
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let runtime_pool = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let authority =
        PgScopeAuthorityObserver::new(store.clone(), std::sync::Arc::new(FixtureCandidateGuidance));
    let authority_request = ScopeAuthorityRequest {
        tenant_id: tenant,
        workspace_id: workspace,
        actor_id: actor,
        session_id: session,
        candidate_set_id: candidate,
    };
    let observed = authority.observe(&authority_request).await.unwrap();
    let ScopeAuthorityOutcome::Authorized(observed) = observed else {
        panic!("persisted source must be authorized");
    };
    assert_eq!(observed.source, manifest.source);
    assert_eq!(observed.obligations, manifest.obligations);
    let service_authority = std::sync::Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        std::sync::Arc::new(FixtureCandidateGuidance),
    ));
    let service_supplier = std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        service_authority.clone(),
    ));
    let authored_scope_set = tect_application::AuthoredScopeSet {
        expected_candidate_set_revision: 3,
        baseline_key: "baseline".into(),
        alternatives: vec![tect_application::AuthoredScopeAlternative {
            key: "baseline".into(),
            kind: tect_domain::ScopeDecompositionKind::Cohesive,
            draft: serde_json::from_value(serde_json::json!({
                "boundary": "finite",
                "goals": [{
                    "identity": {"local": "goal"},
                    "text": "Preserve source",
                    "source_ref_id": source_refs[1],
                    "resolution": {"kind": "candidate", "reference": {"local": "candidate"}}
                }],
                "candidates": [{
                    "identity": {"local": "candidate"},
                    "title": "Cohesive",
                    "outcome": "Exact outcome",
                    "trigger": "Exact trigger",
                    "delivered_behavior": "Exact behavior",
                    "proof": "Exact proof",
                    "coverage_goals": [{"local": "goal"}]
                }]
            }))
            .unwrap(),
            covered_source_ref_ids: source_refs.to_vec(),
        }],
    };
    service_supplier
        .supply_authored(&tect_application::ScopeAuthoredManifestRequest {
            tenant_id: tenant,
            observation: observed.clone(),
            authored_scope_set: authored_scope_set.clone(),
        })
        .await
        .unwrap();
    let service = WorkspaceService::new_with_scope_sources(
        std::sync::Arc::new(store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        service_authority,
        service_supplier,
    );
    let no_call = service
        .run_scope_advisory(
            &tect_domain::RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: session.to_string(),
                workspace_key: format!("scope-live-{workspace}"),
            },
            &tect_application::RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id: candidate,
                session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                authored_scope_set: Some(authored_scope_set.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(no_call.opportunity.state, AdvisoryOpportunityState::NoCall);
    assert_eq!(
        no_call.opportunity.primary_reason,
        AdvisoryReason::CapabilityUnavailable
    );
    assert!(no_call.advice.is_none());
    let dispatch_count: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(no_call.opportunity.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(dispatch_count, 0);
    let provider_calls = std::sync::Arc::new(std::sync::atomic::AtomicUsize::new(0));
    let budget_service = WorkspaceService::new_with_scope_advisory_adapters(
        std::sync::Arc::new(store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(PgScopeAuthorityObserver::new(
            store.clone(),
            std::sync::Arc::new(FixtureCandidateGuidance),
        )),
        std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
            store.clone(),
            std::sync::Arc::new(PgScopeAuthorityObserver::new(
                store.clone(),
                std::sync::Arc::new(FixtureCandidateGuidance),
            )),
        )),
        std::sync::Arc::new(DenyScopeBudget),
        std::sync::Arc::new(CountingCapableProvider(provider_calls.clone())),
    );
    let budget_no_call = budget_service
        .run_scope_advisory(
            &tect_domain::RequestContext {
                auth: enrollment.auth.clone(),
                native_session_id: session.to_string(),
                workspace_key: format!("scope-live-{workspace}"),
            },
            &tect_application::RunScopeAdvisory {
                request_id: Uuid::new_v4(),
                candidate_set_id: candidate,
                session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
                authored_scope_set: Some(authored_scope_set.clone()),
            },
        )
        .await
        .unwrap();
    assert_eq!(
        budget_no_call.opportunity.state,
        AdvisoryOpportunityState::NoCall
    );
    assert_eq!(
        budget_no_call.opportunity.primary_reason,
        AdvisoryReason::BudgetPolicyInvalid
    );
    assert!(budget_no_call.advice.is_none());
    assert_eq!(provider_calls.load(std::sync::atomic::Ordering::SeqCst), 0);
    let (state, reason): (String, String) =
        sqlx::query_as("SELECT state,primary_reason FROM advisory_opportunity WHERE id=$1")
            .bind(budget_no_call.opportunity.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        (state.as_str(), reason.as_str()),
        ("no_call", "budget_policy_invalid")
    );
    let budget_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(budget_no_call.opportunity.id)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(budget_dispatches, 0);

    // Explicit test-only opt-in crosses the real Jev HTTP adapter and the
    // committed dispatch lifecycle. The same request is replayed while the
    // listener is open so an accidental retry is visible on the socket.
    let (endpoint, fake_done, fake_server) = fake_jev_once().await;
    let jev = tect_host::JevScopeAdviceProvider::new(
        tect_host::JevScopeAdviceConfig {
            profile: "fixture".into(),
            endpoint: endpoint.clone(),
            model: "jev".into(),
            timeout: std::time::Duration::from_secs(2),
            maximum_request_bytes: 65_536,
            maximum_response_bytes: 65_536,
        },
        "test-only-credential".into(),
    )
    .unwrap();
    let positive_service = WorkspaceService::new_with_scope_advisory_adapters(
        std::sync::Arc::new(store.clone()),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(UnusedHostAdapters),
        std::sync::Arc::new(PgScopeAuthorityObserver::new(
            store.clone(),
            std::sync::Arc::new(FixtureCandidateGuidance),
        )),
        std::sync::Arc::new(PgScopeAuthoredManifestSupplier::new(
            store.clone(),
            std::sync::Arc::new(PgScopeAuthorityObserver::new(
                store.clone(),
                std::sync::Arc::new(FixtureCandidateGuidance),
            )),
        )),
        std::sync::Arc::new(SyntheticPositiveBudget),
        std::sync::Arc::new(jev),
    );
    let positive_context = tect_domain::RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key: format!("scope-live-{workspace}"),
    };
    let positive_request = tect_application::RunScopeAdvisory {
        request_id: Uuid::new_v4(),
        candidate_set_id: candidate,
        session_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        request_preference: tect_domain::AdvisoryRequestPreference::UseWorkspace,
        authored_scope_set: Some(authored_scope_set.clone()),
    };
    let positive = positive_service
        .run_scope_advisory(&positive_context, &positive_request)
        .await
        .unwrap();
    assert_eq!(
        positive.opportunity.state,
        AdvisoryOpportunityState::Advised
    );
    assert_eq!(
        positive.opportunity.primary_reason,
        AdvisoryReason::ProviderResponse
    );
    assert!(positive.advice.is_some());
    let replay = positive_service
        .run_scope_advisory(&positive_context, &positive_request)
        .await
        .unwrap();
    assert_eq!(replay.opportunity.id, positive.opportunity.id);
    assert_eq!(replay.opportunity.state, positive.opportunity.state);
    assert_eq!(replay.advice, positive.advice);
    assert!(!replay.opportunity.provider_called);
    fake_done.send(()).unwrap();
    let (received_body, second_call) = fake_server.await.unwrap();
    assert!(!second_call, "replay sent a second HTTP request");
    let dispatch_rows: Vec<(i32, String, String, String, String, String, String, serde_json::Value, Vec<u8>, String, String, Option<String>, bool, bool)> = sqlx::query_as(
        "SELECT attempt_number,provider,model,state,send_certainty,outcome,retry_basis,configuration_snapshot,request_payload,payload_digest,material_digest,raw_response_ref,send_started_at IS NOT NULL,sealed_at IS NOT NULL FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 ORDER BY attempt_number",
    )
    .bind(tenant).bind(workspace).bind(positive.opportunity.id)
    .fetch_all(&pool).await.unwrap();
    assert_eq!(dispatch_rows.len(), 1);
    let (
        attempt,
        provider_name,
        model,
        state,
        certainty,
        outcome,
        retry_basis,
        config_snapshot,
        request_payload,
        payload_digest,
        material_digest,
        raw_ref,
        started,
        sealed,
    ) = &dispatch_rows[0];
    assert_eq!(
        (*attempt, provider_name.as_str(), model.as_str()),
        (1, "jev-system-one", "jev")
    );
    assert_eq!(
        (
            state.as_str(),
            certainty.as_str(),
            outcome.as_str(),
            retry_basis.as_str()
        ),
        ("sealed", "sent", "provider_response", "initial")
    );
    assert!(*started && *sealed);
    assert_eq!(
        config_snapshot["budget_policy_id"],
        "test-only-synthetic-positive"
    );
    assert_eq!(config_snapshot["destination"], endpoint.as_str());
    assert_eq!(request_payload, &received_body);
    assert_eq!(
        payload_digest,
        &format!("{:x}", sha2::Sha256::digest(&received_body))
    );
    assert_eq!(material_digest, &positive.opportunity.material_digest);
    assert!(raw_ref.as_deref().unwrap().contains("jev:fixture:"));
    let receipt: (Vec<u8>, Option<i64>, Option<i64>) = sqlx::query_as(
        "SELECT response_payload,input_tokens,output_tokens FROM advisory_dispatch WHERE opportunity_id=$1",
    ).bind(positive.opportunity.id).fetch_one(&pool).await.unwrap();
    assert_eq!(receipt.1, Some(11));
    assert_eq!(receipt.2, Some(5));
    assert_eq!(
        serde_json::from_slice::<serde_json::Value>(&receipt.0).unwrap()["model"],
        "jev"
    );
    let audit: (String, String, i64) = sqlx::query_as(
        "SELECT state,primary_reason,(SELECT count(*) FROM advisory_dispatch d WHERE d.opportunity_id=o.id)::bigint FROM advisory_opportunity o WHERE id=$1",
    ).bind(positive.opportunity.id).fetch_one(&pool).await.unwrap();
    assert_eq!(audit, ("advised".into(), "provider_response".into(), 1));
    let attribution: (String, String, String, Uuid, String) = sqlx::query_as(
        "SELECT capability,decision_point,work_item_kind,work_item_id,source_revision FROM advisory_opportunity WHERE id=$1",
    ).bind(positive.opportunity.id).fetch_one(&pool).await.unwrap();
    assert_eq!(
        attribution,
        (
            "scope_decomposition".into(),
            "scope.decomposition.before_selection".into(),
            "scope_candidate_set".into(),
            candidate,
            "3".into(),
        )
    );

    assert_eq!(
        authority
            .observe(&ScopeAuthorityRequest {
                actor_id: Uuid::new_v4(),
                ..authority_request
            })
            .await,
        Err(Error::Forbidden)
    );
    let prepared = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert!(
        unit.scope_advisory_manifest_by_request_key(workspace, &request_key)
            .await
            .unwrap()
            .is_none()
    );
    assert!(
        unit.scope_advisory_manifest_by_request_key(Uuid::new_v4(), &request_key)
            .await
            .unwrap()
            .is_none()
    );
    let wrong = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: Uuid::new_v4(),
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &wrong)
            .await,
        Err(Error::InputConflict)
    );
    let mut rejected_baseline = manifest.clone();
    let mut rejected = rejected_baseline.emitted[0].clone();
    rejected.material.candidates[0].title = "Rejected".into();
    rejected.material_digest =
        scope_candidate_material_digest(&Sha256ScopeDigest, &rejected.material).unwrap();
    rejected.id = stable_scope_alternative_id(
        &Sha256ScopeDigest,
        &rejected_baseline.constructor,
        &rejected_baseline.source.digest,
        rejected.kind,
        &rejected.material_digest,
        &rejected.coverage,
    )
    .unwrap();
    rejected_baseline.rejected = vec![RejectedScopeAlternative {
        alternative: rejected.clone(),
        reason_codes: vec!["not_eligible".into()],
    }];
    rejected_baseline.baseline_id = rejected.id.clone();
    rejected_baseline.ordered_ids.push(rejected.id);
    rejected_baseline.ordered_ids.sort();
    rejected_baseline.whole_set_digest = rejected_baseline
        .canonical_whole_set_digest(&Sha256ScopeDigest)
        .unwrap();
    let rejected_record = ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: rejected_baseline,
    };
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &rejected_record)
            .await,
        Err(Error::InvalidArguments)
    );
    let make_record = |manifest: ScopeConstructorManifest| ScopeManifestRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest,
    };
    let missing =
        super::live_support::manifest(candidate, snapshot, program, &[(source_refs[0], D)]);
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(missing))
            .await,
        Err(Error::InvalidSource)
    );
    let extra = super::live_support::manifest(
        candidate,
        snapshot,
        program,
        &[
            (source_refs[0], D),
            (source_refs[1], D),
            (Uuid::new_v4(), D),
        ],
    );
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(extra))
            .await,
        Err(Error::InvalidSource)
    );
    let different_digest = "bbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbbb";
    let altered = super::live_support::manifest(
        candidate,
        snapshot,
        program,
        &[(source_refs[0], D), (source_refs[1], different_digest)],
    );
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(altered))
            .await,
        Err(Error::InvalidSource)
    );
    let mut omitted_coverage = manifest.clone();
    omitted_coverage.emitted[0].coverage.pop();
    reseal_manifest(&mut omitted_coverage);
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(omitted_coverage))
            .await,
        Err(Error::InvalidSource)
    );
    let mut stale_snapshot = manifest.clone();
    stale_snapshot.source.snapshot_id = Uuid::new_v4();
    reseal_manifest(&mut stale_snapshot);
    assert_eq!(
        unit.prepare_scope_advisory_manifest(workspace, &make_record(stale_snapshot))
            .await,
        Err(Error::StaleRevision)
    );
    let authored_request_digest = "a".repeat(64);
    unit.prepare_authored_scope_advisory_manifest(workspace, &prepared, &authored_request_digest)
        .await
        .unwrap();
    let stored = unit
        .scope_advisory_manifest_by_request_key(workspace, &request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored.record, prepared);
    assert_eq!(
        stored.authored_request_digest.as_deref(),
        Some(authored_request_digest.as_str())
    );
    unit.commit().await.unwrap();
    let mut replay = rw(&store, &enrollment.auth, tenant).await;
    let stored_replay = replay
        .scope_advisory_manifest_by_request_key(workspace, &request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(stored_replay, stored);
    assert_eq!(
        replay
            .prepare_authored_scope_advisory_manifest(workspace, &prepared, &"b".repeat(64),)
            .await,
        Err(Error::InputConflict)
    );
    replay.commit().await.unwrap();

    let stale_opportunity = Uuid::new_v4();
    let stale_request_key = format!("request-{stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&stale_request_key).bind(D).execute(&pool).await.unwrap();
    let stale_prepared = ScopeManifestRecord {
        opportunity_id: stale_opportunity,
        candidate_set_id: candidate,
        config_revision: prepared.config_revision,
        opportunity_material_digest: prepared.opportunity_material_digest.clone(),
        manifest: manifest.clone(),
    };
    let stale_disposition = ScopePreparedAdvisoryDisposition {
        opportunity_id: stale_opportunity,
        candidate_set_id: candidate,
        expected_source_digest: manifest.source.digest.clone(),
        reason: AdvisoryReason::DeterministicInputInvalid,
    };
    let mut stale_unit = rw(&store, &enrollment.auth, tenant).await;
    stale_unit
        .prepare_scope_advisory_manifest(workspace, &stale_prepared)
        .await
        .unwrap();
    stale_unit
        .finalize_prepared_scope_advisory_without_dispatch(workspace, &stale_disposition)
        .await
        .unwrap();
    stale_unit.commit().await.unwrap();
    let mut stale_replay = rw(&store, &enrollment.auth, tenant).await;
    stale_replay
        .finalize_prepared_scope_advisory_without_dispatch(workspace, &stale_disposition)
        .await
        .unwrap();
    stale_replay.commit().await.unwrap();
    let stale_state: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        stale_state,
        ("no_call".into(), "deterministic_input_invalid".into())
    );
    let stale_attempts: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_attempts, 0);

    sqlx::query("UPDATE advisory_opportunity SET state='awaiting_response',primary_reason='send_unknown' WHERE id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(dispatch).bind(tenant).bind(workspace).bind(opportunity).bind(D).execute(&pool).await.unwrap();
    let dispatched_invalidation = ScopePreparedAdvisoryDisposition {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        expected_source_digest: manifest.source.digest.clone(),
        reason: AdvisoryReason::DeterministicInputInvalid,
    };
    let mut dispatched_unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        dispatched_unit
            .finalize_prepared_scope_advisory_without_dispatch(workspace, &dispatched_invalidation,)
            .await,
        Err(Error::InputConflict)
    );
    dispatched_unit.commit().await.unwrap();
    let request = ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let answers = NormalizedScopeAdviceAnswers {
        answers: vec![NormalizedScopeAdviceAnswer {
            alternative_id: manifest.baseline_id.clone(),
            choice: ScopeAdviceChoice::Preferred,
            score: ScopeAdviceScoreBand::StrongFit,
            choice_confidence: ConfidenceBasisPoints(9000),
            score_confidence: ConfidenceBasisPoints(8000),
        }],
    };
    let advice = guard_scope_advice(
        &Sha256ScopeDigest,
        opportunity,
        &manifest,
        &request,
        &answers,
    )
    .unwrap();
    let advice_record = GuardedScopeAdviceRecord {
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        dispatch_id: dispatch,
        dispatch_material_digest: D.into(),
        config_revision: 1,
        advice: advice.clone(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_guarded_scope_advice(workspace, &advice_record)
            .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind(dispatch).execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    unit.persist_guarded_scope_advice(workspace, &advice_record)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let read_context = tect_domain::RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key: format!("scope-live-{workspace}"),
    };
    let read = service
        .candidate_advisory_get(&read_context, candidate, opportunity)
        .await
        .unwrap();
    let projected = read.scope_decomposition.unwrap();
    assert_eq!(projected.version, 1);
    assert_eq!(projected.manifest, manifest);
    assert_eq!(projected.advice, advice);
    assert_eq!(
        projected.manifest.baseline_id,
        projected.advice.ranked_ids[0]
    );
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_guarded_scope_advice(workspace, &advice_record)
            .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;

    // Two valid advice records in one workspace compete for a single request ID.
    // Both transactions start together; the winner commits before the loser checks replay.
    let other_opportunity = Uuid::new_v4();
    let other_dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(other_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(format!("request-{other_opportunity}")).bind(D).execute(&pool).await.unwrap();
    let mut setup = rw(&store, &enrollment.auth, tenant).await;
    setup
        .prepare_scope_advisory_manifest(
            workspace,
            &ScopeManifestRecord {
                opportunity_id: other_opportunity,
                ..prepared.clone()
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(other_dispatch).bind(tenant).bind(workspace).bind(other_opportunity).bind(D)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind(other_dispatch).execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(other_opportunity).execute(&pool).await.unwrap();
    let other_advice = guard_scope_advice(
        &Sha256ScopeDigest,
        other_opportunity,
        &manifest,
        &request,
        &answers,
    )
    .unwrap();
    assert_eq!(
        advice.content_digest(&Sha256ScopeDigest).unwrap(),
        other_advice.content_digest(&Sha256ScopeDigest).unwrap()
    );
    assert_ne!(advice.id, other_advice.id);
    let mut setup = rw(&store, &enrollment.auth, tenant).await;
    setup
        .persist_guarded_scope_advice(
            workspace,
            &GuardedScopeAdviceRecord {
                opportunity_id: other_opportunity,
                candidate_set_id: candidate,
                dispatch_id: other_dispatch,
                dispatch_material_digest: D.into(),
                config_revision: 1,
                advice: other_advice.clone(),
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    let mut replay = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        replay
            .persist_guarded_scope_advice(
                workspace,
                &GuardedScopeAdviceRecord {
                    opportunity_id: other_opportunity,
                    candidate_set_id: candidate,
                    dispatch_id: other_dispatch,
                    dispatch_material_digest: D.into(),
                    config_revision: 1,
                    advice: other_advice.clone(),
                }
            )
            .await
            .unwrap(),
        other_advice
    );
    replay.commit().await.unwrap();
    // A persisted v1 aggregate has no opportunity field and uses the content hash as its ID.
    let mut legacy_advice = other_advice.clone();
    legacy_advice.id = ScopeAdviceId(legacy_advice.content_digest(&Sha256ScopeDigest).unwrap());
    legacy_advice.opportunity_id = None;
    sqlx::query("UPDATE advisory_scope_advice SET advice_id=$1,aggregate_payload=$2 WHERE tenant_id=$3 AND workspace_id=$4 AND opportunity_id=$5")
        .bind(&legacy_advice.id.0)
        .bind(serde_json::to_value(&legacy_advice).unwrap())
        .bind(tenant).bind(workspace).bind(other_opportunity)
        .execute(&pool).await.unwrap();
    let mut legacy_read = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        legacy_read
            .guarded_scope_advice(workspace, other_opportunity)
            .await
            .unwrap(),
        Some(legacy_advice)
    );
    legacy_read.commit().await.unwrap();
    sqlx::query("UPDATE advisory_scope_advice SET advice_id=$1,aggregate_payload=$2 WHERE tenant_id=$3 AND workspace_id=$4 AND opportunity_id=$5")
        .bind(&other_advice.id.0)
        .bind(serde_json::to_value(&other_advice).unwrap())
        .bind(tenant).bind(workspace).bind(other_opportunity)
        .execute(&pool).await.unwrap();
    let third_opportunity = Uuid::new_v4();
    let third_dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(third_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(format!("request-{third_opportunity}")).bind(D).execute(&pool).await.unwrap();
    let mut setup = rw(&store, &enrollment.auth, tenant).await;
    setup
        .prepare_scope_advisory_manifest(
            workspace,
            &ScopeManifestRecord {
                opportunity_id: third_opportunity,
                ..prepared.clone()
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,state,send_certainty,retry_basis,send_started_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','sending','sent_unknown','initial',clock_timestamp())")
        .bind(third_dispatch).bind(tenant).bind(workspace).bind(third_opportunity).bind(D)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_dispatch SET response_payload='y',state='sealed',send_certainty='sent',outcome='provider_response',sealed_at=clock_timestamp() WHERE id=$1")
        .bind(third_dispatch).execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(third_opportunity).execute(&pool).await.unwrap();
    let mut third_answers = answers.clone();
    third_answers.answers[0].choice_confidence = ConfidenceBasisPoints(8800);
    let third_advice = guard_scope_advice(
        &Sha256ScopeDigest,
        third_opportunity,
        &manifest,
        &request,
        &third_answers,
    )
    .unwrap();
    assert_ne!(other_advice.id, third_advice.id);
    let mut setup = rw(&store, &enrollment.auth, tenant).await;
    setup
        .persist_guarded_scope_advice(
            workspace,
            &GuardedScopeAdviceRecord {
                opportunity_id: third_opportunity,
                candidate_set_id: candidate,
                dispatch_id: third_dispatch,
                dispatch_material_digest: D.into(),
                config_revision: 1,
                advice: third_advice.clone(),
            },
        )
        .await
        .unwrap();
    setup.commit().await.unwrap();
    let race_request_id = Uuid::new_v4();
    let race_barrier = std::sync::Arc::new(tokio::sync::Barrier::new(2));
    let race = |opportunity_id,
                advice_id: ScopeAdviceId,
                barrier: std::sync::Arc<tokio::sync::Barrier>| {
        let store = store.clone();
        let auth = enrollment.auth.clone();
        let selected_id = manifest.baseline_id.clone();
        async move {
            let mut unit = rw(&store, &auth, tenant).await;
            barrier.wait().await;
            let result = unit
                .cas_scope_advisory_disposition(
                    workspace,
                    ScopeDispositionRecord {
                        opportunity_id,
                        candidate_set_id: candidate,
                        actor_id: actor,
                        session_id: session,
                        request: ScopeDispositionRequest {
                            request_id: race_request_id,
                            advice_id,
                            expected_revision: 0,
                            action: ScopeDispositionAction::Accept,
                            selected_id: Some(selected_id.clone()),
                            items: vec![ScopeDispositionItem {
                                alternative_id: selected_id,
                                state: ScopeDispositionItemState::Selected,
                            }],
                            rationale: "race".into(),
                        },
                    },
                )
                .await;
            if result.is_ok() {
                unit.commit().await.unwrap();
            }
            result
        }
    };
    let (first, second) = tokio::join!(
        race(
            other_opportunity,
            other_advice.id.clone(),
            race_barrier.clone()
        ),
        race(third_opportunity, third_advice.id.clone(), race_barrier),
    );
    assert!(
        matches!(
            (&first, &second),
            (Ok(_), Err(Error::InputConflict)) | (Err(Error::InputConflict), Ok(_))
        ),
        "same request across distinct advice IDs must have one success and one InputConflict: {first:?}, {second:?}"
    );

    let item = ScopeDispositionItem {
        alternative_id: manifest.baseline_id.clone(),
        state: ScopeDispositionItemState::Selected,
    };
    let mut partial = ScopeDispositionRequest {
        request_id: Uuid::new_v4(),
        advice_id: projected.advice.id.clone(),
        expected_revision: 0,
        action: ScopeDispositionAction::Accept,
        selected_id: Some(manifest.baseline_id.clone()),
        items: vec![],
        rationale: "accept".into(),
    };
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: partial.clone(),
            }
        )
        .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: partial.clone()
            }
        )
        .await,
        Err(Error::InvalidArguments)
    );
    partial.items = vec![item];
    drop(unit);
    let context = tect_domain::RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: session.to_string(),
        workspace_key: format!("scope-live-{workspace}"),
    };
    let disposition = service
        .decide_scope_advisory(&context, opportunity, candidate, partial.clone())
        .await
        .unwrap();
    assert_eq!(
        service
            .decide_scope_advisory(&context, opportunity, candidate, partial.clone())
            .await
            .unwrap(),
        disposition
    );
    let other_disposition_opportunity = if first.is_ok() {
        other_opportunity
    } else {
        third_opportunity
    };
    let separate_dispositions: i64 = sqlx::query_scalar(
        "SELECT count(DISTINCT opportunity_id) FROM advisory_scope_disposition WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id = ANY($3)",
    )
    .bind(tenant).bind(workspace).bind(vec![opportunity, other_disposition_opportunity])
    .fetch_one(&pool).await.unwrap();
    assert_eq!(separate_dispositions, 2);
    let mut changed_lineage = partial.clone();
    changed_lineage.advice_id = ScopeAdviceId(D.into());
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: changed_lineage
            }
        )
        .await,
        Err(Error::InputConflict)
    );
    drop(unit);
    let mut competing = partial.clone();
    competing.request_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: competing
            }
        )
        .await,
        Err(Error::StaleRevision)
    );
    drop(unit);

    let observation = FreshScopeObservation {
        source: manifest.source.clone(),
        manifest: manifest.clone(),
        candidate_set_revision: 3,
        advice_id: advice.id.clone(),
    };
    let preservation = evaluate_scope_preservation(
        &Sha256ScopeDigest,
        &manifest,
        &advice,
        &disposition,
        &observation,
    )
    .unwrap();
    let preservation_id = Uuid::new_v4();
    let preservation_input = ScopePreservationReceiptInput {
        receipt_id: preservation_id,
        request_id: Uuid::new_v4(),
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        disposition_id: disposition.id,
        observation: observation.clone(),
        result: preservation.clone(),
    };
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_scope_preservation_receipt(workspace, &preservation_input)
            .await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    unit.persist_scope_preservation_receipt(workspace, &preservation_input)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let audit_query = AdvisoryAuditQuery {
        limit: 1,
        scope_id: None,
        after: None,
        capability: None,
        decision_point: None,
        reason: None,
        state: None,
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    let before_caller = unit
        .candidate_advisory_opportunity_detail(workspace, candidate, opportunity)
        .await
        .unwrap()
        .opportunity;
    assert_eq!(before_caller.guarded_advice_id, None);
    assert_eq!(
        before_caller.guarded_advice_digest.as_deref(),
        Some(advice.id.0.as_str())
    );
    assert_eq!(before_caller.disposition_id, Some(disposition.id));
    assert_eq!(before_caller.preservation_receipt_id, Some(preservation_id));
    assert_eq!(before_caller.preservation_status.as_deref(), Some("passed"));
    assert_eq!(before_caller.caller_receipt_id, None);
    assert_eq!(before_caller.verifier_receipt_id, None);
    drop(unit);
    let mut changed_preservation = preservation_input.clone();
    changed_preservation.disposition_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_scope_preservation_receipt(workspace, &changed_preservation)
            .await,
        Err(Error::InputConflict)
    );
    drop(unit);
    let caller_request = Uuid::new_v4();
    sqlx::query("INSERT INTO scope_candidate_receipts(tenant_id,workspace_id,candidate_set_id,operation,request_id,request_payload,result_revision,result_payload) VALUES($1,$2,$3,'save_review',$4,'{}',3,'{}')")
        .bind(tenant).bind(workspace).bind(candidate).bind(caller_request).execute(&pool).await.unwrap();
    let caller = ScopeCallerLinkInput {
        link_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        disposition_id: disposition.id,
        preservation_receipt_id: preservation_id,
        caller_operation: "save_review".into(),
        caller_request_id: caller_request,
        caller_result_revision: 3,
        actor_id: actor,
        session_id: session,
    };
    set_config(&pool, tenant, workspace, false).await;
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &caller).await,
        Err(Error::StaleContext)
    );
    drop(unit);
    set_config(&pool, tenant, workspace, true).await;
    let failed_preservation_id = Uuid::new_v4();
    let mut failed_result = preservation.clone();
    failed_result.status = ScopePreservationStatus::Failed;
    failed_result.reason_codes = vec!["source_changed".into()];
    sqlx::query("INSERT INTO advisory_scope_preservation_receipt(tenant_id,workspace_id,opportunity_id,candidate_set_id,receipt_id,request_id,advice_id,disposition_id,disposition_revision,source_digest,manifest_digest,eligible_set_digest,observed_candidate_set_revision,status,aggregate_schema,observation_payload,result_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,$12,3,'failed','tect.scope-preservation/1',$13,$14)")
        .bind(tenant).bind(workspace).bind(opportunity).bind(candidate).bind(failed_preservation_id)
        .bind(Uuid::new_v4()).bind(&advice.id.0).bind(disposition.id).bind(disposition.revision)
        .bind(&manifest.source.digest).bind(&manifest.whole_set_digest).bind(&manifest.eligible_set_digest)
        .bind(serde_json::to_value(&observation).unwrap()).bind(serde_json::to_value(&failed_result).unwrap())
        .execute(&pool).await.unwrap();
    let mut failed_caller = caller.clone();
    failed_caller.link_id = Uuid::new_v4();
    failed_caller.request_id = Uuid::new_v4();
    failed_caller.preservation_receipt_id = failed_preservation_id;
    let mut wrong_lineage = caller.clone();
    wrong_lineage.candidate_set_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &failed_caller)
            .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &wrong_lineage)
            .await,
        Err(Error::InputConflict)
    );
    unit.link_scope_advisory_caller(workspace, &caller)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    let before_verifier = unit
        .candidate_advisory_opportunity_detail(workspace, candidate, opportunity)
        .await
        .unwrap()
        .opportunity;
    assert_eq!(before_verifier.caller_receipt_id, Some(caller_request));
    assert_eq!(before_verifier.caller_link_id, Some(caller.link_id));
    assert_eq!(
        before_verifier.preservation_receipt_id,
        Some(preservation_id)
    );
    assert_eq!(before_verifier.verifier_receipt_id, None);
    drop(unit);
    let mut changed_caller = caller.clone();
    changed_caller.preservation_receipt_id = Uuid::new_v4();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.link_scope_advisory_caller(workspace, &changed_caller)
            .await,
        Err(Error::InputConflict)
    );
    drop(unit);
    let mut verifier = ScopeVerifierReceiptInput {
        receipt_id: Uuid::new_v4(),
        request_id: Uuid::new_v4(),
        opportunity_id: opportunity,
        candidate_set_id: candidate,
        caller_link_id: caller.link_id,
        actor_id: actor,
        session_id: session,
        verified_revision: 3,
        verifier_digest: D.into(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        unit.persist_scope_verifier_receipt(workspace, &verifier)
            .await,
        Err(Error::InputConflict)
    );
    verifier.session_id = verifier_session;
    unit.persist_scope_verifier_receipt(workspace, &verifier)
        .await
        .unwrap();
    unit.commit().await.unwrap();
    let skipped = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','skip','1',$7,$8,'no_call','request_skip')")
        .bind(skipped).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(format!("skip-{skipped}")).bind(D).execute(&pool).await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    let first = unit
        .candidate_advisory_audit(workspace, candidate, &audit_query)
        .await
        .unwrap();
    assert_eq!(first.opportunities.len(), 1);
    assert_eq!(first.opportunities[0].id, skipped);
    assert_eq!(first.opportunities[0].guarded_advice_digest, None);
    assert_eq!(first.opportunities[0].disposition_id, None);
    assert_eq!(first.opportunities[0].preservation_receipt_id, None);
    assert_eq!(first.opportunities[0].preservation_status, None);
    assert_eq!(first.opportunities[0].caller_receipt_id, None);
    assert_eq!(first.opportunities[0].caller_link_id, None);
    assert_eq!(first.opportunities[0].verifier_receipt_id, None);
    assert_eq!(first.opportunities[0].selected_save_observation, None);
    let mut after = first.next_after;
    let mut found = false;
    while let Some(cursor) = after {
        let page = unit
            .candidate_advisory_audit(
                workspace,
                candidate,
                &AdvisoryAuditQuery {
                    after: Some(cursor),
                    ..audit_query.clone()
                },
            )
            .await
            .unwrap();
        if page.opportunities[0].id == opportunity {
            assert_eq!(
                page.opportunities[0].caller_receipt_id,
                Some(caller_request)
            );
            assert_eq!(page.opportunities[0].caller_link_id, Some(caller.link_id));
            assert_eq!(
                page.opportunities[0].verifier_receipt_id,
                Some(verifier.receipt_id)
            );
            assert_eq!(
                page.opportunities[0].preservation_receipt_id,
                Some(preservation_id)
            );
            found = true;
        }
        after = page.next_after;
    }
    assert!(
        found,
        "selected opportunity must remain reachable through pagination"
    );
    drop(unit);
    let foreign = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    assert_ne!(foreign.tenant_id, tenant);
    let mut foreign_unit = rw(&store, &foreign.auth, foreign.tenant_id).await;
    let foreign_page = foreign_unit
        .candidate_advisory_audit(workspace, candidate, &audit_query)
        .await
        .unwrap();
    assert!(foreign_page.opportunities.is_empty());
    assert_eq!(foreign_page.aggregate.opportunities, 0);
    drop(foreign_unit);
    let completed_preselection: (bool, i64) = sqlx::query_as(
        "SELECT o.scope_id IS NULL,(SELECT count(*) FROM native_scopes n WHERE n.tenant_id=o.tenant_id AND n.workspace_id=o.workspace_id)::bigint FROM advisory_opportunity o WHERE o.id=$1",
    ).bind(opportunity).fetch_one(&pool).await.unwrap();
    assert_eq!(completed_preselection, (true, 0));

    let original = serde_json::to_value(&manifest).unwrap();
    sqlx::query("UPDATE advisory_scope_manifest SET aggregate_payload=jsonb_set(aggregate_payload,'{baseline_id}','\"tampered\"') WHERE opportunity_id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    assert!(
        unit.scope_advisory_manifest(workspace, opportunity)
            .await
            .is_err()
    );
    drop(unit);
    sqlx::query("UPDATE advisory_scope_manifest SET aggregate_payload=$1 WHERE opportunity_id=$2")
        .bind(original)
        .bind(opportunity)
        .execute(&pool)
        .await
        .unwrap();

    let dispatch_authorization = |opportunity_id, dispatch_id| AdvisoryDispatchAuthorization {
        dispatch_id,
        opportunity_id,
        predecessor_dispatch_id: None,
        attempt_number: 1,
        retry_basis: AdvisoryRetryBasis::Initial,
        provider: "fixture".into(),
        model: "jev".into(),
        configuration_snapshot: serde_json::json!({
            "provider_profile_ref": "fixture",
            "model_configuration": { "model": "jev" }
        }),
        configuration_digest: D.into(),
        material_digest: D.into(),
        payload_digest: D.into(),
        request_payload: b"fixture request".to_vec(),
    };

    // Authorization fences a stale authored source before inserting a dispatch.
    let authorize_stale_opportunity = Uuid::new_v4();
    let authorize_stale_request = format!("request-{authorize_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'3',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(authorize_stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&authorize_stale_request).bind(D).execute(&pool).await.unwrap();
    let authorize_stale_record = ScopeManifestRecord {
        opportunity_id: authorize_stale_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: manifest.clone(),
    };
    let mut prepare = rw(&store, &enrollment.auth, tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(
            workspace,
            &authorize_stale_record,
            &"c".repeat(64),
        )
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let stale_authorization = dispatch_authorization(authorize_stale_opportunity, Uuid::new_v4());
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    assert_eq!(
        crate::advisory::authorize_dispatch_for_test(
            &mut tx,
            tenant,
            workspace,
            1,
            &stale_authorization,
        )
        .await,
        Err(Error::StaleContext)
    );
    tx.commit().await.unwrap();
    let stale_auth_dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(authorize_stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(stale_auth_dispatches, 0);

    // If the frozen source changes after authorization, dispatch-start is the
    // DB linearization point: it records cancellation and a terminal no-call,
    // leaving send_started_at and provider outcome absent.
    let start_stale_opportunity = Uuid::new_v4();
    let start_stale_request = format!("request-{start_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'4',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(start_stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&start_stale_request).bind(D).execute(&pool).await.unwrap();
    let mut current_manifest = manifest.clone();
    current_manifest.source.candidate_set_revision = 4;
    reseal_manifest(&mut current_manifest);
    let start_stale_record = ScopeManifestRecord {
        opportunity_id: start_stale_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: current_manifest,
    };
    let mut prepare = rw(&store, &enrollment.auth, tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(workspace, &start_stale_record, &"d".repeat(64))
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    let authorization = dispatch_authorization(start_stale_opportunity, Uuid::new_v4());
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let authorized =
        crate::advisory::authorize_dispatch_for_test(&mut tx, tenant, workspace, 1, &authorization)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    let dispatch_id = authorized.id;
    sqlx::query("UPDATE scope_candidate_sets SET revision=revision+1 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let started = crate::advisory::start_dispatch_for_test(&mut tx, tenant, workspace, dispatch_id)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    assert!(!started.should_send);
    assert_eq!(started.dispatch.state, AdvisoryDispatchState::Cancelled);
    assert_eq!(
        started.dispatch.send_certainty,
        AdvisorySendCertainty::NotSent
    );
    let dispatch_audit: (String, String, Option<String>, bool, bool, bool) = sqlx::query_as(
        "SELECT state,send_certainty,outcome,send_started_at IS NULL,response_payload IS NULL,sealed_at IS NOT NULL FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(dispatch_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispatch_audit,
        (
            "cancelled".into(),
            "not_sent".into(),
            None,
            true,
            true,
            true
        )
    );
    let opportunity_audit: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(start_stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        opportunity_audit,
        ("no_call".into(), "deterministic_input_invalid".into())
    );

    let config_stale_opportunity = Uuid::new_v4();
    let config_stale_request = format!("request-{config_stale_opportunity}");
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'5',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(config_stale_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(&config_stale_request).bind(D).execute(&pool).await.unwrap();
    let mut config_manifest = manifest.clone();
    config_manifest.source.candidate_set_revision = 5;
    reseal_manifest(&mut config_manifest);
    let config_stale_record = ScopeManifestRecord {
        opportunity_id: config_stale_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: config_manifest,
    };
    let mut prepare = rw(&store, &enrollment.auth, tenant).await;
    prepare
        .prepare_authored_scope_advisory_manifest(workspace, &config_stale_record, &"e".repeat(64))
        .await
        .unwrap();
    prepare.commit().await.unwrap();
    let config_stale_authorization =
        dispatch_authorization(config_stale_opportunity, Uuid::new_v4());
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let authorized = crate::advisory::authorize_dispatch_for_test(
        &mut tx,
        tenant,
        workspace,
        1,
        &config_stale_authorization,
    )
    .await
    .unwrap();
    tx.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history(tenant_id,workspace_id,revision,previous_revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES($1,$2,2,1,'disabled',NULL,NULL,$3,$4)")
        .bind(tenant).bind(workspace).bind(actor).bind(session).execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_workspace_config SET revision=2,mode='disabled',provider_profile_ref=NULL,model_configuration=NULL WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let started =
        crate::advisory::start_dispatch_for_test(&mut tx, tenant, workspace, authorized.id)
            .await
            .unwrap();
    tx.commit().await.unwrap();
    assert!(!started.should_send);
    assert_eq!(started.dispatch.state, AdvisoryDispatchState::Cancelled);
    assert_eq!(
        started.dispatch.send_certainty,
        AdvisorySendCertainty::NotSent
    );
    let config_stale_state: (String, String) = sqlx::query_as(
        "SELECT state,primary_reason FROM advisory_opportunity WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(tenant)
    .bind(workspace)
    .bind(config_stale_opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        config_stale_state,
        ("invalidated".into(), "configuration_changed".into())
    );

    // The selected source-authored material, preservation and caller receipt
    // commit together. A changed draft rolls all of them back.
    set_config(&pool, tenant, workspace, true).await;
    sqlx::query("UPDATE scope_candidate_sets SET status='draft' WHERE id=$1")
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    let selected_opportunity = Uuid::new_v4();
    let selected_dispatch = Uuid::new_v4();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,'5',$5,$6,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','1',$7,$8,'prepared','dispatch_authorized')")
        .bind(selected_opportunity).bind(tenant).bind(workspace).bind(candidate).bind(session).bind(actor)
        .bind(format!("request-{selected_opportunity}")).bind(D).execute(&pool).await.unwrap();
    let ScopeAuthorityOutcome::Authorized(current) =
        authority.observe(&authority_request).await.unwrap()
    else {
        panic!("current source must be authorized");
    };
    let mut authored_for_save = authored_scope_set.clone();
    authored_for_save.expected_candidate_set_revision = 5;
    let selected_manifest = PgScopeAuthoredManifestSupplier::new(
        store.clone(),
        std::sync::Arc::new(PgScopeAuthorityObserver::new(
            store.clone(),
            std::sync::Arc::new(FixtureCandidateGuidance),
        )),
    )
    .supply_authored(&tect_application::ScopeAuthoredManifestRequest {
        tenant_id: tenant,
        observation: current,
        authored_scope_set: authored_for_save.clone(),
    })
    .await
    .unwrap();
    let selected_record = ScopeManifestRecord {
        opportunity_id: selected_opportunity,
        candidate_set_id: candidate,
        config_revision: 1,
        opportunity_material_digest: D.into(),
        manifest: selected_manifest.clone(),
    };
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    unit.prepare_authored_scope_advisory_manifest(workspace, &selected_record, &"f".repeat(64))
        .await
        .unwrap();
    unit.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES($1,$2,$3,$4,1,'fixture','jev','{}',$5,$5,$5,'x','y','sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(selected_dispatch).bind(tenant).bind(workspace).bind(selected_opportunity).bind(D)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(selected_opportunity).execute(&pool).await.unwrap();
    let selected_request =
        ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &selected_manifest).unwrap();
    let selected_answers = NormalizedScopeAdviceAnswers {
        answers: vec![NormalizedScopeAdviceAnswer {
            alternative_id: selected_manifest.baseline_id.clone(),
            choice: ScopeAdviceChoice::Preferred,
            score: ScopeAdviceScoreBand::StrongFit,
            choice_confidence: ConfidenceBasisPoints(9000),
            score_confidence: ConfidenceBasisPoints(8000),
        }],
    };
    let selected_advice = guard_scope_advice(
        &Sha256ScopeDigest,
        selected_opportunity,
        &selected_manifest,
        &selected_request,
        &selected_answers,
    )
    .unwrap();
    let mut unit = rw(&store, &enrollment.auth, tenant).await;
    unit.persist_guarded_scope_advice(
        workspace,
        &GuardedScopeAdviceRecord {
            opportunity_id: selected_opportunity,
            candidate_set_id: candidate,
            dispatch_id: selected_dispatch,
            dispatch_material_digest: D.into(),
            config_revision: 1,
            advice: selected_advice.clone(),
        },
    )
    .await
    .unwrap();
    unit.commit().await.unwrap();
    let selected_disposition = service
        .decide_scope_advisory(
            &context,
            selected_opportunity,
            candidate,
            ScopeDispositionRequest {
                request_id: Uuid::new_v4(),
                advice_id: selected_advice.id.clone(),
                expected_revision: 0,
                action: ScopeDispositionAction::Accept,
                selected_id: Some(selected_manifest.baseline_id.clone()),
                items: vec![ScopeDispositionItem {
                    alternative_id: selected_manifest.baseline_id.clone(),
                    state: ScopeDispositionItemState::Selected,
                }],
                rationale: "Use selected cohesive alternative".into(),
            },
        )
        .await
        .unwrap();
    let mut save = SaveCandidateDraft {
        candidate_set_id: candidate,
        revision: 5,
        snapshot_id: snapshot,
        input_cursor: 2,
        request_id: Uuid::new_v4(),
        draft: authored_for_save.alternatives[0].draft.clone(),
        consumed_knowledge: None,
        selected_advisory: Some(SelectedScopeAdvisory {
            opportunity_id: selected_opportunity,
            disposition_id: selected_disposition.id,
            selected_id: selected_manifest.baseline_id.clone(),
            alternative_key: "baseline".into(),
        }),
    };
    let mut superseding = rw(&store, &enrollment.auth, tenant).await;
    let rejected = superseding
        .cas_scope_advisory_disposition(
            workspace,
            ScopeDispositionRecord {
                opportunity_id: selected_opportunity,
                candidate_set_id: candidate,
                actor_id: actor,
                session_id: session,
                request: ScopeDispositionRequest {
                    request_id: Uuid::new_v4(),
                    advice_id: selected_advice.id.clone(),
                    expected_revision: 1,
                    action: ScopeDispositionAction::RejectAll,
                    selected_id: None,
                    items: vec![ScopeDispositionItem {
                        alternative_id: selected_manifest.baseline_id.clone(),
                        state: ScopeDispositionItemState::NotSelected,
                    }],
                    rationale: "Supersede the selected decision".into(),
                },
            },
        )
        .await
        .unwrap();
    assert_eq!(rejected.revision, 2);
    let (started_tx, started_rx) = tokio::sync::oneshot::channel();
    let pending_store = store.clone();
    let pending_auth = enrollment.auth.clone();
    let pending_save = save.clone();
    let mut pending = tokio::spawn(async move {
        let mut unit = rw(&pending_store, &pending_auth, tenant).await;
        started_tx.send(()).unwrap();
        unit.save_selected_candidate_draft(workspace, actor, session, &pending_save)
            .await
    });
    started_rx.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut pending)
            .await
            .is_err()
    );
    superseding.commit().await.unwrap();
    assert_eq!(
        tokio::time::timeout(std::time::Duration::from_secs(5), pending)
            .await
            .unwrap()
            .unwrap(),
        Err(Error::StaleRevision),
    );
    let rejected_effects: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind(candidate).bind(save.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(rejected_effects, (0, 0));
    let accepted_again = service
        .decide_scope_advisory(
            &context,
            selected_opportunity,
            candidate,
            ScopeDispositionRequest {
                request_id: Uuid::new_v4(),
                advice_id: selected_advice.id.clone(),
                expected_revision: 2,
                action: ScopeDispositionAction::Accept,
                selected_id: Some(selected_manifest.baseline_id.clone()),
                items: vec![ScopeDispositionItem {
                    alternative_id: selected_manifest.baseline_id.clone(),
                    state: ScopeDispositionItemState::Selected,
                }],
                rationale: "Select the frozen alternative after review".into(),
            },
        )
        .await
        .unwrap();
    save.selected_advisory.as_mut().unwrap().disposition_id = accepted_again.id;
    let mut bypass = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        bypass.save_candidate_draft(workspace, &save).await,
        Err(Error::InvalidArguments),
    );
    drop(bypass);
    let bypass_effects: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind(candidate).bind(save.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(bypass_effects, (0, 0, 0));
    let mut altered = save.clone();
    altered.request_id = Uuid::new_v4();
    altered.draft.candidates[0].title = "Altered".into();
    let mut failed = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        failed
            .save_selected_candidate_draft(workspace, actor, session, &altered)
            .await,
        Err(Error::InputConflict)
    );
    drop(failed);
    let rolled_back: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind(candidate).bind(altered.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(rolled_back, (0, 0, 0));
    let mut wrong_id = save.clone();
    wrong_id.request_id = Uuid::new_v4();
    wrong_id.selected_advisory.as_mut().unwrap().selected_id = ScopeAlternativeId(D.into());
    let mut failed = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        failed
            .save_selected_candidate_draft(workspace, actor, session, &wrong_id)
            .await,
        Err(Error::InputConflict)
    );
    drop(failed);
    let mut successful = rw(&store, &enrollment.auth, tenant).await;
    let stored = successful
        .save_selected_candidate_draft(workspace, actor, session, &save)
        .await
        .unwrap();
    assert_eq!(
        stored.draft.as_ref(),
        Some(&selected_manifest.emitted[0].material)
    );
    let (second_started_tx, second_started_rx) = tokio::sync::oneshot::channel();
    let second_store = store.clone();
    let second_auth = enrollment.auth.clone();
    let second_save = save.clone();
    let mut second = tokio::spawn(async move {
        let mut unit = rw(&second_store, &second_auth, tenant).await;
        second_started_tx.send(()).unwrap();
        let replay = unit
            .save_selected_candidate_draft(workspace, actor, session, &second_save)
            .await?;
        unit.commit().await?;
        Ok::<_, Error>(replay)
    });
    second_started_rx.await.unwrap();
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut second)
            .await
            .is_err()
    );
    successful.commit().await.unwrap();
    let concurrent_replay = tokio::time::timeout(std::time::Duration::from_secs(5), second)
        .await
        .unwrap()
        .unwrap()
        .unwrap();
    assert_eq!(concurrent_replay.draft, stored.draft);
    assert_eq!(concurrent_replay.context.candidate_set.revision, 6);
    let committed: (i64, i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=6),\
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2),\
                (SELECT count(*) FROM advisory_scope_preservation_receipt WHERE candidate_set_id=$1 AND request_id=$2 AND status='passed'),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2 AND caller_result_revision=6)"
    ).bind(candidate).bind(save.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(committed, (1, 1, 1, 1));
    let mut audit_unit = rw(&store, &enrollment.auth, tenant).await;
    let selected_audit = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity;
    assert_eq!(selected_audit.disposition_id, Some(accepted_again.id));
    assert_eq!(
        selected_audit.preservation_status.as_deref(),
        Some("passed")
    );
    assert!(selected_audit.preservation_receipt_id.is_some());
    assert_eq!(selected_audit.caller_receipt_id, Some(save.request_id));
    assert!(selected_audit.caller_link_id.is_some());
    assert_eq!(selected_audit.verifier_receipt_id, None);
    assert_eq!(selected_audit.selected_save_observation, None);
    drop(audit_unit);
    let observe_request = SelectedSaveObservationRequest {
        request_id: Uuid::new_v4(),
        opportunity_id: selected_opportunity,
        candidate_set_id: candidate,
        caller_link_id: selected_audit.caller_link_id.unwrap(),
        caller_receipt_request_id: save.request_id,
        target_revision: 6,
        session_id: session,
    };
    let mut observation_unit = rw(&store, &enrollment.auth, tenant).await;
    let observed = observation_unit
        .observe_selected_scope_save(workspace, &observe_request)
        .await
        .unwrap();
    assert_eq!(observed.status, SelectedSaveObservationStatus::Passed);
    assert!(observed.reason_codes.is_empty());
    assert_eq!(observed.qualification, "unresolved");
    observation_unit.commit().await.unwrap();
    let mut audit_unit = rw(&store, &enrollment.auth, tenant).await;
    let passed_audit = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity;
    let public_pass = passed_audit.selected_save_observation.unwrap();
    assert_eq!(public_pass.id, observed.id);
    assert_eq!(public_pass.status, SelectedSaveObservationStatus::Passed);
    assert_eq!(public_pass.target_revision, 6);
    assert!(public_pass.reason_codes.is_empty());
    assert_eq!(public_pass.evidence_digest, observed.evidence_digest);
    assert_eq!(public_pass.qualification, "unresolved");
    assert!(!public_pass.establishes_independent_approval);
    assert!(!public_pass.establishes_current_acceptance);
    assert_eq!(passed_audit.verifier_receipt_id, None);
    drop(audit_unit);
    let foreign = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let mut foreign_unit = rw(&store, &foreign.auth, foreign.tenant_id).await;
    assert!(
        foreign_unit
            .candidate_advisory_audit(workspace, candidate, &audit_query)
            .await
            .unwrap()
            .opportunities
            .is_empty()
    );
    assert!(matches!(
        foreign_unit
            .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
            .await,
        Err(Error::NotFound)
    ));
    drop(foreign_unit);
    let mut replay_unit = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        replay_unit
            .observe_selected_scope_save(workspace, &observe_request)
            .await
            .unwrap(),
        observed
    );
    let mut changed_target = observe_request.clone();
    changed_target.caller_link_id = Uuid::new_v4();
    assert_eq!(
        replay_unit
            .observe_selected_scope_save(workspace, &changed_target)
            .await,
        Err(Error::InputConflict)
    );
    let mut changed_identity = observe_request.clone();
    changed_identity.session_id = verifier_session;
    assert_eq!(
        replay_unit
            .observe_selected_scope_save(workspace, &changed_identity)
            .await,
        Err(Error::InputConflict)
    );
    drop(replay_unit);
    let mut concurrent_request = observe_request.clone();
    concurrent_request.request_id = Uuid::new_v4();
    let mut first_observer = rw(&store, &enrollment.auth, tenant).await;
    let first_observation = first_observer
        .observe_selected_scope_save(workspace, &concurrent_request)
        .await
        .unwrap();
    let second_store = store.clone();
    let second_auth = enrollment.auth.clone();
    let second_request = concurrent_request.clone();
    let mut second_observer = tokio::spawn(async move {
        let mut unit = rw(&second_store, &second_auth, tenant).await;
        let observation = unit
            .observe_selected_scope_save(workspace, &second_request)
            .await?;
        unit.commit().await?;
        Ok::<_, Error>(observation)
    });
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(100), &mut second_observer)
            .await
            .is_err()
    );
    first_observer.commit().await.unwrap();
    let second_observation = second_observer.await.unwrap().unwrap();
    assert_eq!(second_observation, first_observation);
    let effects_before: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1)",
    )
    .bind(candidate)
    .fetch_one(&pool)
    .await
    .unwrap();
    let original_draft: serde_json::Value = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=6",
    ).bind(tenant).bind(workspace).bind(candidate).fetch_one(&pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_drafts SET payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=6")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let mut tampered = observe_request.clone();
    tampered.request_id = Uuid::new_v4();
    let mut observation_unit = rw(&store, &enrollment.auth, tenant).await;
    let tamper_result = observation_unit
        .observe_selected_scope_save(workspace, &tampered)
        .await
        .unwrap();
    assert_eq!(tamper_result.status, SelectedSaveObservationStatus::Failed);
    assert!(
        tamper_result
            .reason_codes
            .contains(&"saved_material_missing_or_mismatched".into())
    );
    observation_unit.commit().await.unwrap();
    let mut audit_unit = rw(&store, &enrollment.auth, tenant).await;
    let latest = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity
        .selected_save_observation
        .unwrap();
    assert_eq!(latest.id, tamper_result.id);
    assert_eq!(latest.status, SelectedSaveObservationStatus::Failed);
    assert_eq!(latest.target_revision, 6);
    assert!(
        latest
            .reason_codes
            .contains(&"saved_material_missing_or_mismatched".into())
    );
    assert_eq!(latest.qualification, "unresolved");
    let mut cursor = None;
    let mut selected_rows = 0;
    loop {
        let page = audit_unit
            .candidate_advisory_audit(
                workspace,
                candidate,
                &AdvisoryAuditQuery {
                    after: cursor,
                    ..audit_query.clone()
                },
            )
            .await
            .unwrap();
        assert!(page.opportunities.len() <= 1);
        for opportunity in page.opportunities {
            if opportunity.id == selected_opportunity {
                selected_rows += 1;
                assert_eq!(
                    opportunity.selected_save_observation.as_ref(),
                    Some(&latest)
                );
            }
        }
        cursor = page.next_after;
        if cursor.is_none() {
            break;
        }
    }
    assert_eq!(selected_rows, 1);
    drop(audit_unit);
    let mut stale = observe_request.clone();
    stale.request_id = Uuid::new_v4();
    stale.target_revision = 5;
    let mut observation_unit = rw(&store, &enrollment.auth, tenant).await;
    let stale_result = observation_unit
        .observe_selected_scope_save(workspace, &stale)
        .await
        .unwrap();
    assert_eq!(stale_result.status, SelectedSaveObservationStatus::Failed);
    assert!(
        stale_result
            .reason_codes
            .contains(&"candidate_revision_stale".into())
    );
    observation_unit.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_drafts SET payload=$1 WHERE tenant_id=$2 AND workspace_id=$3 AND candidate_set_id=$4 AND set_revision=6")
        .bind(original_draft).bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let mut missing = observe_request.clone();
    missing.request_id = Uuid::new_v4();
    missing.caller_link_id = Uuid::new_v4();
    let mut observation_unit = rw(&store, &enrollment.auth, tenant).await;
    let missing_result = observation_unit
        .observe_selected_scope_save(workspace, &missing)
        .await
        .unwrap();
    assert_eq!(missing_result.status, SelectedSaveObservationStatus::Failed);
    assert!(
        missing_result
            .reason_codes
            .contains(&"caller_link_missing_or_mismatched".into())
    );
    observation_unit.commit().await.unwrap();
    let effects_after: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1), \
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1)",
    )
    .bind(candidate)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(effects_after, effects_before);
    let mut changed_payload = save.clone();
    changed_payload.draft.candidates[0].title = "Conflicting replay".into();
    let mut conflict = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        conflict
            .save_selected_candidate_draft(workspace, actor, session, &changed_payload)
            .await,
        Err(Error::InputConflict),
    );
    drop(conflict);
    let mut replay_unit = rw(&store, &enrollment.auth, tenant).await;
    let replayed = replay_unit
        .save_selected_candidate_draft(workspace, actor, session, &save)
        .await
        .unwrap();
    assert_eq!(replayed.draft, stored.draft);
    assert_eq!(
        replayed.context.candidate_set.revision,
        stored.context.candidate_set.revision
    );
    replay_unit.commit().await.unwrap();
    let mut wrong_session = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        wrong_session
            .save_selected_candidate_draft(workspace, actor, verifier_session, &save)
            .await,
        Err(Error::InputConflict),
    );
    drop(wrong_session);
    let same_session_replay = service
        .save_candidate_draft(
            &context,
            &save,
            &FixtureCandidateGuidance,
            &FixtureCandidateOutputGuard,
        )
        .await
        .unwrap();
    assert_eq!(same_session_replay.draft, stored.draft);
    let other_context = tect_domain::RequestContext {
        native_session_id: verifier_session.to_string(),
        ..context.clone()
    };
    assert_eq!(
        service
            .save_candidate_draft(
                &other_context,
                &save,
                &FixtureCandidateGuidance,
                &FixtureCandidateOutputGuard,
            )
            .await,
        Err(Error::InputConflict),
    );
    let mut stale_save = save.clone();
    stale_save.request_id = Uuid::new_v4();
    let mut failed = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        failed
            .save_selected_candidate_draft(workspace, actor, session, &stale_save)
            .await,
        Err(Error::StaleRevision)
    );
    drop(failed);
    let stale_effects: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=7),\
                (SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2)"
    ).bind(candidate).bind(stale_save.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(stale_effects, (0, 0));

    // A fresh verifier principal, not a second owner session, qualifies the
    // independent observation. Enrollment itself creates no session.
    let verifier = admin::prepare_verifier_enrollment(&pool, tenant, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let independent_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(independent_session).bind(tenant).bind(verifier.auth.host_id).bind(workspace)
        .bind(independent_session.to_string()).execute(&pool).await.unwrap();
    let mut qualified_request = observe_request.clone();
    qualified_request.request_id = Uuid::new_v4();
    qualified_request.session_id = independent_session;
    let mut owner_attempt = rw(&store, &enrollment.auth, tenant).await;
    assert_eq!(
        owner_attempt
            .independently_observe_selected_scope_save(workspace, &qualified_request)
            .await,
        Err(Error::Forbidden)
    );
    drop(owner_attempt);
    let no_owner_qualified: i64 = sqlx::query_scalar("SELECT count(*) FROM advisory_scope_selected_save_observation WHERE tenant_id=$1 AND workspace_id=$2 AND request_id=$3")
        .bind(tenant).bind(workspace).bind(qualified_request.request_id).fetch_one(&pool).await.unwrap();
    assert_eq!(no_owner_qualified, 0);
    let mut verifier_attempt = rw(&store, &verifier.auth, tenant).await;
    let mut forged_session = qualified_request.clone();
    forged_session.session_id = session;
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &forged_session)
            .await,
        Err(Error::InvalidArguments)
    );
    let mut forged_caller = qualified_request.clone();
    forged_caller.caller_link_id = Uuid::new_v4();
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &forged_caller)
            .await,
        Err(Error::Forbidden)
    );
    let mut forged_receipt = qualified_request.clone();
    forged_receipt.caller_receipt_request_id = Uuid::new_v4();
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &forged_receipt)
            .await,
        Err(Error::Forbidden)
    );
    let qualified_pass = verifier_attempt
        .independently_observe_selected_scope_save(workspace, &qualified_request)
        .await
        .unwrap();
    assert_eq!(qualified_pass.status, SelectedSaveObservationStatus::Passed);
    assert_eq!(qualified_pass.actor_id, verifier.principal_id);
    assert_eq!(qualified_pass.qualification, "independently_observed");
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &qualified_request)
            .await
            .unwrap(),
        qualified_pass
    );
    let second_independent_session = Uuid::new_v4();
    sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
        .bind(second_independent_session).bind(tenant).bind(verifier.auth.host_id).bind(workspace)
        .bind(second_independent_session.to_string()).execute(&pool).await.unwrap();
    let mut changed_session = qualified_request.clone();
    changed_session.session_id = second_independent_session;
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &changed_session)
            .await,
        Err(Error::InputConflict)
    );
    assert_eq!(
        verifier_attempt
            .observe_selected_scope_save(workspace, &qualified_request)
            .await,
        Err(Error::InputConflict)
    );
    let mut changed_qualified = qualified_request.clone();
    changed_qualified.target_revision = 5;
    assert_eq!(
        verifier_attempt
            .independently_observe_selected_scope_save(workspace, &changed_qualified)
            .await,
        Err(Error::Forbidden)
    );
    verifier_attempt.commit().await.unwrap();
    let mut audit_unit = rw(&store, &enrollment.auth, tenant).await;
    let public_qualified = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity
        .selected_save_observation
        .unwrap();
    assert_eq!(public_qualified.id, qualified_pass.id);
    assert_eq!(public_qualified.qualification, "independently_observed");
    assert!(!public_qualified.establishes_independent_approval);
    assert!(!public_qualified.establishes_current_acceptance);
    drop(audit_unit);

    sqlx::query("UPDATE scope_candidate_drafts SET payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=6")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let mut failed_request = qualified_request.clone();
    failed_request.request_id = Uuid::new_v4();
    let mut verifier_attempt = rw(&store, &verifier.auth, tenant).await;
    let qualified_fail = verifier_attempt
        .independently_observe_selected_scope_save(workspace, &failed_request)
        .await
        .unwrap();
    assert_eq!(qualified_fail.status, SelectedSaveObservationStatus::Failed);
    assert_eq!(qualified_fail.qualification, "independently_observed");
    assert!(
        qualified_fail
            .reason_codes
            .contains(&"saved_material_missing_or_mismatched".into())
    );
    verifier_attempt.commit().await.unwrap();
    let mut audit_unit = rw(&store, &enrollment.auth, tenant).await;
    let public_failure = audit_unit
        .candidate_advisory_opportunity_detail(workspace, candidate, selected_opportunity)
        .await
        .unwrap()
        .opportunity
        .selected_save_observation
        .unwrap();
    assert_eq!(public_failure.id, qualified_fail.id);
    assert!(!public_failure.establishes_independent_approval);
    assert!(!public_failure.establishes_current_acceptance);
    drop(audit_unit);
    sqlx::query("UPDATE scope_candidate_sets SET revision=7 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(tenant).bind(workspace).bind(candidate).execute(&pool).await.unwrap();
    let mut stale_qualified_request = qualified_request.clone();
    stale_qualified_request.request_id = Uuid::new_v4();
    let mut verifier_attempt = rw(&store, &verifier.auth, tenant).await;
    let stale_qualified = verifier_attempt
        .independently_observe_selected_scope_save(workspace, &stale_qualified_request)
        .await
        .unwrap();
    assert_eq!(
        stale_qualified.status,
        SelectedSaveObservationStatus::Failed
    );
    assert_eq!(stale_qualified.qualification, "independently_observed");
    assert!(
        stale_qualified
            .reason_codes
            .contains(&"candidate_revision_stale".into())
    );
    verifier_attempt.commit().await.unwrap();
}

#[test]
fn authored_request_digest_requires_lowercase_sha256_hex() {
    assert!(super::valid_authored_request_digest(&"a".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"A".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"g".repeat(64)));
    assert!(!super::valid_authored_request_digest(&"a".repeat(63)));
}

#[test]
fn prepared_scope_disposition_uses_only_pre_dispatch_terminal_reasons() {
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::DeterministicInputInvalid),
        Ok("no_call")
    );
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::ConfigurationChanged),
        Ok("invalidated")
    );
    assert_eq!(
        super::prepared_scope_disposition_state(AdvisoryReason::ProviderUnconfigured),
        Err(Error::InvalidArguments)
    );
    assert!(super::valid_scope_source_digest(&"a".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"A".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"z".repeat(64)));
    assert!(!super::valid_scope_source_digest(&"a".repeat(63)));
}

#[tokio::test]
#[ignore = "requires disposable PG18 and TECT_TEST_ADMIN_URL/TECT_TEST_RUNTIME_URL/TECT_TEST_RUNTIME_ROLE"]
async fn published_source_snapshots_are_permanently_frozen_and_refresh_advances() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = sqlx::PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let runtime = sqlx::PgPool::connect(&runtime_url).await.unwrap();
    let enrollment = admin::enroll_host(&pool, None, vec![]).await.unwrap();
    let tenant = enrollment.tenant_id;
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let candidate = Uuid::new_v4();
    let first = Uuid::new_v4();
    let second = Uuid::new_v4();
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(tenant)
        .bind(format!("source-freeze-{workspace}"))
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,name,intent,basis,boundaries,constraints,success,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'open',1,'p','i','b','finite','c','s','ready',0,1,4096)")
        .bind(program).bind(tenant).bind(workspace).execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_sets(id,tenant_id,workspace_id,program_id,origin_request_id,origin_input,origin_payload,status,boundary,max_input_bytes) VALUES($1,$2,$3,$4,$5,'input','{}','ready','finite',4096)")
        .bind(candidate).bind(tenant).bind(workspace).bind(program).bind(Uuid::new_v4())
        .execute(&pool).await.unwrap();
    sqlx::query("INSERT INTO scope_candidate_contents(tenant_id,workspace_id,digest,body) VALUES($1,$2,$3,'body')")
        .bind(tenant).bind(workspace).bind(D).execute(&pool).await.unwrap();
    for (id, sequence) in [(first, 1_i64), (second, 2)] {
        sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,$5,1,1,1,$6,'{}',$6,'m','1',$6,'body','[]','1',$6,'[]')")
            .bind(id).bind(tenant).bind(workspace).bind(candidate).bind(sequence).bind(D)
            .execute(&pool).await.unwrap();
    }

    // Exercise the trigger through the actual runtime role and tenant policy.
    let mut runtime_tx = runtime.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut *runtime_tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','intent',$5,'intent')")
        .bind(tenant).bind(workspace).bind(candidate).bind(first).bind(D)
        .execute(&mut *runtime_tx).await.unwrap();
    runtime_tx.commit().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(first)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    let late = sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','basis',$5,'basis')")
        .bind(tenant).bind(workspace).bind(candidate).bind(first).bind(D)
        .execute(&pool).await;
    assert!(
        late.is_err(),
        "published snapshot accepted a late reference"
    );

    // A pending refresh can accumulate refs, then publish at a greater sequence.
    sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','basis',$5,'basis')")
        .bind(tenant).bind(workspace).bind(candidate).bind(second).bind(D)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(second)
        .bind(candidate)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query(
        "UPDATE scope_candidate_sets SET current_snapshot_id=$1,revision=revision+1 WHERE id=$2",
    )
    .bind(second)
    .bind(candidate)
    .execute(&pool)
    .await
    .unwrap();
    assert!(
        sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
            .bind(first)
            .bind(candidate)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(
        sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=NULL WHERE id=$1")
            .bind(candidate)
            .execute(&pool)
            .await
            .is_err()
    );
    assert!(sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','constraints',$5,'constraints')")
        .bind(tenant).bind(workspace).bind(candidate).bind(second).bind(D)
        .execute(&pool).await.is_err());

    // Publication holds the same set-row lock as manifest preparation.
    let third = Uuid::new_v4();
    sqlx::query("INSERT INTO scope_candidate_snapshots(id,tenant_id,workspace_id,candidate_set_id,sequence,program_revision,program_latest_input,planning_latest_input,program_body_digest,selected_worktree_ids,selected_sources_digest,method_id,method_revision,method_digest,method_body,method_origin_refs,registry_revision,registry_digest,rules) VALUES($1,$2,$3,$4,3,1,1,1,$5,'{}',$5,'m','1',$5,'body','[]','1',$5,'[]')")
        .bind(third).bind(tenant).bind(workspace).bind(candidate).bind(D)
        .execute(&pool).await.unwrap();
    let mut publisher = pool.begin().await.unwrap();
    sqlx::query("UPDATE scope_candidate_sets SET current_snapshot_id=$1 WHERE id=$2")
        .bind(third)
        .bind(candidate)
        .execute(&mut *publisher)
        .await
        .unwrap();
    let insert_pool = pool.clone();
    let insert = tokio::spawn(async move {
        sqlx::query("INSERT INTO scope_candidate_source_refs(tenant_id,workspace_id,candidate_set_id,snapshot_id,kind,program_field,body_digest,label) VALUES($1,$2,$3,$4,'program_field','boundaries',$5,'boundaries')")
            .bind(tenant).bind(workspace).bind(candidate).bind(third).bind(D)
            .execute(&insert_pool).await
    });
    tokio::pin!(insert);
    assert!(
        tokio::time::timeout(std::time::Duration::from_millis(150), &mut insert)
            .await
            .is_err(),
        "insert did not wait for publication lock"
    );
    publisher.commit().await.unwrap();
    assert!(
        insert.await.unwrap().is_err(),
        "concurrent append escaped publication freeze"
    );
}
