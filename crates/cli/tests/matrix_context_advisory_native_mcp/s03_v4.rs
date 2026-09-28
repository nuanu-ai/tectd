//! S03 public native MCP happy path with exact V2 Matrix authority and schema/4 Pipeline advice.
//! All identities and responses are synthetic; this test never reaches a real provider.
use super::*;
use std::time::Duration;
use tect_application::{
    FixedPipelineCompatibilityPolicy, PipelineProviderIdentity,
    PipelineRecommendationDefinitionProvider,
};
use tect_domain::{
    EngineeringMode, PIPELINE_COMPATIBILITY_POLICY_VERSION, PipelineCardCoverage,
    PipelineCompatibilityPolicy, PipelineCompatibilityRule, PipelineExclusionReason, PipelineKind,
    PipelineRecommendationManifest, PipelineVerificationObligation, VerifiedEngineeringMatrixFacts,
    compose_engineering_matrix, matrix_input_digest, pipeline_obligation_digest,
};
use tect_host::jev_pipeline_recommendation::{
    JevPipelineConfig, JevPipelineProvider, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES, WIRE_VERSION,
    prepare_native_request,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
};
use url::Url;

#[path = "s03_v4/s03_live.rs"]
mod s03_live;
#[path = "s05_public_v2.rs"]
mod s05_public_v2;

async fn exact_fixture() -> (PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let expected_system_id = std::env::var("TECT_TEST_SYSTEM_ID").unwrap();
    let expected_oid: i64 = std::env::var("TECT_TEST_DATABASE_OID")
        .unwrap()
        .parse()
        .unwrap();
    let expected_database = std::env::var("TECT_TEST_DB_NAME").unwrap();
    let expected_port: u16 = std::env::var("TECT_TEST_PG_PORT").unwrap().parse().unwrap();
    assert_eq!(
        std::env::var("TECT_TEST_RUNTIME_ROLE").as_deref(),
        Ok("tect_ci")
    );
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_url_parsed = Url::parse(&admin_url).unwrap();
    let runtime_url_parsed = Url::parse(&runtime_url).unwrap();
    for (url, role) in [
        (&admin_url_parsed, "postgres"),
        (&runtime_url_parsed, "tect_ci"),
    ] {
        assert!(matches!(url.scheme(), "postgres" | "postgresql"));
        assert_eq!(url.host_str(), Some("127.0.0.1"));
        assert_eq!(url.port(), Some(expected_port));
        assert_eq!(url.path(), format!("/{expected_database}"));
        assert_eq!(url.username(), role);
        assert!(url.query().is_none() && url.fragment().is_none());
    }
    let pool = PgPool::connect_with(PgConnectOptions::from_str(&admin_url).unwrap())
        .await
        .unwrap();
    let preflight: (i32, String, i64, String, bool) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system()),\
         to_regclass('public._sqlx_migrations') IS NULL",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        preflight,
        (
            180006,
            expected_database.clone(),
            expected_oid,
            expected_system_id.clone(),
            true
        )
    );
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        identity,
        (
            180006,
            expected_database.clone(),
            "postgres".into(),
            expected_oid,
            expected_system_id
        )
    );
    let ledger: Vec<(i64, bool, Vec<u8>)> =
        sqlx::query_as("SELECT version,success,checksum FROM _sqlx_migrations ORDER BY version")
            .fetch_all(&pool)
            .await
            .unwrap();
    assert_eq!(
        ledger.len(),
        111,
        "fixture must have exactly migrations 1 through 111"
    );
    for (index, (version, success, _)) in ledger.iter().enumerate() {
        assert_eq!(*version, index as i64 + 1);
        assert!(*success, "migration {version} is not successful");
    }
    for (version, bytes) in [
        (
            105,
            include_bytes!(
                "../../../postgres/migrations/0105_matrix_declared_requirements_context.sql"
            )
            .as_slice(),
        ),
        (
            106,
            include_bytes!(
                "../../../postgres/migrations/0106_matrix_task_requirements_binding.sql"
            )
            .as_slice(),
        ),
        (
            107,
            include_bytes!("../../../postgres/migrations/0107_context_matrix_verification.sql")
                .as_slice(),
        ),
        (
            108,
            include_bytes!(
                "../../../postgres/migrations/0108_matrix_v1_dispatch_cutover_allowlist.sql"
            )
            .as_slice(),
        ),
        (
            109,
            include_bytes!(
                "../../../postgres/migrations/0109_matrix_planning_context_selection.sql"
            )
            .as_slice(),
        ),
        (
            110,
            include_bytes!(
                "../../../postgres/migrations/0110_pipeline_context_matrix_authority.sql"
            )
            .as_slice(),
        ),
        (
            111,
            include_bytes!("../../../postgres/migrations/0111_session_advisory_preference.sql")
                .as_slice(),
        ),
    ] {
        assert_eq!(
            ledger[(version - 1) as usize].2,
            Sha384::digest(bytes).to_vec(),
            "fixture migration {version} differs from reviewed source"
        );
    }
    let runtime = PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
        .await
        .unwrap();
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,(SELECT oid::bigint FROM pg_database WHERE datname=current_database())"
    ).fetch_one(&runtime).await.unwrap();
    assert_eq!(
        runtime_identity,
        (expected_database, "tect_ci".into(), expected_oid)
    );
    (pool, runtime_url)
}

