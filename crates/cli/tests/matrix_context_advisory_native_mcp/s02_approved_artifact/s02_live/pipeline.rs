//! No-network S03 preparation from the same Owner-case V2 Matrix effect.
use super::*;
use std::{fs, io::Write, sync::Mutex, time::Duration};
use tect_domain::{EngineeringMode, PipelineRecommendationManifest};
use tect_host::jev_pipeline_recommendation::{
    JevPipelineConfig, JevPipelineProvider, MAX_REQUEST_BYTES, MAX_RESPONSE_BYTES,
    parse_native_response, prepare_native_request,
};

pub(super) struct OwnerCasePipeline<'a> {
    pub pool: &'a PgPool,
    pub runtime_url: &'a str,
    pub keys: BudgetOwnerKeys,
    pub approval: ApprovedMatrixEvidenceArtifact,
    pub workspace_key: &'a str,
    pub owner_host: &'a Path,
    pub root: &'a Path,
    pub task: Uuid,
    pub recorded: &'a Value,
    pub validated: &'a Value,
    pub disposition: &'a Value,
    pub matrix_effect: &'a Value,
    pub saved: &'a Value,
    pub ready: &'a Value,
    pub source_head: &'a str,
    pub mode: &'a str,
    pub workspace: Uuid,
    pub enrolled: &'a tect_postgres::admin::Enrollment,
    pub verifier_host: &'a Path,
}

