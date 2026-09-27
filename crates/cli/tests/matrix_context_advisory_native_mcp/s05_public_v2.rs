//! S05 public MCP proof against exact V2 Matrix Work; synthetic adviser only.
use super::*;
use tect_application::{
    ModelRouteCatalogueProvider, ModelRouteHostCapabilitiesProvider, ModelRoutePreparedAttempt,
    ModelRouteProviderObservation, ModelRouteRankingProvider, ModelRouteSendPermit,
    PreparedModelRouteRecommendation,
};
use tect_domain::{
    Error, MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    MODEL_ROUTE_RANKING_WIRE_SCHEMA, ModelRoute, ModelRouteCatalogue, ModelRouteFact,
    ModelRouteFactProvenance, ModelRouteHostCapabilities, ModelRouteRankingWireRequest,
};

struct Routes;
impl ModelRouteCatalogueProvider for Routes {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: ["route-a", "route-b", "route-ineligible"]
                .into_iter()
                .map(|id| ModelRoute {
                    id: id.into(),
                    provider: "synthetic.invalid".into(),
                    model: format!("candidate-{id}"),
                    effort: "medium".into(),
                    enabled: true,
                    allowed_matrix_choice_ids: vec!["b".into()],
                    allowed_roles: vec!["agent".into()],
                    allowed_tools: vec!["code".into()],
                    allowed_data_classes: vec!["internal".into()],
                    required_host_capabilities: vec![
                        if id == "route-ineligible" {
                            "unavailable-capability"
                        } else {
                            "model-api"
                        }
                        .into(),
                    ],
                    minimum_budget_units: 10,
                    minimum_latency_ms: 50,
                })
                .collect(),
        }))
    }
}

struct HostCapabilities;
impl ModelRouteHostCapabilitiesProvider for HostCapabilities {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>> {
        ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["model-api".into()],
        }
        .fact()
    }
}

struct FakeJev {
    calls: Arc<AtomicUsize>,
}
#[async_trait]
impl ModelRouteRankingProvider for FakeJev {
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        ModelRoutePreparedAttempt::new(ModelRouteRankingWireRequest::new(
            saved.workspace_id,
            &saved.request_key,
            &saved.work,
            saved.catalogue.as_ref().ok_or(Error::InputConflict)?,
            saved.eligible.as_ref().ok_or(Error::InputConflict)?,
            "synthetic-jev",
        )?)
    }

    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        _permit: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        self.calls.fetch_add(1, Ordering::SeqCst);
        assert_eq!(
            attempted.request.binding.eligible_route_ids,
            vec!["route-a", "route-b"]
        );
        Ok(serde_json::to_vec(&json!({
            "schema":MODEL_ROUTE_RANKING_WIRE_SCHEMA,
            "binding_digest":attempted.request.binding_digest,
            "adviser_model":"synthetic-jev",
            "outcome":{"kind":"ranked","route_ids":["route-b","route-a"]}
        }))
        .unwrap())
    }

    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        let raw = self.attempt_prepared(attempted, permit).await?;
        Ok(ModelRouteProviderObservation {
            raw,
            response_complete: Some(true),
            original_transport_context: None,
            http_status: Some(200),
            input_tokens: Some(4),
            output_tokens: Some(3),
            elapsed_monotonic_ms: Some(1),
        })
    }
}

