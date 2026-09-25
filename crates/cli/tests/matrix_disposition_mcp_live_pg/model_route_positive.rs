//! Positive public MCP proof with a local fake adviser and a disposable PG18 fixture.
use super::*;
use crate::support::{ready_source_candidate, repository};
use tect_application::{
    ModelRouteCatalogueProvider, ModelRouteHostCapabilitiesProvider, ModelRoutePreparedAttempt,
    ModelRouteRankingProvider, ModelRouteSendPermit, PreparedModelRouteRecommendation,
};
use tect_domain::{
    Error, MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    MODEL_ROUTE_RANKING_WIRE_SCHEMA, ModelRoute, ModelRouteCatalogue, ModelRouteFact,
    ModelRouteHostCapabilities, ModelRouteRankingWireRequest,
};

struct Catalogue;
impl ModelRouteCatalogueProvider for Catalogue {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: ["route-a", "route-b"]
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
                    required_host_capabilities: vec!["model-api".into()],
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

struct FakeAdviser {
    pool: PgPool,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl ModelRouteRankingProvider for FakeAdviser {
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
            "fake-jev-adviser",
        )?)
    }

    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        // The provider observes a committed, one-use send fence before it is called.
        let committed: (String, Vec<u8>, String) = sqlx::query_as(
            "SELECT state,request_payload,request_sha256 FROM model_route_advisory_attempts \
             WHERE workspace_id=$1 AND id=$2 AND preparation_request_key=$3",
        )
        .bind(permit.workspace_id)
        .bind(permit.attempt_id)
        .bind(&permit.preparation_request_key)
        .fetch_one(&self.pool)
        .await
        .map_err(|_| Error::StorageUnavailable)?;
        assert_eq!(committed.0, "send_unknown");
        assert_eq!(committed.1, attempted.request_bytes);
        assert_eq!(committed.2, attempted.request_sha256);
        self.calls.fetch_add(1, Ordering::SeqCst);
        Ok(response(&attempted.request))
    }
}

fn response(request: &ModelRouteRankingWireRequest) -> Vec<u8> {
    serde_json::to_vec(&json!({
        "schema": MODEL_ROUTE_RANKING_WIRE_SCHEMA,
        "binding_digest": request.binding_digest,
        "adviser_model": "fake-jev-adviser",
        "outcome": {"kind":"ranked","route_ids":["route-b","route-a"]},
    }))
    .unwrap()
}

async fn pinned_fixture() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let expected_system_id = std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap();
    let expected_database_oid: i64 = std::env::var("TECT_TEST_EXPECTED_DB_OID")
        .unwrap()
        .parse()
        .unwrap();
    let expected_port: u16 = std::env::var("TECT_TEST_EXPECTED_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    for (options, username) in [(&admin_options, "tony"), (&runtime_options, "tect_ci")] {
        assert_eq!(options.get_username(), username);
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(options.get_port(), expected_port);
    }
    let pool = PgPool::connect_with(admin_options).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_catalog.pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            "tect_test".into(),
            "tony".into(),
            expected_database_oid,
            expected_system_id
        )
    );
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let ledger: (i64, bool) =
        sqlx::query_as("SELECT max(version),bool_and(success) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(ledger, (82, true));
    let runtime = PgPool::connect_with(runtime_options).await.unwrap();
    let role: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,\
         (SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database())",
    )
    .fetch_one(&runtime)
    .await
    .unwrap();
    assert_eq!(
        role,
        ("tect_test".into(), "tect_ci".into(), expected_database_oid)
    );
    (pool, runtime_url)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only an explicitly identity-pinned disposable PostgreSQL 18.6 fixture"]