fn policy(task: Uuid, matrix: &EngineeringMatrixInput) -> PipelineCompatibilityPolicy {
    let composition = compose_engineering_matrix(
        &VerifiedEngineeringMatrixFacts::bind_caller_verified_task_revision(
            task.to_string(),
            "1".into(),
            matrix.clone(),
        )
        .unwrap(),
    );
    let cards: Vec<_> = composition
        .mandatory_cards
        .iter()
        .map(|card| card.id.to_string())
        .collect();
    let definitions = tect_host::StaticPipelineRecommendationDefinitions;
    PipelineCompatibilityPolicy {
        version: PIPELINE_COMPATIBILITY_POLICY_VERSION.into(),
        task_id: task.to_string(),
        task_revision: "1".into(),
        catalogue_revision: "4".into(),
        // The selected synthetic work can either establish its design through
        // brainstorming or implement the bounded change with focused TDD.
        // Every other catalogue kind has no rule and is recorded as excluded.
        rules: [
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::DeepBrainstorming,
        ]
        .into_iter()
        .map(|kind| {
            let definition = definitions.definition("4", kind).unwrap().unwrap();
            let phase = definition
                .phases
                .iter()
                .find(|phase| {
                    phase.required
                        && (!phase.required_fields.is_empty()
                            || !phase.required_artifacts.is_empty()
                            || !phase.validator_contracts.is_empty()
                            || !phase.output_constraints.is_empty()
                            || phase.fresh_reviewer_input)
                })
                .unwrap();
            let obligation = PipelineVerificationObligation {
                phase_id: phase.id.clone(),
                required_fields: phase.required_fields.clone(),
                required_artifacts: phase.required_artifacts.clone(),
                validator_contracts: phase.validator_contracts.clone(),
                output_constraints: phase.output_constraints.clone(),
                allowed_verdicts: phase.allowed_verdicts.clone(),
                verdict_routes: phase.verdict_routes.clone(),
                disposition_required: phase.disposition_required,
                required_dispositions: phase.required_dispositions.clone(),
                fresh_reviewer_input: phase.fresh_reviewer_input,
                output_contract: phase.output_contract.clone(),
            };
            let digest = pipeline_obligation_digest(&obligation).unwrap();
            PipelineCompatibilityRule {
                kind,
                matrix_input_digest: matrix_input_digest(matrix).unwrap(),
                allowed_modes: vec![EngineeringMode::Demo],
                selected_candidate_ids: vec!["b".into()],
                card_coverage: cards
                    .iter()
                    .map(|card_id| PipelineCardCoverage {
                        card_id: card_id.clone(),
                        phase_id: phase.id.clone(),
                        obligation_digest: digest.clone(),
                    })
                    .collect(),
            }
        })
        .collect(),
    }
}

