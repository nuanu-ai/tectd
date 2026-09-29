//! Public S02 route proof with exact, approved PostgreSQL artifact bytes.
//! The adviser and owner response are synthetic; no installed runtime is used.
use super::*;
use recovery_support::public_call;
use tect_application::{FixedPipelineCompatibilityPolicy, PipelineProviderIdentity};
use tect_domain::{PipelineKind, RequiredMatrixFact};
use tect_host::jev_pipeline_recommendation::{JevPipelineSavedResponseParser, WIRE_VERSION};
use tect_postgres::{ApprovedMatrixEvidenceArtifact, PgMatrixEvidenceValidator};
use url::Url;

#[path = "s02_approved_artifact/s02_live.rs"]
mod s02_live;

fn relaunch_with_isolated_codex_home(test_name: &str) -> bool {
    let root = std::env::var("TECT_TEST_ISOLATED_ROOT").ok();
    let home = std::env::var("CODEX_HOME").ok();
    if root.is_some() && home.is_some() {
        return false;
    }
    assert!(
        root.is_none() && home.is_none(),
        "partial S02 isolation environment"
    );
    assert_eq!(std::env::var("GITHUB_ACTIONS").as_deref(), Ok("true"));
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let home = root.join("codex-home");
    std::fs::create_dir(&home).unwrap();
    let status = std::process::Command::new(std::env::current_exe().unwrap())
        .arg(test_name)
        .arg("--exact")
        .arg("--ignored")
        .env("TECT_TEST_ISOLATED_ROOT", &root)
        .env("CODEX_HOME", &home)
        .status()
        .unwrap();
    assert!(status.success(), "isolated S02 child failed: {status}");
    true
}

async fn fresh_artifact_fixture() -> (PgPool, String) {
    let isolated_root =
        std::fs::canonicalize(std::env::var("TECT_TEST_ISOLATED_ROOT").unwrap()).unwrap();
    let codex_home = std::fs::canonicalize(std::env::var("CODEX_HOME").unwrap()).unwrap();
    assert_eq!(codex_home, isolated_root.join("codex-home"));
    assert_ne!(
        codex_home,
        std::path::PathBuf::from(std::env::var("HOME").unwrap()).join(".codex")
    );
    let ci = std::env::var("GITHUB_ACTIONS").as_deref() == Ok("true");
    let expected_data = if ci {
        let run = std::env::var("GITHUB_RUN_ID").unwrap();
        let attempt = std::env::var("GITHUB_RUN_ATTEMPT").unwrap();
        assert_eq!(
            std::env::var("TECT_TEST_DB_NAME").unwrap(),
            format!("tect_matrix_approved_{run}_{attempt}")
        );
        assert_eq!(std::env::var("TECT_TEST_PG_PORT").as_deref(), Ok("5432"));
        None
    } else {
        Some(std::fs::canonicalize(isolated_root.join("pgdata")).unwrap())
    };
    let own_exe = std::fs::canonicalize(std::env::current_exe().unwrap()).unwrap();
    let build_dir = own_exe.parent().unwrap().parent().unwrap();
    let mcp_exe = std::fs::canonicalize(env!("CARGO_BIN_EXE_tectd-mcp")).unwrap();
    assert_eq!(mcp_exe, build_dir.join("tectd-mcp"));
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let name = std::env::var("TECT_TEST_DB_NAME").unwrap();
    assert!(name.starts_with("tect_matrix_approved_"));
    let port: u16 = std::env::var("TECT_TEST_PG_PORT").unwrap().parse().unwrap();
    assert_ne!(port, 64775);
    let system_id = std::env::var("TECT_TEST_SYSTEM_ID").unwrap();
    let oid: i64 = std::env::var("TECT_TEST_DATABASE_OID")
        .unwrap()
        .parse()
        .unwrap();
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    for (raw, user) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let url = Url::parse(raw).unwrap();
        assert!(matches!(url.scheme(), "postgres" | "postgresql"));
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(port));
        assert_eq!(url.path(), format!("/{name}"));
        assert_eq!(url.username(), user);
        assert!(url.query().is_none() && url.fragment().is_none());
    }
    let pool = PgPool::connect_with(PgConnectOptions::from_str(&admin_url).unwrap())
        .await
        .unwrap();
    let identity: (i32, String, String, i64, String, bool, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         to_regclass('public._sqlx_migrations') IS NULL,current_setting('data_directory')",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        (
            &identity.0,
            &identity.1,
            &identity.2,
            &identity.3,
            &identity.4,
            &identity.5
        ),
        (
            &180006,
            &name,
            &"postgres".to_owned(),
            &oid,
            &system_id,
            &true
        )
    );
    if let Some(expected_data) = expected_data {
        assert_eq!(std::fs::canonicalize(&identity.6).unwrap(), expected_data);
    }
    let runtime: (String, String) = sqlx::query_as("SELECT current_database(),current_user")
        .fetch_one(
            &PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
                .await
                .unwrap(),
        )
        .await
        .unwrap();
    assert_eq!(runtime, (identity.1, "tect_ci".into()));
    (pool, runtime_url)
}

