//! Test-only one-shot S03 fixture. All source, Matrix, policy, and verifier data are synthetic.
//! `preflight` never constructs a credential-bearing provider or runs a dispatch.
use super::*;
use std::{fs, io::Write, sync::Arc};
use tect_domain::PipelineRecommendationManifest;
use tect_host::jev_pipeline_recommendation::JevPipelineSavedResponseParser;

#[path = "s03_live/audit.rs"]
pub(crate) mod audit;
#[path = "s03_live/budget.rs"]
pub(crate) mod budget;
#[path = "s03_live/effect.rs"]
pub(crate) mod effect;
#[path = "s03_live/guard.rs"]
mod guard;
#[path = "s03_live/phase.rs"]
mod phase;
#[path = "s03_live/runtime.rs"]
mod runtime;
#[path = "s03_live/send.rs"]
mod send;
use runtime::{identity, pipeline_service, start_server};

const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const REAL_MODEL: &str = "jev-1.13.0";

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit JEV_PIPELINE_ONE_SHOT_MODE=preflight or send; fresh isolated PostgreSQL 18 only"]
async fn one_shot_real_s03_pipeline() {
    let mode =
        std::env::var("JEV_PIPELINE_ONE_SHOT_MODE").expect("set preflight or send explicitly");
    run_fixture(&mode, 2).await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "one-call signed policy negative on a fresh isolated PostgreSQL 18 database"]
async fn one_call_budget_rejected_before_marker() {
    run_fixture("budget_negative", 1).await;
}

