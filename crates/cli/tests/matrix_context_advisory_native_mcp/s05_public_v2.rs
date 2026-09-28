//! S05 public MCP proof against exact V2 Matrix Work; synthetic adviser only.
use super::*;
#[path = "s05_public_v2/s05_historical.rs"]
mod s05_historical;
#[path = "s05_public_v2/s05_live.rs"]
mod s05_live;
use tect_application::{
    ModelRouteCatalogueProvider, ModelRouteHostCapabilitiesProvider, ModelRoutePreparedAttempt,
    ModelRouteProviderObservation, ModelRouteRankingProvider, ModelRouteSendPermit,
    PreparedModelRouteRecommendation,
};
use tect_domain::{
    Error, MODEL_ROUTE_CATALOGUE_SCHEMA, MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA,
    MODEL_ROUTE_RANKING_WIRE_SCHEMA, ModelRoute, ModelRouteCatalogue, ModelRouteFact,
    ModelRouteFactProvenance, ModelRouteHostCapabilities, ModelRouteRankingWireRequest,
    ModelRouteRecord, ModelRouteSelectionLink, ModelRouteWorkContext,
};

async fn s05_session_fixture(target_version: i64) -> (PgPool, String) {
    assert!(matches!(target_version, 104 | 113));
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let database = std::env::var("TECT_TEST_DB_NAME").unwrap();
    assert!(database.starts_with("tect_modelroute_session_"));
    let expected_system_id = std::env::var("TECT_TEST_SYSTEM_ID").unwrap();
    let expected_oid: i64 = std::env::var("TECT_TEST_DATABASE_OID")
        .unwrap()
        .parse()
        .unwrap();
    let expected_port: u16 = std::env::var("TECT_TEST_PG_PORT").unwrap().parse().unwrap();
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    for (url, role) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let parsed = Url::parse(url).unwrap();
        assert!(matches!(parsed.scheme(), "postgres" | "postgresql"));
        assert_eq!(parsed.host_str(), Some("127.0.0.1"));
        assert_eq!(parsed.port(), Some(expected_port));
        assert_eq!(parsed.path(), format!("/{database}"));
        assert_eq!(parsed.username(), role);
        assert!(parsed.query().is_none() && parsed.fragment().is_none());
    }
    let pool = PgPool::connect_with(PgConnectOptions::from_str(&admin_url).unwrap())
        .await
        .unwrap();
    let identity: (i32, String, i64, String, bool) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         to_regclass('public._sqlx_migrations') IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            database.clone(),
            expected_oid,
            expected_system_id,
            true
        )
    );
    if target_version == 104 {
        let staged = tempfile::tempdir().unwrap();
        let migrations =
            std::path::Path::new(env!("CARGO_MANIFEST_DIR")).join("../postgres/migrations");
        let mut copied = 0;
        for entry in std::fs::read_dir(migrations).unwrap() {
            let entry = entry.unwrap();
            let name = entry.file_name();
            let version: i64 = name.to_str().unwrap()[..4].parse().unwrap();
            if version <= 104 {
                std::fs::copy(entry.path(), staged.path().join(name)).unwrap();
                copied += 1;
            }
        }
        assert_eq!(copied, 104);
        sqlx::migrate::Migrator::new(staged.path())
            .await
            .unwrap()
            .run(&pool)
            .await
            .unwrap();
    } else {
        admin::migrate(&pool, "tect_ci").await.unwrap();
    }
    let ledger: Vec<(i64, bool)> =
        sqlx::query_as("SELECT version,success FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(ledger.len(), target_version as usize);
    for (index, (version, success)) in ledger.iter().enumerate() {
        assert_eq!(*version, index as i64 + 1);
        assert!(*success);
    }
    let runtime: (String, String) = sqlx::query_as("SELECT current_database(),current_user")
        .fetch_one(
            &PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime, (database, "tect_ci".into()));
    (pool, runtime_url)
}

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
#[ignore = "writes only exact owned PostgreSQL 18.6 fixture at migration 113; synthetic adviser"]
async fn public_s05_v2_work_caller_assertions_rank_eligible_ids_only() {
    let (pool, runtime_url) = s05_session_fixture(113).await;
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
    let skip_key = format!("s05-v2-skip-{}", Uuid::new_v4());
    let skip_input = json!({
        "disposition_id":chosen["disposition_id"],"expected_task_id":task,
        "expected_task_revision":1,"expected_candidate_set_id":set,
        "expected_caller_request_id":caller_request,"expected_mapped_work_node_id":work["id"],
        "expected_mapped_work_node_revision":work["revision"],
        "request_key":skip_key,"requested_route_id":"route-a"
    });
    let mut forged = skip_input.clone();
    forged["session_preference"] = json!("skip");
    assert_eq!(
        route_error(&mut owner, "command", "model.route.prepare", forged).await["error"]["code"],
        "invalid_arguments"
    );
    let skipped_setting = route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":0,"preference":"skip"}),
    )
    .await;
    assert_eq!(skipped_setting["preference"], "skip");
    let skipped = route(
        &mut owner,
        "command",
        "model.route.prepare",
        skip_input.clone(),
    )
    .await;
    assert_eq!(skipped["preparation"], "SessionSkip");
    assert_eq!(skipped["session_preference"], "skip");
    assert!(!skipped["origin_session_id"].is_null());
    let skipped_run = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":skip_key}),
    )
    .await;
    assert_eq!(skipped_run["attempt"]["no_call_reason"], "session_skip");
    assert!(skipped_run["decision"]["routes"]["recommended_route_id"].is_null());
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 0);
    let unskipped_setting = route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":1,"preference":"use_workspace"}),
    )
    .await;
    assert_eq!(unskipped_setting["preference"], "use_workspace");
    assert_eq!(
        route(&mut owner, "command", "model.route.prepare", skip_input).await,
        skipped
    );
    let skipped_replay = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":skip_key}),
    )
    .await;
    assert_eq!(skipped_replay["attempt"]["no_call_reason"], "session_skip");
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 0);

    // Upgrade compatibility: a pre-0113 immutable receipt has no origin
    // session. It remains readable but cannot be used to initiate a send.
    let legacy_key = format!("s05-v2-legacy-{}", Uuid::new_v4());
    sqlx::query(
        "INSERT INTO model_route_preparations \
         (tenant_id,workspace_id,request_key,disposition_id,candidate_set_id,caller_request_id, \
          work_node_id,work_node_revision,task_id,task_revision,advisory_mode, \
          advisory_config_revision,work_digest,catalogue_digest,host_capability_evidence_ref,prepared_payload) \
         SELECT tenant_id,workspace_id,$2,disposition_id,candidate_set_id,caller_request_id, \
          work_node_id,work_node_revision,task_id,task_revision,advisory_mode, \
          advisory_config_revision,work_digest,catalogue_digest,host_capability_evidence_ref, \
          jsonb_set(prepared_payload - 'origin_session_id','{request_key}',to_jsonb($2::text)) \
         FROM model_route_preparations WHERE workspace_id=$3 AND request_key=$1",
    )
    .bind(&skip_key)
    .bind(&legacy_key)
    .bind(workspace)
    .execute(&pool)
    .await
    .unwrap();
    let legacy_read = route(
        &mut owner,
        "query",
        "model.route.get",
        json!({"preparation_request_key":legacy_key}),
    )
    .await;
    assert!(legacy_read["preparation"]["origin_session_id"].is_null());
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "model.route.run",
            json!({"preparation_request_key":legacy_key})
        )
        .await["error"]["code"],
        "forbidden"
    );
    let mut legacy_tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(enrolled.tenant_id.to_string())
        .execute(&mut *legacy_tx)
        .await
        .unwrap();
    let forged_legacy_attempt = sqlx::query(
        "INSERT INTO model_route_advisory_attempts \
         (tenant_id,workspace_id,id,preparation_request_key,invoking_session_id, \
          invoking_principal_id,candidate_set_id,work_node_id,work_node_revision, \
          task_id,task_revision,work_digest,catalogue_digest,host_capability_evidence_ref, \
          state,no_call_reason) \
         SELECT tenant_id,workspace_id,$3,$2,invoking_session_id, \
          invoking_principal_id,candidate_set_id,work_node_id,work_node_revision, \
          task_id,task_revision,work_digest,catalogue_digest,host_capability_evidence_ref, \
          state,no_call_reason FROM model_route_advisory_attempts \
         WHERE workspace_id=$4 AND preparation_request_key=$1",
    )
    .bind(&skip_key)
    .bind(&legacy_key)
    .bind(Uuid::new_v4())
    .bind(workspace)
    .execute(&mut *legacy_tx)
    .await
    .unwrap_err();
    assert_eq!(
        forged_legacy_attempt
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("42501")
    );
    legacy_tx.rollback().await.unwrap();
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 0);

    // A skip committed after preparation but before send authorization wins
    // the native-session fence and seals a durable no-call.
    let late_skip_key = format!("s05-v2-late-skip-{}", Uuid::new_v4());
    let late_skip_prepared = route(
        &mut owner,
        "command",
        "model.route.prepare",
        json!({
            "disposition_id":chosen["disposition_id"],"expected_task_id":task,
            "expected_task_revision":1,"expected_candidate_set_id":set,
            "expected_caller_request_id":caller_request,"expected_mapped_work_node_id":work["id"],
            "expected_mapped_work_node_revision":work["revision"],
            "request_key":late_skip_key,"requested_route_id":"route-a"
        }),
    )
    .await;
    assert_eq!(late_skip_prepared["preparation"], "Prepared");
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":2,"preference":"skip"}),
    )
    .await;
    let late_skip_run = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":late_skip_key}),
    )
    .await;
    assert_eq!(late_skip_run["attempt"]["no_call_reason"], "session_skip");
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 0);
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":3,"preference":"use_workspace"}),
    )
    .await;
    let late_skip_replay = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":late_skip_key}),
    )
    .await;
    assert_eq!(
        late_skip_replay["attempt"]["attempt_id"],
        late_skip_run["attempt"]["attempt_id"]
    );
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 0);

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
    // Once the send has been authorized and sealed, a later skip cannot
    // replace its immutable evidence or trigger a second provider call.
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":4,"preference":"skip"}),
    )
    .await;
    let replay = route(
        &mut owner,
        "command",
        "model.route.run",
        json!({"preparation_request_key":key}),
    )
    .await;
    assert_eq!(replay["decision"]["id"], run["decision"]["id"]);
    assert_eq!(ranking_calls.load(Ordering::SeqCst), 1);
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":5,"preference":"use_workspace"}),
    )
    .await;
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
    let changed_request = json!({
        "disposition_id":chosen["disposition_id"],"expected_task_id":task,
        "expected_task_revision":1,"expected_candidate_set_id":set,
        "expected_caller_request_id":caller_request,"expected_mapped_work_node_id":work["id"],
        "expected_mapped_work_node_revision":work["revision"],
        "request_key":later_key,"requested_route_id":"route-a","request_preference":"skip"
    });
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "model.route.prepare",
            changed_request
        )
        .await["error"]["code"],
        "input_conflict"
    );
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
    assert_eq!(
        route_error(
            &mut fresh,
            "command",
            "model.route.run",
            json!({"preparation_request_key":later_key})
        )
        .await["error"]["code"],
        "forbidden"
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
    assert_eq!(persisted, (4, 2, 0));
    let states: Vec<(String, i64)> = sqlx::query_as(
        "SELECT state,count(*) FROM model_route_advisory_attempts WHERE workspace_id=$1 \
         GROUP BY state ORDER BY state",
    )
    .bind(workspace)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(states, [("no_call".into(), 2), ("parsed".into(), 2)]);
    let mut immutable_tx = pool.begin().await.unwrap();
    sqlx::query("SELECT set_config('tect.tenant_id',$1,true)")
        .bind(enrolled.tenant_id.to_string())
        .execute(&mut *immutable_tx)
        .await
        .unwrap();
    let immutable_error = sqlx::query(
        "UPDATE model_route_advisory_attempts SET no_call_reason='provider_unavailable' \
         WHERE workspace_id=$1 AND preparation_request_key=$2",
    )
    .bind(workspace)
    .bind(&skip_key)
    .execute(&mut *immutable_tx)
    .await
    .unwrap_err();
    assert_eq!(
        immutable_error
            .as_database_error()
            .unwrap()
            .code()
            .as_deref(),
        Some("23514")
    );
    immutable_tx.rollback().await.unwrap();
    assert_eq!(matrix_calls.load(Ordering::SeqCst), 1);
    independent.finish().await;
    owner.finish().await;
    fresh.finish().await;
    server.abort();
    let _ = server.await;
}