pub(super) async fn prepare_owner_case(input: OwnerCasePipeline<'_>) {
    let matrix: EngineeringMatrixInput =
        serde_json::from_value(input.recorded["input"].clone()).unwrap();
    assert!(matches!(
        &matrix.mode,
        tect_domain::MatrixFact::Known {
            value: EngineeringMode::Mvp,
            ..
        }
    ));
    let mut policy = crate::s03_v4::policy_for_choice(
        input.task,
        &matrix,
        "matrix-local-evidence-first",
        &[
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::DeepBrainstorming,
        ],
    );
    for rule in &mut policy.rules {
        rule.allowed_modes = vec![EngineeringMode::Mvp];
    }
    let policy_digest = policy.digest().unwrap();
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(input.runtime_url).unwrap())
        .await
        .unwrap();
    let live_mode = input.mode != "synthetic_pipeline";
    let profile = if live_mode {
        let value = std::env::var("JEV_PIPELINE_PROFILE_ID")
            .expect("explicit isolated Pipeline profile required");
        assert!(!value.is_empty() && value.len() <= 128);
        assert!(
            value
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
        );
        value
    } else {
        "active-jev-s03-no-network".into()
    };
    let identity = PipelineProviderIdentity {
        provider: profile.clone(),
        model: MODEL.into(),
        destination: if live_mode {
            ENDPOINT
        } else {
            "https://synthetic.invalid/no-send"
        }
        .into(),
        wire_version: WIRE_VERSION.into(),
    };
    let provider = Arc::new(pipeline_live::SwitchedPipelineProvider {
        parser: JevPipelineSavedResponseParser::new(identity.clone()).unwrap(),
        live: Mutex::new(None),
    });
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(
                PgStore::connect(input.runtime_url, 4)
                    .await
                    .unwrap()
                    .with_budget_owner_keys(input.keys.clone()),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(
            runtime_pool,
            input.approval.clone(),
        )))
        .with_pipeline_recommendation_definitions(Arc::new(
            tect_host::StaticPipelineRecommendationDefinitions,
        ))
        .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(
            policy.clone(),
        )))
        .with_pipeline_recommendation_provider(provider.clone()),
    );
    let socket = input.root.join("owner-case-pipeline.sock");
    let server = start_server(&socket, service).await;
    let pipeline_native = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(
        &socket,
        input.owner_host,
        &pipeline_native,
        input.workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    owner.call("get_state", json!({})).await;
    let registered_source = route(
        &mut owner,
        "command",
        "source.register",
        json!({"path":input.root.join("source")}),
    )
    .await;
    let selected_sources = route(
        &mut owner,
        "command",
        "session.select_worktrees",
        json!({"worktree_ids":[registered_source["id"]]}),
    )
    .await;
    assert_eq!(
        selected_sources["selected_worktrees"][0]["id"],
        registered_source["id"]
    );
    if live_mode {
        let config = route(
            &mut owner,
            "command",
            "workspace.advisory.configure",
            json!({
                "expected_revision":3,"mode":"optional",
                "provider_profile_ref":{"id":profile},"model_configuration":{"model":MODEL}
            }),
        )
        .await;
        assert_eq!(config["provider_profile_ref"]["id"], profile);
    }
    let ready = input.ready;
    let work = &input.saved["draft"]["nodes"][0];
    let prepared = route(
        &mut owner,
        "command",
        "pipeline.recommendation.prepare",
        json!({
            "candidate_set_id":input.saved["candidate_set"]["id"],
            "expected_candidate_set_revision":ready["candidate_set"]["revision"],
            "work_node_id":work["id"],"expected_work_node_revision":work["revision"],
            "request_key":format!("owner-case-pipeline-{}",Uuid::new_v4())
        }),
    )
    .await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    let opportunity = id(&prepared["opportunity_id"]);
    let manifest_json: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE opportunity_id=$1",
    )
    .bind(opportunity)
    .fetch_one(input.pool)
    .await
    .unwrap();
    let manifest: PipelineRecommendationManifest = serde_json::from_value(manifest_json).unwrap();
    manifest.validate_digest().unwrap();
    assert!(manifest.has_bound_v2_authority() && manifest.should_call());
    assert_eq!(manifest.matrix_task_id, input.task.to_string());
    assert_eq!(manifest.matrix_task_revision, "1");
    assert_eq!(manifest.selected_choice_id, "matrix-local-evidence-first");
    assert_eq!(manifest.matrix_input_digest, input.recorded["input_digest"]);
    assert_eq!(
        manifest.matrix_choice_set_digest,
        input.recorded["choice_set_digest"]
    );
    assert_eq!(
        manifest.matrix_verification_digest,
        input.validated["verification_digest"]
    );
    assert_eq!(manifest.compatibility_policy_digest, policy_digest);
    assert_eq!(
        manifest.mandatory_card_ids,
        ["EM02-PROTECT@0.1", "EM02-SCOPE@0.1"]
    );
    assert_eq!(
        manifest
            .options
            .iter()
            .map(|option| option.kind)
            .collect::<Vec<_>>(),
        [
            PipelineKind::LightweightTddDevelopment,
            PipelineKind::DeepBrainstorming,
        ]
    );
    for option in &manifest.options {
        option.verification_plan.validate().unwrap();
        assert!(!option.verification_plan.obligations.is_empty());
        let rule = policy
            .rules
            .iter()
            .find(|rule| rule.kind == option.kind)
            .unwrap();
        let mut covered: Vec<_> = rule
            .card_coverage
            .iter()
            .map(|coverage| coverage.card_id.as_str())
            .collect();
        covered.sort_unstable();
        assert_eq!(covered, ["EM02-PROTECT@0.1", "EM02-SCOPE@0.1"]);
        for coverage in &rule.card_coverage {
            assert!(
                option
                    .verification_plan
                    .obligations
                    .iter()
                    .any(|obligation| obligation.phase_id == coverage.phase_id)
            );
        }
    }
    let refs: Vec<String> = sqlx::query_scalar(
        "SELECT DISTINCT b.evidence_ref FROM matrix_verification_bindings b \
         JOIN matrix_verifications v ON (v.tenant_id,v.workspace_id,v.id)= \
             (b.tenant_id,b.workspace_id,b.verification_id) \
         WHERE v.task_id=$1 AND v.task_revision=1 AND v.record_digest=$2 \
           AND v.schema='tect.context-matrix-verification/1' ORDER BY b.evidence_ref",
    )
    .bind(input.task)
    .bind(input.validated["verification_digest"].as_str().unwrap())
    .fetch_all(input.pool)
    .await
    .unwrap();
    assert_eq!(refs.len(), 1);
    assert!(refs[0].starts_with("pipeline-evidence:"));
    assert_eq!(manifest.evidence_refs, refs);
    let matrix_effect_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE verifier_request_id=$1",
    )
    .bind(id(&input.matrix_effect["request_id"]))
    .fetch_one(input.pool)
    .await
    .unwrap();
    assert_ne!(matrix_effect_id, Uuid::nil());
    assert_eq!(
        input.disposition["decision"]["selected_choice_id"],
        "matrix-local-evidence-first"
    );
    let dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE opportunity_id=$1")
            .bind(opportunity)
            .fetch_one(input.pool)
            .await
            .unwrap();
    assert_eq!(dispatches, 0, "preparation cannot send to JEV");
    println!(
        "s03_owner_case_prepared source_head={} task={} choice=matrix-local-evidence-first \
         matrix_effect={} manifest_digest={} evidence_refs={} options={} mandatory_cards=EM02-SCOPE@0.1,EM02-PROTECT@0.1 dispatches=0",
        input.source_head,
        input.task,
        matrix_effect_id,
        manifest.digest,
        manifest.evidence_refs.len(),
        manifest.options.len(),
    );
    if live_mode {
        run_guarded_pipeline(
            &input,
            &socket,
            &pipeline_native,
            &mut owner,
            &provider,
            &identity,
            &prepared,
            &manifest,
            &registered_source,
        )
        .await;
    }
    owner.finish().await;
    server.abort();
}