async fn run_fixture(mode: &str, policy_calls: i64) {
    assert!(matches!(mode, "preflight" | "send" | "budget_negative"));
    let profile =
        std::env::var("JEV_PIPELINE_PROFILE_ID").expect("explicit local profile ID required");
    assert!(
        !profile.is_empty()
            && profile.len() <= 128
            && profile
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    );
    let artifacts = if mode == "send" {
        let (request, marker) = send::artifact_paths();
        assert!(
            !request.exists() && !marker.exists(),
            "one-use artifacts already exist"
        );
        Some((request, marker))
    } else {
        None
    };
    let (pool, runtime_url) = guard::fresh_database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("jev-pipeline-one-shot-{}", Uuid::new_v4());
    let bootstrap = PgStore::connect(&runtime_url, 4).await.unwrap();
    // This agent-authored live-test policy covers one synthetic Matrix call and
    // one Pipeline call; the observed HTTP 200 used 14,114 input / 287 output
    // tokens, so 24k / 2k leaves margin without predicting future token usage.
    let (workspace, keys) = signed_fixture_budget_with_token_ceilings(
        &bootstrap,
        &enrolled,
        &workspace_key,
        policy_calls,
        24_000,
        2_000,
    )
    .await;
    let trusted_budget_store = bootstrap.clone().with_budget_owner_keys(keys.clone());
    let now_ms = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let mut policy_tx = bootstrap
        .clone()
        .with_budget_owner_keys(keys.clone())
        .begin(TransactionMode::ReadOnly)
        .await
        .unwrap();
    policy_tx.authenticate(&enrolled.auth).await.unwrap();
    policy_tx.set_tenant(enrolled.tenant_id).await.unwrap();
    let authorized_policy = policy_tx
        .advisory_budget_policy_store()
        .unwrap()
        .authorized_budget_policy(workspace, now_ms)
        .await
        .unwrap()
        .expect("signed owner budget must verify with runtime trust key");
    assert_eq!(authorized_policy.ceilings().provider_calls, policy_calls);
    assert_eq!(authorized_policy.ceilings().retry_dispatches, 1);
    policy_tx.commit().await.unwrap();
    let limits: (i64, i64, i64) = sqlx::query_as(
        "SELECT provider_calls,retry_dispatches,request_utf8_bytes FROM advisory_budget_policies WHERE workspace_id=$1",
    ).bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(limits.0, policy_calls);
    assert_eq!(limits.1, 1); // Schema floor; this harness never calls a retry route.
    assert!(limits.2 >= MAX_REQUEST_BYTES as i64);
    let calls = Arc::new(AtomicUsize::new(0));
    let budget_calls = Arc::new(AtomicUsize::new(0));
    let matrix_service = Arc::new(
        WorkspaceService::new(
            Arc::new(bootstrap.with_budget_owner_keys(keys.clone())),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(calls.clone())),
            Arc::new(Budget(budget_calls.clone())),
        ),
    );
    let matrix_socket = root.join("matrix.sock");
    let matrix_server = start_server(&matrix_socket, matrix_service).await;
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let mut owner = Mcp::start(
        &matrix_socket,
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
    assert_ne!(verifier.principal_id, enrolled.principal_id);
    let verifier_host = root.join("verifier.json");
    host_file(&verifier_host, &verifier.auth);
    let mut independent = Mcp::start(
        &matrix_socket,
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
        "fact_path":fact.path,"evidence_ref":format!("urn:fixture:jev-pipeline:{}",fact.path)
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
    assert_eq!(verified["frozen_snapshot_id"], snapshot);
    assert_eq!(verified["requirements_semantic_digest"], semantic);
    let advice_key = format!("matrix-{}", Uuid::new_v4());
    let advised = advice(&mut owner, task, &advice_key).await;
    assert_eq!(advised["state"], "advised");
    assert_eq!(calls.load(Ordering::SeqCst), 1);
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":advice_key
        }),
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
    let opened_scope = route(
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
    let matched = route(
        &mut independent,
        "command",
        "engineering.matrix.planning_effect.verify",
        json!({
            "request_id":Uuid::new_v4(),"candidate_set_id":set,"caller_request_id":caller_request,
            "expected_result_revision":effect["material"]["result_revision"],
            "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
            "summary":"Synthetic saved node matches selected choice b."
        }),
    )
    .await;
    assert_eq!(matched["verdict"], "matches");
    let ready = support::review(&mut owner, &saved).await;
    assert_eq!(ready["candidate_set"]["status"], "ready");
    owner.finish().await;
    independent.finish().await;
    matrix_server.abort();

    let pipeline_socket = root.join("pipeline.sock");
    let no_send_provider =
        Arc::new(JevPipelineSavedResponseParser::new(identity(&profile)).unwrap());
    let prepared_service =
        pipeline_service(&runtime_url, keys.clone(), task, &matrix, no_send_provider).await;
    let prepare_server = start_server(&pipeline_socket, prepared_service).await;
    let pipeline_native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(
        &pipeline_socket,
        &owner_host,
        &pipeline_native,
        &workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    let config = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":1,"mode":"optional",
            "provider_profile_ref":{"id":profile},"model_configuration":{"model":REAL_MODEL}
        }),
    )
    .await;
    assert_eq!(config["provider_profile_ref"]["id"], profile);
    let prepared = route(&mut owner, "command", "pipeline.recommendation.prepare", json!({
        "candidate_set_id":set,"expected_candidate_set_revision":ready["candidate_set"]["revision"],
        "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
        "request_key":format!("pipeline-{}",Uuid::new_v4())
    })).await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    let opportunity = id(&prepared["opportunity_id"]);
    let captured_native: String = sqlx::query_scalar(
        "SELECT s.native_session_id FROM advisory_opportunity o \
         JOIN agent_sessions s ON s.tenant_id=o.tenant_id AND s.id=o.session_id \
         WHERE o.workspace_id=$1 AND o.id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(captured_native, pipeline_native);
    let row: (Value, Option<Uuid>, Option<String>, Option<String>, Option<String>) = sqlx::query_as(
        "SELECT manifest_payload,frozen_snapshot_id,requirements_semantic_digest,authority_schema,\
         operating_verification_digest FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    ).bind(workspace).bind(opportunity).fetch_one(&pool).await.unwrap();
    let manifest: PipelineRecommendationManifest = serde_json::from_value(row.0.clone()).unwrap();
    manifest.validate_digest().unwrap();
    assert!(manifest.has_bound_v2_authority() && manifest.should_call());
    assert_eq!(
        manifest
            .options
            .iter()
            .map(|option| option.kind)
            .collect::<Vec<_>>(),
        vec![
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::DeepBrainstorming,
        ]
    );
    assert_eq!(manifest.excluded.len(), 6);
    assert!(manifest.excluded.iter().all(|excluded| {
        !matches!(
            excluded.kind,
            PipelineKind::LightweightTddDevelopment | PipelineKind::DeepBrainstorming
        ) && excluded.reason == PipelineExclusionReason::MissingRule
    }));
    assert_eq!(
        manifest.deterministic_kind,
        PipelineKind::LightweightTddDevelopment
    );
    assert_eq!(
        manifest.deterministic_option_id.as_deref(),
        Some(manifest.options[0].id.as_str())
    );
    assert_eq!(row.1, Some(id(&snapshot)));
    assert_eq!(row.2.as_deref(), semantic.as_str());
    assert_eq!(row.3.as_deref(), Some("tect.matrix-requirements/1"));
    assert_eq!(row.4.as_deref(), verified["verification_digest"].as_str());
    assert_eq!(
        manifest.matrix_input_digest,
        recorded["input_digest"].as_str().unwrap()
    );
    let expected_policy = policy(task, &matrix);
    assert_eq!(
        manifest.compatibility_policy_digest,
        expected_policy.digest().unwrap()
    );
    let wire = prepare_native_request(REAL_MODEL, &manifest, MAX_REQUEST_BYTES).unwrap();
    assert!(
        wire.body.len() < 45_000,
        "frozen request exceeds 45 KB: {}",
        wire.body.len()
    );
    assert_eq!(wire.manifest_digest, manifest.digest);
    assert_eq!(wire.eligible_ids.len(), manifest.options.len());
    assert_eq!(
        serde_json::from_slice::<Value>(&wire.body).unwrap()["state"]["manifest"],
        row.0
    );
    let digest = format!("{:x}", Sha256::digest(&wire.body));
    let dispatch_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(dispatch_count, 0);
    let headroom = budget::remaining_for_pipeline(
        &pool,
        &trusted_budget_store,
        &enrolled,
        workspace,
        task,
        opportunity,
        &authorized_policy,
        wire.body.len(),
        10_000,
    )
    .await;
    if mode == "budget_negative" {
        let marker = root.join("one-call-negative.used");
        assert!(send::confirmation_matches(
            &mut std::io::Cursor::new(format!("SEND JEV PIPELINE {digest}\n")),
            &digest
        ));
        assert!(
            headroom.is_none(),
            "one-call policy must reject before marker"
        );
        assert!(!marker.exists());
        assert_eq!(dispatch_count, 0);
        println!(
            "negative_budget workspace={workspace} opportunity={opportunity} remaining_calls=0 marker_absent=true pipeline_dispatches=0"
        );
        owner.finish().await;
        prepare_server.abort();
        return;
    }
    let headroom = headroom.expect("Matrix consumed budget or Pipeline has no safe headroom");
    assert_eq!(headroom.calls, 1);
    println!(
        "preflight call_id={} workspace={} task={} opportunity={} profile={} manifest_digest={} request_bytes={} request_sha256={} remaining_calls={} remaining_bytes={} remaining_input_tokens={} remaining_output_tokens={} remaining_elapsed_ms={} dispatches=0",
        send::CALL_ID,
        workspace,
        task,
        opportunity,
        profile,
        manifest.digest,
        wire.body.len(),
        digest,
        headroom.calls,
        headroom.request_bytes,
        headroom.input_tokens,
        headroom.output_tokens,
        headroom.elapsed_ms,
    );
    if mode == "preflight" {
        let mut resumed = Mcp::start(
            &pipeline_socket,
            &owner_host,
            &pipeline_native,
            &workspace_key,
        )
        .await;
        resumed.call("open_workspace", json!({})).await;
        let resumed_session: Uuid = sqlx::query_scalar(
            "SELECT id FROM agent_sessions WHERE tenant_id=$1 AND workspace_id=$2 \
             AND native_session_id=$3 AND revoked=false",
        )
        .bind(enrolled.tenant_id)
        .bind(workspace)
        .bind(&pipeline_native)
        .fetch_one(&pool)
        .await
        .unwrap();
        let captured_session: Uuid = sqlx::query_scalar(
            "SELECT session_id FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
        )
        .bind(workspace)
        .bind(opportunity)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(resumed_session, captured_session);
        resumed.finish().await;
    }
    owner.finish().await;
    prepare_server.abort();
    if mode == "preflight" {
        return;
    }

    let (request_path, marker_path) = artifacts.unwrap();
    send::review_request(&request_path, &wire.body);
    let reviewed = Arc::new(fs::read(&request_path).unwrap());
    assert_eq!(format!("{:x}", Sha256::digest(reviewed.as_slice())), digest);
    // Credential is read only after every preflight assertion and exact request freeze.
    let key = std::env::var("TYPESAFE_API_KEY").expect("process-level API key required");
    assert!(!key.trim().is_empty());
    let provider = JevPipelineProvider::new(
        JevPipelineConfig {
            identity: identity(&profile),
            endpoint: Url::parse(ENDPOINT).unwrap(),
            timeout: Duration::from_secs(10),
            maximum_request_bytes: MAX_REQUEST_BYTES,
            maximum_response_bytes: MAX_RESPONSE_BYTES,
        },
        key,
    )
    .expect("provider config invalid");
    let service = pipeline_service(
        &runtime_url,
        keys,
        task,
        &matrix,
        Arc::new(send::ReviewedProvider {
            inner: provider,
            reviewed,
        }),
    )
    .await;
    println!(
        "review exact request at {} ({} bytes, sha256={})",
        request_path.display(),
        wire.body.len(),
        digest
    );
    println!("to authorize this one call, enter exactly: SEND JEV PIPELINE {digest}");
    std::io::stdout().flush().unwrap();
    assert!(
        send::confirmation_matches(&mut std::io::stdin().lock(), &digest),
        "confirmation absent or mismatched; no provider call"
    );
    let final_headroom = budget::remaining_for_pipeline(
        &pool,
        &trusted_budget_store,
        &enrolled,
        workspace,
        task,
        opportunity,
        &authorized_policy,
        wire.body.len(),
        10_000,
    )
    .await
    .expect("signed policy lacks durable headroom; marker not created");
    assert_eq!(final_headroom.calls, 1);
    assert!(
        final_headroom.request_bytes >= i64::try_from(wire.body.len()).unwrap(),
        "exact frozen request does not fit the remaining byte budget; marker not created"
    );
    assert!(
        final_headroom.input_tokens > 0 && final_headroom.output_tokens > 0,
        "signed token headroom is exhausted; marker not created"
    );
    let live_socket = root.join("pipeline-live.sock");
    let live_server = start_server(&live_socket, service).await;
    let mut owner = Mcp::start(&live_socket, &owner_host, &pipeline_native, &workspace_key).await;
    owner.call("open_workspace", json!({})).await;
    let reopened_session: Uuid = sqlx::query_scalar(
        "SELECT id FROM agent_sessions WHERE tenant_id=$1 AND workspace_id=$2 \
         AND native_session_id=$3 AND revoked=false",
    )
    .bind(enrolled.tenant_id)
    .bind(workspace)
    .bind(&pipeline_native)
    .fetch_one(&pool)
    .await
    .unwrap();
    let captured_session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(opportunity)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        reopened_session, captured_session,
        "send must use capture session"
    );
    send::mark_one_use(&marker_path, &digest);
    let outcome = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        json!({"opportunity_id":opportunity}),
    )
    .await;
    println!(
        "live observed opportunity={} status={}",
        opportunity, outcome["status"]
    );
    let ranked = audit::readback(
        &pool,
        workspace,
        opportunity,
        &outcome,
        &request_path,
        &marker_path,
        &digest,
        &manifest.digest,
        &profile,
        send::CALL_ID,
    )
    .await;
    assert!(
        ranked,
        "provider did not return ranked advice; retained audit records the observed outcome"
    );
    let matrix_effect_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2",
    )
    .bind(workspace)
    .bind(id(&matched["request_id"]))
    .fetch_one(&pool)
    .await
    .unwrap();
    effect::verify_ranked_caller_effect(
        &pool,
        workspace,
        &mut owner,
        &live_socket,
        &verifier_host,
        &workspace_key,
        &ready,
        &work,
        &prepared,
        &manifest,
        &outcome,
        matrix_effect_id,
        None,
    )
    .await;
    owner.finish().await;
    live_server.abort();
}