fn assert_caller_fact<T: PartialEq + std::fmt::Debug>(
    fact: &ModelRouteFact<T>,
    expected: T,
    work: &Value,
    field: &str,
) {
    let ModelRouteFact::Known { value, provenance } = fact else {
        panic!("{field} must be a known caller fact");
    };
    assert_eq!(*value, expected);
    let ModelRouteFactProvenance::Caller {
        source_ref,
        work_node_id,
        work_node_revision,
    } = provenance
    else {
        panic!("{field} must retain caller provenance");
    };
    assert!(source_ref.ends_with(&format!("/model_route_facts/{field}")));
    assert_eq!(*work_node_id, id(&work["id"]));
    assert_eq!(*work_node_revision, work["revision"].as_i64().unwrap());
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only exact owned PostgreSQL 18.6 fixture at migration 110; synthetic adviser"]
async fn public_s05_v2_work_caller_assertions_rank_eligible_ids_only() {
    let (pool, runtime_url) = exact_fixture().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("s05-v2.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("s05-v2-{}", Uuid::new_v4());
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let (workspace, owner_keys) = signed_fixture_budget(&store, &enrolled, &workspace_key).await;
    let matrix_calls = Arc::new(AtomicUsize::new(0));
    let ranking_calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.with_budget_owner_keys(owner_keys)),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(matrix_calls.clone())),
            Arc::new(Budget(Arc::new(AtomicUsize::new(0)))),
        )
        .with_model_route_catalogue_provider(Arc::new(Routes))
        .with_model_route_host_capabilities_provider(Arc::new(HostCapabilities))
        .with_model_route_ranking_provider(Arc::new(FakeJev {
            calls: ranking_calls.clone(),
        })),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let owner_file = root.join("owner.json");
    host_file(&owner_file, &enrolled.auth);
    let mut owner = Mcp::start(
        &socket,
        &owner_file,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
    assert_eq!(
        id(&owner.call("open_workspace", json!({})).await["workspace"]["id"]),
        workspace
    );
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    propose_confirm(&mut owner, program, 0, declarations("demo")).await;
    let effective = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record(&mut owner, task, Uuid::new_v4(), Some(locator(program))).await;
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional","provider_profile_ref":{"id":PROFILE},
            "model_configuration":{"model":MODEL}
        }),
    )
    .await;
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_file = root.join("verifier.json");
    host_file(&verifier_file, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_file,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let matrix: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(effective).unwrap();
    let facts = required_matrix_operating_facts(&context, &matrix).unwrap();
    assert!(!facts.is_empty());
    let evidence: Vec<_> = facts.iter().map(|fact| json!({
        "fact_path":fact.path,"evidence_ref":format!("urn:fixture:s05:operating:{}",fact.path)
    })).collect();
    let verified = route(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({
            "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],
            "evidence":evidence
        }),
    )
    .await;
    assert_eq!(verified["schema"], "tect.context-matrix-verification/1");
    assert_eq!(
        verified["frozen_snapshot_id"],
        recorded["requirements_snapshot_id"]
    );
    let advice_key = format!("s05-v2-matrix-{}", Uuid::new_v4());
    let advised = advice(&mut owner, task, &advice_key).await;
    assert_eq!(advised["state"], "advised", "{advised}");
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({"task_id":task,"request_key":advice_key}),
    )
    .await;
    let chosen = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        json!({
            "request_id":Uuid::new_v4(),"task_id":task,"expected_task_revision":1,
            "expected_input_digest":recorded["input_digest"],
            "expected_choice_set_digest":recorded["choice_set_digest"],
            "opportunity_id":advised["opportunity_id"],"basis":"after_advice",
            "advice_id":current["current_advice"]["advice_id"],
            "advice_digest":current["current_advice"]["advice_digest"],
            "decision":{"outcome":"selected","selected_choice_id":"b"}
        }),
    )
    .await;
    let selection = json!({
        "task_id":task,"task_revision":1,"disposition_id":chosen["disposition_id"],
        "selected_choice_id":"b","expected_input_digest":recorded["input_digest"],
        "expected_choice_set_digest":recorded["choice_set_digest"],
        "expected_verification_digest":verified["verification_digest"],
        "mapped_draft_node_indices":[0]
    });
    let opened = route(
        &mut owner,
        "command",
        "scope.open",
        json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]
        }),
    )
    .await;
    let mut save = save_request(&opened["created"]["planning"], selection);
    save["draft"]["nodes"][0]["model_route_facts"] = json!({
        "role":"agent","tool":"code","data_class":"internal",
        "remaining_budget_units":20,"available_latency_ms":100
    });
    let caller_request = save["request_id"].clone();
    let saved = route(&mut owner, "command", "slice.candidates.save", save).await;
    let set = saved["candidate_set"]["id"].clone();
    let work = saved["draft"]["nodes"][0].clone();
    let effect = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({"candidate_set_id":set,"caller_request_id":caller_request}),
    )
    .await;
    route(
        &mut independent,
        "command",
        "engineering.matrix.planning_effect.verify",
        json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller_request,
            "expected_result_revision":effect["material"]["result_revision"],
            "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
            "summary":"Synthetic Work maps exact selected Matrix choice b."
        }),
    )
    .await;
    let key = format!("s05-v2-route-{}", Uuid::new_v4());
    let prepared = route(
        &mut owner,
        "command",
        "model.route.prepare",
        json!({
            "disposition_id":chosen["disposition_id"],"expected_task_id":task,
            "expected_task_revision":1,"expected_candidate_set_id":set,
            "expected_caller_request_id":caller_request,"expected_mapped_work_node_id":work["id"],
            "expected_mapped_work_node_revision":work["revision"],
            "request_key":key,"requested_route_id":"route-a"
        }),
    )
    .await;
    assert_eq!(prepared["preparation"], "Prepared", "{prepared}");
    assert_eq!(
        prepared["work"]["context_authority"]["authority_schema"],
        "tect.matrix-requirements/1"
    );
    assert_eq!(
        prepared["work"]["context_authority"]["frozen_snapshot_id"],
        recorded["requirements_snapshot_id"]
    );
    assert_eq!(
        prepared["work"]["context_authority"]["operating_verification_digest"],
        verified["verification_digest"]
    );
    let route_work: tect_domain::ModelRouteWorkContext =
        serde_json::from_value(prepared["work"].clone()).unwrap();
    assert_caller_fact(&route_work.role, "agent".to_string(), &work, "role");
    assert_caller_fact(&route_work.tool, "code".to_string(), &work, "tool");
    assert_caller_fact(
        &route_work.data_class,
        "internal".to_string(),
        &work,
        "data_class",
    );
    assert_caller_fact(
        &route_work.remaining_budget_units,
        20,
        &work,
        "remaining_budget_units",
    );
    assert_caller_fact(
        &route_work.available_latency_ms,
        100,
        &work,
        "available_latency_ms",
    );
    assert!(matches!(
        route_work.host_capabilities,
        ModelRouteFact::Known {
            provenance: ModelRouteFactProvenance::Host { .. },
            ..
        }
    ));
    assert_eq!(
        prepared["eligible"]["route_ids"],
        json!(["route-a", "route-b"])
    );
    assert_eq!(prepared["routes"]["requested_route_id"], "route-a");
    assert!(prepared["routes"]["observed_actual"].is_null());
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 0);
    let run = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(
        run["decision"]["outcome"]["Recommended"]["route_id"],
        "route-b"
    );
    assert_eq!(run["decision"]["routes"]["requested_route_id"], "route-a");
    assert_eq!(run["decision"]["routes"]["recommended_route_id"], "route-b");
    assert!(run["decision"]["routes"]["observed_actual"].is_null());
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 1);
    let disposition = route(&mut owner, "command", "model.route.disposition", json!({
        "disposition_id":Uuid::new_v4(),"decision_id":run["decision"]["id"],
        "action":"accept","rationale":"Synthetic caller accepts advisory rank without executing a model"
    })).await;
    assert_eq!(disposition["action"], "Accept");
    let read = route(
        &mut owner,
        "query",
        "model.route.get",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(read["disposition"]["id"], disposition["id"]);
    assert!(read["decision"]["routes"]["observed_actual"].is_null());

    // The selected save and first decision belong to the original session.
    // Capture another decision before revoking it, then let a fresh authorized
    // owner session create that decision's first disposition.
    let later_key = format!("s05-v2-later-{}", Uuid::new_v4());
    let later_prepared = route(
        &mut owner,
        "command",
        "model.route.prepare",
        json!({
            "disposition_id":chosen["disposition_id"],"expected_task_id":task,
            "expected_task_revision":1,"expected_candidate_set_id":set,
            "expected_caller_request_id":caller_request,"expected_mapped_work_node_id":work["id"],
            "expected_mapped_work_node_revision":work["revision"],
            "request_key":later_key,"requested_route_id":"route-a"
        }),
    )
    .await;
    assert_eq!(later_prepared["preparation"], "Prepared");
    let later_run = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":later_key}),
    )
    .await;
    assert_eq!(
        later_run["decision"]["outcome"]["Recommended"]["route_id"],
        "route-b"
    );
    assert!(later_run["decision"]["routes"]["observed_actual"].is_null());
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 2);
    let source_session: Uuid = sqlx::query_scalar(
        "SELECT caller_session_id FROM matrix_planning_selection_links \
         WHERE workspace_id=$1 AND candidate_set_id=$2 AND caller_request_id=$3",
    )
    .bind(workspace)
    .bind(id(&set))
    .bind(id(&caller_request))
    .fetch_one(&pool)
    .await
    .unwrap();
    admin::revoke_session(&pool, source_session).await.unwrap();
    let mut fresh = Mcp::start(
        &socket,
        &owner_file,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    assert_eq!(
        id(&fresh.call("open_workspace", json!({})).await["workspace"]["id"]),
        workspace
    );
    let later_disposition = route(
        &mut fresh,
        "command",
        "model.route.disposition",
        json!({
            "disposition_id":Uuid::new_v4(),"decision_id":later_run["decision"]["id"],
            "action":"accept","rationale":"Fresh owner session accepts historical advisory decision"
        }),
    )
    .await;
    assert_eq!(later_disposition["action"], "Accept");
    let persisted_later: (Uuid, Uuid, String) = sqlx::query_as(
        "SELECT id,decision_id,action FROM model_route_dispositions \
         WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(id(&later_disposition["id"]))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted_later,
        (
            id(&later_disposition["id"]),
            id(&later_run["decision"]["id"]),
            "accept".into()
        )
    );
    let persisted: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1),
         (SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1),
         (SELECT count(*) FROM native_slices WHERE workspace_id=$1)",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(persisted, (2, 2, 0));
    assert_eq!(matrix_calls.load(Ordering::SeqCst), 1);
    independent.finish().await;
    owner.finish().await;
    fresh.finish().await;
    server.abort();
    let _ = server.await;
}