#[allow(clippy::too_many_arguments)]
async fn run_guarded_pipeline(
    input: &OwnerCasePipeline<'_>,
    socket: &Path,
    pipeline_native: &str,
    owner: &mut Mcp,
    provider: &Arc<pipeline_live::SwitchedPipelineProvider>,
    identity: &PipelineProviderIdentity,
    prepared: &Value,
    manifest: &PipelineRecommendationManifest,
    registered_source: &Value,
) {
    let opportunity = id(&prepared["opportunity_id"]);
    let wire = prepare_native_request(MODEL, manifest, MAX_REQUEST_BYTES).unwrap();
    assert_eq!(wire.manifest_digest, manifest.digest);
    let digest = format!("{:x}", Sha256::digest(&wire.body));
    let store = PgStore::connect(input.runtime_url, 4)
        .await
        .unwrap()
        .with_budget_owner_keys(input.keys.clone());
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_millis(),
    )
    .unwrap();
    let mut tx = store.begin(TransactionMode::ReadOnly).await.unwrap();
    tx.authenticate(&input.enrolled.auth).await.unwrap();
    tx.set_tenant(input.enrolled.tenant_id).await.unwrap();
    let signed = tx
        .advisory_budget_policy_store()
        .unwrap()
        .authorized_budget_policy(input.workspace, now)
        .await
        .unwrap()
        .expect("signed current budget required");
    tx.commit().await.unwrap();
    let headroom = crate::s03_v4::s03_live::budget::remaining_for_pipeline(
        input.pool,
        &store,
        input.enrolled,
        input.workspace,
        input.task,
        opportunity,
        &signed,
        wire.body.len(),
        10_000,
    )
    .await
    .expect("signed one-call Pipeline headroom required");
    assert_eq!(headroom.calls, 1);
    let captured_session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(input.workspace)
    .bind(opportunity)
    .fetch_one(input.pool)
    .await
    .unwrap();
    let actual_session: Uuid = sqlx::query_scalar(
        "SELECT id FROM agent_sessions WHERE tenant_id=$1 AND workspace_id=$2 AND native_session_id=$3 AND revoked=false",
    ).bind(input.enrolled.tenant_id).bind(input.workspace)
     .bind(pipeline_native).fetch_one(input.pool).await.unwrap();
    assert_eq!(
        captured_session, actual_session,
        "original owner session must still own capture"
    );
    println!(
        "s03_guarded_preflight call_id={} source_head={} task={} opportunity={} manifest_digest={} request_bytes={} request_sha256={} session_match=true remaining_calls=1 pipeline_dispatches=0 matrix_setup=synthetic",
        pipeline_live::CALL_ID,
        input.source_head,
        input.task,
        opportunity,
        manifest.digest,
        wire.body.len(),
        digest
    );
    if input.mode == "pipeline_preflight" {
        let (request, marker) = pipeline_live::artifact_paths();
        assert!(
            !request.exists() && !marker.exists(),
            "one-use Pipeline call already spent"
        );
        println!(
            "s03_guarded_preflight marker_absent=true artifact_dir={}",
            request.parent().unwrap().display()
        );
        return;
    }
    let synthetic = matches!(
        input.mode,
        "pipeline_synthetic_effect" | "pipeline_synthetic_no_select" | "pipeline_synthetic_abstain"
    );
    let mut artifact_paths = None;
    if synthetic {
        provider.activate(
            Arc::new(pipeline_live::SyntheticPipelineProvider {
                parser: JevPipelineSavedResponseParser::new(identity.clone()).unwrap(),
                abstain: input.mode == "pipeline_synthetic_abstain",
            }),
            Arc::new(wire.body.clone()),
        );
    } else {
        assert_eq!(input.mode, "pipeline_send");
        let (request, marker) = pipeline_live::artifact_paths();
        assert!(
            !request.exists() && !marker.exists(),
            "one-use Pipeline call already spent"
        );
        pipeline_live::review_request(&request, &wire.body);
        let reviewed = Arc::new(fs::read(&request).unwrap());
        assert_eq!(format!("{:x}", Sha256::digest(reviewed.as_slice())), digest);
        let key = std::env::var("TYPESAFE_API_KEY").expect("process-only JEV key required");
        assert!(!key.trim().is_empty());
        let native = JevPipelineProvider::new(
            JevPipelineConfig {
                identity: identity.clone(),
                endpoint: Url::parse(ENDPOINT).unwrap(),
                timeout: Duration::from_secs(10),
                maximum_request_bytes: MAX_REQUEST_BYTES,
                maximum_response_bytes: MAX_RESPONSE_BYTES,
            },
            key,
        )
        .unwrap();
        println!(
            "review exact frozen request at {} ({} bytes, sha256={digest})",
            request.display(),
            reviewed.len()
        );
        println!("to authorize one send, enter exactly: SEND JEV PIPELINE {digest}");
        std::io::stdout().flush().unwrap();
        assert!(
            pipeline_live::confirm_send(&mut std::io::stdin().lock(), &digest),
            "SEND absent or mismatched; no provider dispatch"
        );
        crate::s03_v4::s03_live::budget::remaining_for_pipeline(
            input.pool,
            &store,
            input.enrolled,
            input.workspace,
            input.task,
            opportunity,
            &signed,
            wire.body.len(),
            10_000,
        )
        .await
        .expect("budget changed before marker");
        pipeline_live::mark(&marker, &digest);
        provider.activate(Arc::new(native), reviewed);
        artifact_paths = Some((request, marker));
    }
    let outcome = route(
        owner,
        "command",
        "pipeline.recommendation.run",
        json!({"opportunity_id":opportunity}),
    )
    .await;
    let ranked = if let Some((request, marker)) = artifact_paths.as_ref() {
        crate::s03_v4::s03_live::audit::readback(
            input.pool,
            input.workspace,
            opportunity,
            &outcome,
            request,
            marker,
            &digest,
            &manifest.digest,
            &identity.provider,
            pipeline_live::CALL_ID,
        )
        .await
    } else {
        outcome["status"] == "ranked"
    };
    if !ranked {
        let effects: i64 = sqlx::query_scalar("SELECT count(*) FROM pipeline_advice_dispositions WHERE workspace_id=$1 AND opportunity_id=$2")
            .bind(input.workspace).bind(opportunity).fetch_one(input.pool).await.unwrap();
        assert_eq!(effects, 0, "abstention/no-call cannot disposition");
        let phases: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM slice_pipeline_phase_attempts WHERE workspace_id=$1",
        )
        .bind(input.workspace)
        .fetch_one(input.pool)
        .await
        .unwrap();
        assert_eq!(phases, 0, "abstention/no-call cannot complete K1");
        println!(
            "s03_guarded_outcome status={} no_caller_effect=true",
            outcome["status"]
        );
        return;
    }
    let raw: Vec<u8> = sqlx::query_scalar(
        "SELECT response_payload FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2 AND state='sealed'",
    ).bind(input.workspace).bind(opportunity).fetch_one(input.pool).await.unwrap();
    let parsed = parse_native_response(&raw, &wire, MAX_RESPONSE_BYTES).unwrap();
    assert_eq!(
        parsed.ranking,
        tect_domain::PipelineRecommendationRanking::Ranked {
            ranked_ids: outcome["ranked_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| id.as_str().unwrap().to_owned())
                .collect()
        }
    );
    println!(
        "s03_typed_ranking selected_probability={} choice_confidence={} choice_probabilities={:?} score_uncertainty={:?}",
        parsed.selected_probability,
        parsed.choice_confidence,
        parsed.choice_probabilities,
        parsed.score_uncertainty
    );
    let top = outcome["ranked_ids"][0].as_str().unwrap();
    println!(
        "to select the ranked top only, enter exactly: SELECT JEV PIPELINE {} {}",
        manifest.digest, top
    );
    std::io::stdout().flush().unwrap();
    let selected = if input.mode == "pipeline_synthetic_no_select" {
        pipeline_live::confirm_selection(&mut std::io::Cursor::new(""), &manifest.digest, top)
    } else if synthetic {
        pipeline_live::confirm_selection(
            &mut std::io::Cursor::new(format!("SELECT JEV PIPELINE {} {}\n", manifest.digest, top)),
            &manifest.digest,
            top,
        )
    } else {
        pipeline_live::confirm_selection(&mut std::io::stdin().lock(), &manifest.digest, top)
    };
    if !selected {
        let dispositions: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM pipeline_advice_dispositions WHERE workspace_id=$1 AND opportunity_id=$2",
        ).bind(input.workspace).bind(opportunity).fetch_one(input.pool).await.unwrap();
        let slices: i64 =
            sqlx::query_scalar("SELECT count(*) FROM native_slices WHERE workspace_id=$1")
                .bind(input.workspace)
                .fetch_one(input.pool)
                .await
                .unwrap();
        let runs: i64 =
            sqlx::query_scalar("SELECT count(*) FROM slice_pipeline_runs WHERE workspace_id=$1")
                .bind(input.workspace)
                .fetch_one(input.pool)
                .await
                .unwrap();
        assert_eq!((dispositions, slices, runs), (0, 0, 0));
        let phases: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM slice_pipeline_phase_attempts WHERE workspace_id=$1",
        )
        .bind(input.workspace)
        .fetch_one(input.pool)
        .await
        .unwrap();
        assert_eq!(phases, 0, "no selection cannot complete K1");
        println!("s03_selection absent_or_mismatched; no disposition or caller effect");
        return;
    }
    let matrix_effect_id: Uuid = sqlx::query_scalar(
        "SELECT id FROM matrix_planning_effect_attestations WHERE workspace_id=$1 AND verifier_request_id=$2",
    ).bind(input.workspace).bind(id(&input.matrix_effect["request_id"]))
     .fetch_one(input.pool).await.unwrap();
    let source_path = input.root.join("source");
    crate::s03_v4::s03_live::effect::verify_ranked_caller_effect(
        input.pool,
        input.workspace,
        owner,
        socket,
        input.verifier_host,
        input.workspace_key,
        input.ready,
        &input.saved["draft"]["nodes"][0],
        prepared,
        manifest,
        &outcome,
        matrix_effect_id,
        Some((
            registered_source,
            &source_path,
            input.source_head,
            EXPIRES_AT,
        )),
    )
    .await;
}