async fn public_model_route_recommends_from_exact_matrix_work_without_dispatch() {
    let (pool, runtime_url) = pinned_fixture().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("model-route-positive.sock");
    let matrix_calls = Arc::new(AtomicUsize::new(0));
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false)))))
        .with_matrix_advisory_adapters(Arc::new(Provider(matrix_calls.clone())), Arc::new(Budget))
        .with_model_route_catalogue_provider(Arc::new(Catalogue))
        .with_model_route_host_capabilities_provider(Arc::new(HostCapabilities))
        .with_model_route_ranking_provider(Arc::new(FakeAdviser {
            pool: pool.clone(),
            calls: calls.clone(),
        })),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("model-route-positive-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &owner_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
    let opened = owner.call("open_workspace", json!({})).await;
    let workspace = Uuid::parse_str(opened["workspace"]["id"].as_str().unwrap()).unwrap();
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;

    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":PROFILE},"model_configuration":{"model":MODEL}
        }),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record_task(&mut owner, task, &["a", "b"]).await;
    verify(&mut independent, &recorded, task).await;
    let advice_key = format!("model-route-matrix-{}", Uuid::new_v4());
    let advised = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,"request_key":advice_key
        }),
    )
    .await;
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":advice_key
        }),
    )
    .await;
    let selected = disposition(
        &recorded,
        task,
        &advised,
        "after_advice",
        Some(&current["current_advice"]),
        json!({"outcome":"selected","selected_choice_id":"b"}),
    );
    let chosen = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        selected,
    )
    .await;
    let verification_digest: String = sqlx::query_scalar(
        "SELECT matrix_verification_digest FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2"
    ).bind(workspace).bind(Uuid::parse_str(advised["opportunity_id"].as_str().unwrap()).unwrap()).fetch_one(&pool).await.unwrap();
    let matrix_selection = json!({
        "task_id":task,"task_revision":1,"disposition_id":chosen["disposition_id"],
        "selected_choice_id":"b","expected_input_digest":recorded["input_digest"],
        "expected_choice_set_digest":recorded["choice_set_digest"],
        "expected_verification_digest":verification_digest,"mapped_draft_node_indices":[0]
    });
    let scope = route(
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
    let mut save = planning_effect::save_request(&scope["created"]["planning"], matrix_selection);
    save["draft"]["nodes"][0]["model_route_facts"] = json!({
        "role":"agent","tool":"code","data_class":"internal",
        "remaining_budget_units":20,"available_latency_ms":100
    });
    let caller_request_id = save["request_id"].clone();
    let saved = route(&mut owner, "command", "slice.candidates.save", save).await;
    let work = &saved["draft"]["nodes"][0];
    let set = &saved["candidate_set"]["id"];
    let effect = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({
            "candidate_set_id":set,"caller_request_id":caller_request_id
        }),
    )
    .await;
    let matched = route(&mut independent, "command", "engineering.matrix.planning_effect.verify", json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller_request_id,
        "expected_result_revision":effect["material"]["result_revision"],
        "expected_effect_digest":effect["effect_digest"],
        "verdict":"matches","summary":"Exact selected Work remains mapped."
    })).await;
    assert_eq!(matched["verdict"], "matches");
    let key = format!("model-route-{}", Uuid::new_v4());
    let prepare_request = json!({
        "disposition_id":chosen["disposition_id"],"expected_task_id":task,
        "expected_task_revision":1,"expected_candidate_set_id":set,
        "expected_caller_request_id":caller_request_id,
        "expected_mapped_work_node_id":work["id"],
        "expected_mapped_work_node_revision":work["revision"],
        "request_key":key,"requested_route_id":"route-a"
    });
    let prepared = route(
        &mut owner,
        "command",
        "model.route.prepare",
        prepare_request.clone(),
    )
    .await;
    assert_eq!(prepared["preparation"], "Prepared");
    assert_eq!(
        prepared["work"]["approved_matrix_selection"]["selected_choice_id"],
        "b"
    );
    assert_eq!(
        prepared["work"]["selection_link"]["mapped_work_node_id"],
        work["id"]
    );
    assert_eq!(prepared["routes"]["requested_route_id"], "route-a");
    assert!(prepared["routes"]["recommended_route_id"].is_null());
    assert!(prepared["routes"]["observed_actual"].is_null());
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    let run = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({
            "preparation_request_key":key
        }),
    )
    .await;
    assert_eq!(run["attempt"]["state"], "parsed");
    assert_eq!(
        run["decision"]["outcome"]["Recommended"]["route_id"],
        "route-b"
    );
    assert_eq!(run["decision"]["routes"]["requested_route_id"], "route-a");
    assert_eq!(run["decision"]["routes"]["recommended_route_id"], "route-b");
    assert!(run["decision"]["routes"]["observed_actual"].is_null());
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let replay = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({
            "preparation_request_key":key
        }),
    )
    .await;
    assert_eq!(replay["decision"]["id"], run["decision"]["id"]);
    assert_eq!(calls.load(Ordering::SeqCst), 1);

    let row: (String, Vec<u8>, String, Vec<u8>, String, bool, bool) = sqlx::query_as(
        "SELECT state,request_payload,request_sha256,response_payload,response_sha256, \
         raw_sealed_at IS NOT NULL,parsed_at IS NOT NULL \
         FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2",
    )
    .bind(workspace)
    .bind(&key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "parsed");
    assert!(row.5 && row.6);
    assert_eq!(format!("{:x}", Sha256::digest(&row.1)), row.2);
    assert_eq!(format!("{:x}", Sha256::digest(&row.3)), row.4);
    let wire: ModelRouteRankingWireRequest = serde_json::from_slice(&row.1).unwrap();
    wire.validate().unwrap();
    assert_eq!(wire.binding.workspace_id, workspace);
    assert_eq!(wire.binding.eligible_route_ids, vec!["route-a", "route-b"]);
    assert_eq!(row.3, response(&wire));
    let audit: (String, String, Uuid, i64, bool) = sqlx::query_as(
        "SELECT capability,step,work_item_id,sum(call_count)::bigint,\
         bool_and(raw_sealed_at IS NOT NULL) FROM advisory_call_audit \
         WHERE workspace_id=$1 AND request_key=$2 GROUP BY capability,step,work_item_id",
    )
    .bind(workspace)
    .bind(&key)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        audit,
        (
            "model_routing".into(),
            "recommendation_before_model_choice".into(),
            Uuid::parse_str(work["id"].as_str().unwrap()).unwrap(),
            1,
            true
        )
    );
    let no_execution: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM native_slices WHERE workspace_id=$1),\
         (SELECT count(*) FROM model_route_dispositions WHERE workspace_id=$1)",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(no_execution, (0, 0));
    assert_eq!(matrix_calls.load(Ordering::SeqCst), 1);
    independent.finish().await;
    owner.finish().await;
    server.abort();
    let _ = server.await;
}