fn save_request(planning: &Value, selection: Value) -> Value {
    let mut request = json!({
        "kind":"draft","scope_id":planning["scope"]["id"],
        "candidate_set_id":planning["candidate_set"]["id"],
        "revision":planning["candidate_set"]["revision"],
        "snapshot_id":planning["snapshot"]["id"],
        "input_cursor":planning["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),
        "draft":{"coverage_summary":"Selected Matrix choice b bounds this synthetic work",
            "nodes":[{"kind":"work","identity":{"local":"choice-b"},
                "title":"Develop selected approach b",
                "outcome":"The selected approach has a documented result",
                "includes":["selected approach b"],"excludes":["deployment"],
                "dependencies":[],"proof":["Synthetic result is recorded"],
                "pipeline":"slice.lightweight-tdd-development",
                "pipeline_reason":"A bounded implementation with focused proof is the deterministic route; unresolved design can use deep brainstorming",
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

fn native_response(body: &[u8], eligible: &[String]) -> Vec<u8> {
    let request: Value = serde_json::from_slice(body).unwrap();
    let mut answers = serde_json::Map::new();
    for (index, _) in eligible.iter().enumerate() {
        let legend = request["questions"][format!("score_v1_{index}")]["criteria"]
            .as_array()
            .unwrap()
            .iter()
            .enumerate()
            .map(|(level, label)| (level.to_string(), label.clone()))
            .collect::<serde_json::Map<_, _>>();
        let score = 9 - index;
        let probabilities = (0..10)
            .map(|level| {
                (
                    level.to_string(),
                    json!(if level == score { 1.0 } else { 0.0 }),
                )
            })
            .collect::<serde_json::Map<_, _>>();
        answers.insert(
            format!("score_v1_{index}"),
            json!({"type":"score","score":score,
            "legend":legend,"probabilities":probabilities,"confidence":0.91}),
        );
    }
    let mut probabilities = eligible
        .iter()
        .enumerate()
        .map(|(index, id)| (id.clone(), json!(if index == 0 { 0.8 } else { 0.0 })))
        .collect::<serde_json::Map<_, _>>();
    probabilities.insert("ABSTAIN".into(), json!(0.2));
    answers.insert(
        "choice_v1".into(),
        json!({"type":"choice","choice":eligible[0],
        "probabilities":probabilities,"confidence":0.8}),
    );
    serde_json::to_vec(&json!({"model":"jev-1.13.0","answers":answers,
        "usage":{"input_tokens":20,"output_tokens":30}}))
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only exact owned PostgreSQL 18.6 fixture at migration 111; synthetic loopback provider"]
async fn public_s03_v4_context_bound_pipeline_happy_path() {
    let (pool, runtime_url) = exact_fixture().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("s03-v4.sock");
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("s03-v4-{}", Uuid::new_v4());
    let bootstrap = PgStore::connect(&runtime_url, 4).await.unwrap();
    let (workspace, owner_keys) =
        signed_fixture_budget(&bootstrap, &enrolled, &workspace_key).await;
    let calls = Arc::new(AtomicUsize::new(0));
    let budget_calls = Arc::new(AtomicUsize::new(0));
    let matrix_service = Arc::new(
        WorkspaceService::new(
            Arc::new(bootstrap.with_budget_owner_keys(owner_keys.clone())),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(calls.clone())),
            Arc::new(Budget(budget_calls)),
        ),
    );
    let unix = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let matrix_server = tokio::spawn(tect_host::serve(unix, matrix_service));
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let mut owner = Mcp::start(
        &socket,
        &owner_host,
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
    let confirmed = propose_confirm(&mut owner, program, 0, declarations("demo")).await;
    assert_eq!(confirmed["confirmation"]["proposal_revision"], 1);
    let effective = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record(&mut owner, task, Uuid::new_v4(), Some(locator(program))).await;
    let snapshot = recorded["requirements_snapshot_id"].clone();
    let semantic = recorded["requirements_semantic_digest"].clone();
    assert_eq!(semantic, effective["semantic_digest"]);
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
        "expected_revision":0,"mode":"optional","provider_profile_ref":{"id":PROFILE},
        "model_configuration":{"model":MODEL}}),
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
    let matrix: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(effective).unwrap();
    let facts = required_matrix_operating_facts(&context, &matrix).unwrap();
    assert!(!facts.is_empty());
    let evidence: Vec<_> = facts.iter().map(|fact| json!({
        "fact_path":fact.path,"evidence_ref":format!("urn:fixture:s03:operating:{}",fact.path)
    })).collect();
    let verified = route(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({
        "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],
        "evidence":evidence}),
    )
    .await;
    assert_eq!(verified["schema"], "tect.context-matrix-verification/1");
    assert_eq!(verified["frozen_snapshot_id"], snapshot);
    assert_eq!(verified["requirements_semantic_digest"], semantic);
    let advice_key = format!("s03-v4-matrix-{}", Uuid::new_v4());
    let advised = advice(&mut owner, task, &advice_key).await;
    assert_eq!(advised["state"], "advised", "{advised}");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({"task_id":task,"request_key":advice_key}),
    )
    .await;
    assert!(
        current["current_advice"]["advice_id"].as_str().is_some(),
        "{current}"
    );
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
    let opened_scope = route(
        &mut owner,
        "command",
        "scope.open",
        json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":source["candidate_set"]["id"],
        "candidate_set_revision":source["candidate_set"]["revision"],
        "candidate_snapshot_id":source["snapshot"]["id"],
        "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let save = save_request(&opened_scope["created"]["planning"], selection);
    let saved = route(&mut owner, "command", "slice.candidates.save", save.clone()).await;
    let set = id(&saved["candidate_set"]["id"]);
    let caller_request = id(&save["request_id"]);
    let work = saved["draft"]["nodes"][0].clone();
    let effect = route(
        &mut independent,
        "query",
        "engineering.matrix.planning_effect.get",
        json!({"candidate_set_id":set,"caller_request_id":caller_request}),
    )
    .await;
    assert_eq!(effect["material"]["selected_choice"]["candidate_id"], "b");
    assert_eq!(effect["material"]["nodes"][0]["node_id"], work["id"]);
    let matched = route(
        &mut independent,
        "command",
        "engineering.matrix.planning_effect.verify",
        json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller_request,
        "expected_result_revision":effect["material"]["result_revision"],
        "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
        "summary":"Synthetic saved node implements selected choice b."}),
    )
    .await;
    assert_eq!(matched["verdict"], "matches");
    let ready = support::review(&mut owner, &saved).await;
    assert_eq!(ready["candidate_set"]["status"], "ready");
    let matrix_effect_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2")
        .bind(workspace).bind(id(&matched["request_id"])).fetch_one(&pool).await.unwrap();

    // The second public host has only a loopback JEV adapter; no external credential or endpoint exists.
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = Url::parse(&format!(
        "http://{}/v1/systemone",
        listener.local_addr().unwrap()
    ))
    .unwrap();
    let provider = JevPipelineProvider::new(
        JevPipelineConfig {
            identity: PipelineProviderIdentity {
                provider: "fixture-systemone".into(),
                model: "jev-1.13.0".into(),
                destination: endpoint.as_str().into(),
                wire_version: WIRE_VERSION.into(),
            },
            endpoint,
            timeout: Duration::from_secs(2),
            maximum_request_bytes: MAX_REQUEST_BYTES,
            maximum_response_bytes: MAX_RESPONSE_BYTES,
        },
        "fixture-secret".into(),
    )
    .unwrap();
    let pipeline_socket = root.join("s03-v4-pipeline.sock");
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
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        ))
        .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(policy(
            task, &matrix,
        ))))
        .with_pipeline_recommendation_provider(Arc::new(provider)),
    );
    let unix = UnixListener::bind(&pipeline_socket).unwrap();
    std::fs::set_permissions(&pipeline_socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let pipeline_server = tokio::spawn(tect_host::serve(unix, pipeline_service));
    owner.finish().await;
    independent.finish().await;
    let pipeline_native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(
        &pipeline_socket,
        &owner_host,
        &pipeline_native,
        &workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    let mut independent = Mcp::start(
        &pipeline_socket,
        &verifier_host,
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
        "expected_revision":1,"mode":"optional","provider_profile_ref":{"id":"fixture-systemone"},
        "model_configuration":{"model":"jev-1.13.0"}}),
    )
    .await;
    let prepare_request = json!({
        "candidate_set_id":set,"expected_candidate_set_revision":ready["candidate_set"]["revision"],
        "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
        "request_key":format!("s03-v4-pipeline-{}",Uuid::new_v4())});
    let prepared = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        prepare_request.clone(),
    )
    .await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "pipeline.recommendation.prepare",
            json!({"candidate_set_id":set,
            "expected_candidate_set_revision":ready["candidate_set"]["revision"],
            "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
            "request_key":format!("forged-{}",Uuid::new_v4()),
            "session_preference":"skip"})
        )
        .await["error"]["code"],
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
    let replay = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        prepare_request.clone(),
    )
    .await;
    assert_eq!(replay["opportunity_id"], prepared["opportunity_id"]);
    let mut other_owner_session = Mcp::start(
        &pipeline_socket,
        &owner_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    other_owner_session.call("open_workspace", json!({})).await;
    assert_eq!(
        route_error(
            &mut other_owner_session,
            "command",
            "pipeline.recommendation.prepare",
            prepare_request.clone()
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let mut skipped_request = prepare_request.clone();
    skipped_request["request_key"] = json!(format!("s03-session-skip-{}", Uuid::new_v4()));
    let skipped = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        skipped_request,
    )
    .await;
    assert_eq!(
        (skipped["state"].as_str(), skipped["reason"].as_str()),
        (Some("no_call"), Some("session_skip"))
    );
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":1,"preference":"use_workspace"}),
    )
    .await;
    let mut pending_request = prepare_request.clone();
    pending_request["request_key"] = json!(format!("s03-skip-before-send-{}", Uuid::new_v4()));
    let pending = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        pending_request.clone(),
    )
    .await;
    assert_eq!(pending["state"], "prepared");
    assert_eq!(
        route_error(
            &mut other_owner_session,
            "command",
            "pipeline.recommendation.run",
            json!({"opportunity_id":pending["opportunity_id"]})
        )
        .await["error"]["code"],
        "forbidden"
    );
    other_owner_session.finish().await;
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":2,"preference":"skip"}),
    )
    .await;
    let no_send = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        json!({"opportunity_id":pending["opportunity_id"]}),
    )
    .await;
    assert_eq!(
        (no_send["status"].as_str(), no_send["reason"].as_str()),
        (Some("no_call"), Some("session_skip"))
    );
    let pending_id = id(&pending["opportunity_id"]);
    let skip_audit: (String, String, String, i64) = sqlx::query_as(
        "SELECT state,primary_reason,session_preference,\
         (SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2)\
         FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(pending_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        skip_audit,
        (
            "no_call".into(),
            "session_skip".into(),
            "use_workspace".into(),
            0
        )
    );
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":3,"preference":"use_workspace"}),
    )
    .await;
    assert_eq!(
        route_error(
            &mut owner,
            "command",
            "pipeline.recommendation.prepare",
            json!({"candidate_set_id":set,
            "expected_candidate_set_revision":ready["candidate_set"]["revision"],
            "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
            "request_key":pending_request["request_key"],
            "request_preference":"skip"})
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let opportunity = id(&prepared["opportunity_id"]);
    let row: (Value, Option<Uuid>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT manifest_payload,frozen_snapshot_id,requirements_semantic_digest,authority_schema,\
         operating_verification_digest FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2")
        .bind(workspace).bind(opportunity).fetch_one(&pool).await.unwrap();
    let manifest: PipelineRecommendationManifest = serde_json::from_value(row.0.clone()).unwrap();
    assert_eq!(row.0["schema"], "tect.pipeline-recommendation/4");
    assert_eq!(row.1, Some(id(&snapshot)));
    assert_eq!(row.2.as_deref(), semantic.as_str());
    assert_eq!(row.3.as_deref(), Some("tect.matrix-requirements/1"));
    assert_eq!(row.4.as_deref(), verified["verification_digest"].as_str());
    assert_eq!(row.0["matrix_authority"]["frozen_snapshot_id"], snapshot);
    assert_eq!(
        row.0["matrix_authority"]["requirements_semantic_digest"],
        semantic
    );
    assert_eq!(
        row.0["matrix_authority"]["operating_verification_digest"],
        verified["verification_digest"]
    );
    assert_eq!(row.0["matrix_input_digest"], recorded["input_digest"]);
    let frozen = prepare_native_request("jev-1.13.0", &manifest, MAX_REQUEST_BYTES).unwrap();
    let response = native_response(&frozen.body, &frozen.eligible_ids);
    let audit_pool = pool.clone();
    let preference_socket = pipeline_socket.clone();
    let preference_host = owner_host.clone();
    let preference_native = pipeline_native.clone();
    let preference_workspace = workspace_key.clone();
    let stub = tokio::spawn(async move {
        let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
            .await
            .unwrap()
            .unwrap();
        let committed: (String, i64) = sqlx::query_as(
            "SELECT state,(SELECT count(*) FROM advisory_budget_reservations WHERE dispatch_id=d.id) \
             FROM advisory_dispatch d WHERE workspace_id=$1 AND opportunity_id=$2")
            .bind(workspace).bind(opportunity).fetch_one(&audit_pool).await.unwrap();
        assert_eq!(committed, ("sending".into(), 1));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let head_end = loop {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..head_end]).into_owned();
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < head_end + length {
            let read = stream.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
        assert!(
            headers
                .lines()
                .any(|line| line.eq_ignore_ascii_case("authorization: bearer fixture-secret"))
        );
        assert_eq!(&request[head_end..], frozen.body);
        let mut same_session = Mcp::start(
            &preference_socket,
            &preference_host,
            &preference_native,
            &preference_workspace,
        )
        .await;
        same_session.call("open_workspace", json!({})).await;
        let changed = route(
            &mut same_session,
            "command",
            "session.advisory.preference.set",
            json!({"expected_revision":4,"preference":"skip"}),
        )
        .await;
        assert_eq!(changed["preference"], "skip");
        same_session.finish().await;
        let head = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response.len()
        );
        stream.write_all(head.as_bytes()).await.unwrap();
        stream.write_all(&response).await.unwrap();
        (listener, response)
    });
    let run_request = json!({"opportunity_id":opportunity});
    let ranked = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        run_request.clone(),
    )
    .await;
    assert_eq!(ranked["status"], "ranked", "{ranked}");
    let (listener, response) = stub.await.unwrap();
    let replay = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        run_request,
    )
    .await;
    assert_eq!(replay, ranked);
    route(
        &mut owner,
        "command",
        "session.advisory.preference.set",
        json!({"expected_revision":5,"preference":"use_workspace"}),
    )
    .await;
    assert!(
        tokio::time::timeout(Duration::from_millis(100), listener.accept())
            .await
            .is_err(),
        "replay must not send a second provider request"
    );
    let dispatch_id = id(&ranked["dispatch_id"]);
    let dispatch: (String, Vec<u8>, String) = sqlx::query_as(
        "SELECT state,response_payload,pipeline_response_sha256 FROM advisory_dispatch WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(dispatch_id).fetch_one(&pool).await.unwrap();
    assert_eq!(dispatch.0, "sealed");
    assert_eq!(dispatch.1, response);
    assert_eq!(dispatch.2, format!("{:x}", Sha256::digest(&response)));
    let counts: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2),\
         (SELECT count(*) FROM advisory_budget_reservations WHERE dispatch_id=$3),\
         (SELECT count(*) FROM advisory_budget_consumptions WHERE dispatch_id=$3)")
        .bind(workspace).bind(opportunity).bind(dispatch_id).fetch_one(&pool).await.unwrap();
    assert_eq!(counts, (1, 1, 1));
    let no_effect: (i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM native_slices WHERE workspace_id=$1),\
         (SELECT count(*) FROM slice_pipeline_runs WHERE workspace_id=$1)",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        no_effect,
        (0, 0),
        "provider ranking must not create caller effects"
    );
    let disposition = route(&mut owner, "command", "pipeline.recommendation.disposition", json!({
        "request_id":Uuid::new_v4(),"opportunity_id":opportunity,
        "expected_work_revision":work["revision"],"manifest_digest":prepared["manifest_digest"],
        "action":"accept_recommendation","rationale":"Caller explicitly accepts synthetic JEV advice."})).await;
    assert_eq!(disposition["selected_option_id"], ranked["ranked_ids"][0]);
    let slice_open_request = json!({
        "request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
        "scope_revision":ready["scope"]["revision"],
        "candidate_set_id":set,"candidate_set_revision":ready["candidate_set"]["revision"],
        "candidate_snapshot_id":ready["snapshot"]["id"],"candidate_id":work["id"],
        "candidate_revision":work["revision"],"disposition_id":disposition["id"]});
    let opened = route(
        &mut owner,
        "command",
        "slice.open",
        slice_open_request.clone(),
    )
    .await;
    assert_eq!(
        opened["created"]["selected_option_id"],
        disposition["selected_option_id"]
    );
    let slice_id = id(&opened["created"]["id"]);
    let begin = json!({
        "request_id":Uuid::new_v4(),"scope_id":ready["scope"]["id"],
        "slice_id":slice_id,"slice_revision":opened["created"]["revision"],
        "definition_version":opened["created"]["verification_plan_source_definition_version"],
        "qualification_reason":"Caller explicitly begins selected synthetic pipeline."});
    let begun = route(&mut owner, "command", "slice.pipeline.begin", begin.clone()).await;
    let pipeline_run = &begun["created"]["run"];
    assert_eq!(
        pipeline_run["selected_option_id"],
        disposition["selected_option_id"]
    );
    let stored: (String, String) = sqlx::query_as(
        "SELECT selected_option_id,verification_plan_digest FROM slice_pipeline_runs WHERE workspace_id=$1 AND id=$2")
        .bind(workspace).bind(id(&pipeline_run["id"])).fetch_one(&pool).await.unwrap();
    assert_eq!(
        stored.0,
        pipeline_run["selected_option_id"].as_str().unwrap()
    );
    assert_eq!(
        stored.1,
        pipeline_run["verification_plan_digest"].as_str().unwrap()
    );
    let open_effect = route(
        &mut independent,
        "query",
        "pipeline.open_effect.get",
        json!({
        "slice_id":slice_id,"open_request_id":slice_open_request["request_id"]}),
    )
    .await;
    assert_eq!(open_effect["material"]["slice"], opened["created"]);
    assert_eq!(
        open_effect["material"]["disposition"]["id"],
        disposition["id"]
    );
    assert_eq!(
        open_effect["material"]["matrix_effect_attestation_id"],
        json!(matrix_effect_id)
    );
    assert_ne!(
        open_effect["verifier_principal_id"],
        open_effect["material"]["caller_principal_id"]
    );
    assert_eq!(
        route_error(
            &mut owner,
            "query",
            "pipeline.open_effect.get",
            json!({
        "slice_id":slice_id,"open_request_id":slice_open_request["request_id"]})
        )
        .await["error"]["code"],
        "forbidden"
    );
    let attested = route(
        &mut independent,
        "command",
        "pipeline.open_effect.verify",
        json!({
        "request_id":Uuid::new_v4(),"slice_id":slice_id,
        "open_request_id":slice_open_request["request_id"],
        "expected_effect_digest":open_effect["effect_digest"],"verdict":"matches",
        "summary":"Independent synthetic verifier read the exact saved opening effect."}),
    )
    .await;
    assert_eq!(attested["verdict"], "matches");
    let attestation_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pipeline_open_effect_attestations WHERE workspace_id=$1 AND slice_id=$2")
        .bind(workspace).bind(slice_id).fetch_one(&pool).await.unwrap();
    assert_eq!(attestation_count, 1);
    owner.finish().await;
    independent.finish().await;
    pipeline_server.abort();
    matrix_server.abort();
}