fn sourced(fact: Value, observed_at: i64, expires_at: i64) -> Value {
    json!({"fact":fact,"source_ref":"system-observation:synthetic-42",
        "observed_at":observed_at,"expires_at":expires_at})
}

fn artifact_body(workspace: Uuid, task: Uuid, observed_at: i64, expires_at: i64) -> String {
    let input = input();
    serde_json::to_string(&json!({
        "schema":"tect.matrix-operating-evidence/1","workspace_id":workspace,
        "task_id":task,"task_revision":1,
        "scale":sourced(input["envelope"]["scale"].clone(),observed_at,expires_at),
        "criticality":sourced(input["criticality"].clone(),observed_at,expires_at),
        "affected_guarantees":sourced(input["affected_guarantees"].clone(),observed_at,expires_at),
        "actual_exposure":sourced(input["actual_exposure"].clone(),observed_at,expires_at),
        "urgent_repair":sourced(input["urgent_repair"].clone(),observed_at,expires_at),
        "operational_facts":sourced(input["envelope"]["operational_facts"].clone(),observed_at,expires_at)
    })).unwrap()
}

fn planning_save(planning: &Value, selection: Value) -> Value {
    let mut request = json!({
        "kind":"draft","scope_id":planning["scope"]["id"],
        "candidate_set_id":planning["candidate_set"]["id"],
        "revision":planning["candidate_set"]["revision"],
        "snapshot_id":planning["snapshot"]["id"],
        "input_cursor":planning["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),
        "draft":{"coverage_summary":"Selected Matrix choice a bounds this synthetic work",
            "nodes":[{"kind":"work","identity":{"local":"choice-a"},
                "title":"Investigate selected approach a",
                "outcome":"The selected approach has a documented diagnosis",
                "includes":["selected approach a"],"excludes":["deployment"],
                "dependencies":[],"proof":["Synthetic diagnosis is recorded"],
                "pipeline":"slice.debug-root-cause",
                "pipeline_reason":"The selected approach needs a bounded diagnosis",
                "source_result_ids":[]}],"supersessions":[]},
        "matrix_selection":selection
    });
    let manifest = &planning["planning_knowledge"]["manifest"];
    if manifest["id"].as_str().is_some() {
        request["consumed_knowledge"] = json!({
            "manifest_id":manifest["id"],"digest":manifest["digest"],
            "workspace_generation":manifest["workspace_generation"]
        });
    }
    request
}

async fn artifact_route(client: &mut Mcp, tool: &str, name: &str, params: Value) -> Value {
    let response = client
        .exchange(
            "tools/call",
            public_call(tool, json!({"route":name,"params":params})),
        )
        .await;
    assert!(
        response.get("error").is_none() && response["result"]["isError"] != true,
        "{response}"
    );
    serde_json::from_str(response["result"]["content"][1]["text"].as_str().unwrap()).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh owned PostgreSQL 18.6 cluster and explicit identity guard"]
async fn public_artifact_issuance_revalidates_exact_task_revision_and_bytes() {
    if relaunch_with_isolated_codex_home(
        "s02_approved_artifact::public_artifact_issuance_revalidates_exact_task_revision_and_bytes",
    ) {
        return;
    }
    let (pool, runtime_url) = fresh_artifact_fixture().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("matrix-artifact-issuance.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(store),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let host = root.join("owner.json");
    host_file(&host, &enrolled.auth);
    let mut owner = Mcp::start(
        &socket,
        &host,
        &Uuid::new_v4().to_string(),
        &format!("matrix-artifact-{}", Uuid::new_v4()),
    )
    .await;
    let opened = route(&mut owner, "command", "workspace.open", json!({})).await;
    let workspace = id(&opened["workspace"]["id"]);
    let task = Uuid::new_v4();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    // Fixture values exercise the issuance mechanism only. They are not
    // approved operating observations and this test has no advice provider.
    let body = artifact_body(workspace, task, now - 10, now + 600);
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    let registered = artifact_route(
        &mut owner,
        "command",
        "slice.pipeline.evidence_artifact.register",
        json!({
            "request_id":Uuid::new_v4(),"digest":digest,"size":body.len(),
            "format":"application/vnd.tect.matrix-operating-evidence+json;version=1",
            "provenance":"synthetic-public-issuance-test","target":format!("matrix-task:{task}@1")
        }),
    )
    .await;
    let artifact_id = id(&registered["artifact"]["artifact_id"]);
    assert_eq!(registered["artifact"]["readiness"], "uploading");
    let finalized = artifact_route(
        &mut owner,
        "command",
        "slice.pipeline.evidence_artifact.finalize",
        json!({"request_id":Uuid::new_v4(),"artifact_id":artifact_id,
            "revision":1,"body":body}),
    )
    .await;
    assert_eq!(finalized["artifact"]["readiness"], "ready");
    let read = artifact_route(
        &mut owner,
        "query",
        "slice.pipeline.evidence_artifact.read",
        json!({"artifact_id":artifact_id,"revision":1,"offset":0,"limit":65536}),
    )
    .await;
    assert_eq!(read["fragment"], body);
    assert_eq!(read["complete"], true);
    let approval = ApprovedMatrixEvidenceArtifact {
        tenant_id: enrolled.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        artifact_id,
        artifact_revision: 1,
        sha256: digest.clone(),
        policy_version: "synthetic-issuance-check/1".into(),
        max_age_seconds: 600,
    };
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
        .await
        .unwrap();
    let validator = PgMatrixEvidenceValidator::new(runtime_pool, approval);
    let parsed: EngineeringMatrixInput = serde_json::from_value(input()).unwrap();
    let scale = &parsed.envelope.scale;
    let fact = RequiredMatrixFact {
        path: "/envelope/scale".into(),
        value_digest: format!("{:x}", Sha256::digest(serde_json::to_vec(scale).unwrap())),
    };
    let reference = format!("pipeline-evidence:{artifact_id}@1");
    let binding = validator
        .validate(workspace, task, 1, &fact, &reference, now)
        .await
        .unwrap();
    assert_eq!(binding.content_digest, digest);
    validator
        .revalidate(workspace, task, 1, &fact, &binding, now)
        .await
        .unwrap();
    assert!(
        validator
            .validate(workspace, task, 2, &fact, &reference, now)
            .await
            .is_err()
    );
    assert!(
        validator
            .validate(workspace, Uuid::new_v4(), 1, &fact, &reference, now)
            .await
            .is_err()
    );
    assert!(
        validator
            .validate(workspace, task, 1, &fact, &reference, now + 601)
            .await
            .is_err()
    );
    owner.finish().await;
    server.abort();
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "requires a fresh owned PostgreSQL 18.6 cluster and explicit identity guard"]
async fn public_s02_approved_artifact_to_independently_verified_planning_effect() {
    if relaunch_with_isolated_codex_home(
        "s02_approved_artifact::public_s02_approved_artifact_to_independently_verified_planning_effect",
    ) {
        return;
    }
    let (pool, runtime_url) = fresh_artifact_fixture().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("matrix-approved-artifact.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("matrix-approved-{}", Uuid::new_v4());
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
    let (workspace, owner_keys) = signed_fixture_budget(&store, &enrolled, &workspace_key).await;
    let task = Uuid::new_v4();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let body = artifact_body(workspace, task, now - 10, now + 600);
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    // Public issuance assigns the artifact ID. Bootstrap the same disposable
    // workspace without a validator, then bind the validator to those exact
    // finalized bytes for the independent V2 verification below.
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let issuance_socket = root.join("matrix-planning-issuance.sock");
    let issuance_service = Arc::new(WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let issuance_listener = UnixListener::bind(&issuance_socket).unwrap();
    std::fs::set_permissions(&issuance_socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let issuance_server = tokio::spawn(tect_host::serve(issuance_listener, issuance_service));
    let mut issuer = Mcp::start(
        &issuance_socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let opened = route(&mut issuer, "command", "workspace.open", json!({})).await;
    assert_eq!(id(&opened["workspace"]["id"]), workspace);
    let registered = artifact_route(
        &mut issuer,
        "command",
        "slice.pipeline.evidence_artifact.register",
        json!({
            "request_id":Uuid::new_v4(),"digest":digest,"size":body.len(),
            "format":"application/vnd.tect.matrix-operating-evidence+json;version=1",
            "provenance":"synthetic-public-planning-test",
            "target":format!("matrix-task:{task}@1")
        }),
    )
    .await;
    let artifact_id = id(&registered["artifact"]["artifact_id"]);
    assert_eq!(registered["artifact"]["readiness"], "uploading");
    let finalized = artifact_route(
        &mut issuer,
        "command",
        "slice.pipeline.evidence_artifact.finalize",
        json!({"request_id":Uuid::new_v4(),"artifact_id":artifact_id,
            "revision":1,"body":body}),
    )
    .await;
    assert_eq!(finalized["artifact"]["readiness"], "ready");
    let read = artifact_route(
        &mut issuer,
        "query",
        "slice.pipeline.evidence_artifact.read",
        json!({"artifact_id":artifact_id,"revision":1,"offset":0,"limit":65536}),
    )
    .await;
    assert_eq!(read["fragment"], body);
    assert_eq!(read["complete"], true);
    issuer.finish().await;
    issuance_server.abort();
    let approval = ApprovedMatrixEvidenceArtifact {
        tenant_id: enrolled.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        artifact_id,
        artifact_revision: 1,
        sha256: digest.clone(),
        policy_version: "matrix-approved-artifact/1".into(),
        max_age_seconds: 600,
    };
    let calls = Arc::new(AtomicUsize::new(0));
    let budgets = Arc::new(AtomicUsize::new(0));
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
        .await
        .unwrap();
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.with_budget_owner_keys(owner_keys.clone())),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(
            runtime_pool,
            approval.clone(),
        )))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(calls.clone())),
            Arc::new(Budget(budgets.clone())),
        ),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let mut owner = Mcp::start(
        &socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
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
    let recorded = record(&mut owner, task, Uuid::new_v4(), Some(locator(program))).await;
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
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_host = root.join("verifier.json");
    host_file(&verifier_host, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let parsed: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(effective).unwrap();
    let facts = required_matrix_operating_facts(&context, &parsed).unwrap();
    let reference = format!("pipeline-evidence:{artifact_id}@1");
    let evidence: Vec<Value> = facts
        .iter()
        .map(|fact| json!({"fact_path":fact.path,"evidence_ref":reference}))
        .collect();
    let mut unapproved = evidence.clone();
    unapproved[0]["evidence_ref"] = json!(format!("pipeline-evidence:{}@1", Uuid::new_v4()));
    let denied = route_error(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({
            "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],
            "evidence":unapproved
        }),
    )
    .await;
    assert_eq!(denied["error"]["code"], "forbidden", "{denied}");
    let before = advice(&mut owner, task, &format!("unverified-{}", Uuid::new_v4())).await;
    assert_eq!(before["state"], "no_call", "{before}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(budgets.load(Ordering::SeqCst), 0);
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
    assert_eq!(verified["policy_version"], "matrix-approved-artifact/1");
    assert_eq!(verified["facts"].as_array().unwrap().len(), facts.len());
    for binding in verified["facts"].as_array().unwrap() {
        assert_eq!(binding["content_digest"], digest);
    }
    let sourced_bindings: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM matrix_verification_bindings b \
         JOIN matrix_verifications v ON v.tenant_id=b.tenant_id AND v.workspace_id=b.workspace_id AND v.id=b.verification_id \
         WHERE v.workspace_id=$1 AND v.task_id=$2 AND v.task_revision=1 AND \
         b.source='system-observation:synthetic-42' AND b.content_digest=$3 AND b.evidence_ref=$4",
    ).bind(workspace).bind(task).bind(&digest).bind(&reference).fetch_one(&pool).await.unwrap();
    assert_eq!(sourced_bindings as usize, facts.len());

    // The saved verification cannot authorize advice once its approved bytes change.
    let altered_body = body.replacen(
        "system-observation:synthetic-42",
        "system-observation:synthetic-43",
        1,
    );
    assert_ne!(altered_body, body);
    let altered = sqlx::query(
        "UPDATE pipeline_evidence_artifacts SET body=$1 \
         WHERE tenant_id=$2 AND workspace_id=$3 AND artifact_id=$4 AND revision=1",
    )
    .bind(&altered_body)
    .bind(enrolled.tenant_id)
    .bind(workspace)
    .bind(artifact_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(altered.rows_affected(), 1);
    let stale = advice(
        &mut owner,
        task,
        &format!("stale-artifact-{}", Uuid::new_v4()),
    )
    .await;
    assert_eq!(stale["state"], "no_call", "{stale}");
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(budgets.load(Ordering::SeqCst), 0);

    let restored = sqlx::query(
        "UPDATE pipeline_evidence_artifacts SET body=$1 \
         WHERE tenant_id=$2 AND workspace_id=$3 AND artifact_id=$4 AND revision=1",
    )
    .bind(&body)
    .bind(enrolled.tenant_id)
    .bind(workspace)
    .bind(artifact_id)
    .execute(&pool)
    .await
    .unwrap();
    assert_eq!(restored.rows_affected(), 1);
    let advice_key = format!("approved-{}", Uuid::new_v4());
    let advised = advice(&mut owner, task, &advice_key).await;
    assert_eq!(advised["state"], "advised", "{advised}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    assert_eq!(budgets.load(Ordering::SeqCst), 1);
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
            "decision":{"outcome":"selected","selected_choice_id":"a"}
        }),
    )
    .await;
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
    let selection = json!({
        "task_id":task,"task_revision":1,"disposition_id":chosen["disposition_id"],
        "selected_choice_id":"a","expected_input_digest":recorded["input_digest"],
        "expected_choice_set_digest":recorded["choice_set_digest"],
        "expected_verification_digest":verified["verification_digest"],
        "mapped_draft_node_indices":[0]
    });
    let save = planning_save(&scope["created"]["planning"], selection);
    let caller_request = save["request_id"].clone();
    let saved = route(&mut owner, "command", "slice.candidates.save", save).await;
    let effect = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({
            "candidate_set_id":saved["candidate_set"]["id"],"caller_request_id":caller_request
        }),
    )
    .await;
    assert_eq!(effect["material"]["selected_choice"]["candidate_id"], "a");
    let attested = route(&mut independent, "command", "engineering.matrix.planning_effect.verify", json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":saved["candidate_set"]["id"],
        "caller_request_id":caller_request,"expected_result_revision":effect["material"]["result_revision"],
        "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
        "summary":"Exact selected synthetic Work remains mapped."
    })).await;
    assert_eq!(attested["verdict"], "matches");
    let ready = support::review(&mut owner, &saved).await;
    assert_eq!(ready["candidate_set"]["status"], "ready");
    independent.finish().await;
    owner.finish().await;
    server.abort();

    // A separate branch-built host prepares advice from the same verified V2
    // Work. The saved-response adapter has no transport and is never run.
    let pipeline_socket = root.join("matrix-approved-pipeline.sock");
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
        .await
        .unwrap();
    let no_send_provider = JevPipelineSavedResponseParser::new(PipelineProviderIdentity {
        provider: PROFILE.into(),
        model: MODEL.into(),
        destination: "https://synthetic.invalid/no-send".into(),
        wire_version: WIRE_VERSION.into(),
    })
    .unwrap();
    let pipeline_service = Arc::new(
        WorkspaceService::new(
            Arc::new(
                PgStore::connect(&runtime_url, 4)
                    .await
                    .unwrap()
                    .with_budget_owner_keys(owner_keys),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(
            runtime_pool,
            approval,
        )))
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        ))
        .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(
            super::s03_v4::policy_for_choice(
                task,
                &parsed,
                "a",
                &[
                    PipelineKind::DebugRootCause,
                    PipelineKind::DeepBrainstorming,
                ],
            ),
        )))
        .with_pipeline_recommendation_provider(Arc::new(no_send_provider)),
    );
    let listener = UnixListener::bind(&pipeline_socket).unwrap();
    std::fs::set_permissions(&pipeline_socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let pipeline_server = tokio::spawn(tect_host::serve(listener, pipeline_service));
    let mut owner = Mcp::start(
        &pipeline_socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    let work = &saved["draft"]["nodes"][0];
    let prepared = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        json!({
            "candidate_set_id":saved["candidate_set"]["id"],
            "expected_candidate_set_revision":ready["candidate_set"]["revision"],
            "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
            "request_key":format!("approved-pipeline-{}",Uuid::new_v4())
        }),
    )
    .await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    let opportunity_id = id(&prepared["opportunity_id"]);
    let manifest: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    // Public verification receipts intentionally omit evidence_ref; read the
    // independently sealed, exact task/revision/digest bindings for comparison.
    let verified_refs: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT b.evidence_ref FROM matrix_verification_bindings b \
         JOIN matrix_verifications v ON (v.tenant_id,v.workspace_id,v.id)= \
             (b.tenant_id,b.workspace_id,b.verification_id) \
         WHERE v.workspace_id=$1 AND v.task_id=$2 AND v.task_revision=1 \
           AND v.record_digest=$3 AND v.schema='tect.context-matrix-verification/1' \
         ORDER BY b.evidence_ref",
    )
    .bind(workspace)
    .bind(task)
    .bind(verified["verification_digest"].as_str().unwrap())
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(verified_refs, [reference]);
    assert_eq!(manifest["evidence_refs"], json!(verified_refs));
    assert_eq!(manifest["matrix_task_id"], task.to_string());
    assert_eq!(manifest["matrix_task_revision"], "1");
    let dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dispatches, 0);
    owner.finish().await;
    pipeline_server.abort();
}
