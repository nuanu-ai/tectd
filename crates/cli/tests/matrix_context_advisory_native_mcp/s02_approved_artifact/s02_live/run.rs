use super::*;
#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit preflight or send; fresh isolated PostgreSQL 18.6 only"]
async fn one_shot_owner_attested_s02_matrix() {
    if relaunch_with_isolated_codex_home(
        "s02_approved_artifact::s02_live::run::one_shot_owner_attested_s02_matrix",
    ) {
        return;
    }
    let mode = std::env::var("JEV_MATRIX_ONE_SHOT_MODE")
        .expect("set preflight, synthetic, synthetic_pipeline, pipeline_preflight, pipeline_send, pipeline_synthetic_effect, pipeline_synthetic_no_select, pipeline_synthetic_abstain or send");
    assert!(matches!(
        mode.as_str(),
        "preflight"
            | "synthetic"
            | "synthetic_pipeline"
            | "pipeline_preflight"
            | "pipeline_send"
            | "pipeline_synthetic_effect"
            | "pipeline_synthetic_no_select"
            | "pipeline_synthetic_abstain"
            | "send"
    ));
    let pipeline_probe = matches!(
        mode.as_str(),
        "synthetic_pipeline"
            | "pipeline_preflight"
            | "pipeline_send"
            | "pipeline_synthetic_effect"
            | "pipeline_synthetic_no_select"
            | "pipeline_synthetic_abstain"
    );
    let synthetic_mode = mode == "synthetic" || pipeline_probe;
    let profile = std::env::var(PROFILE_ENV).expect("explicit local provider profile required");
    assert_eq!(
        std::env::var("JEV_MATRIX_ONE_SHOT_RANKING_POLICY").as_deref(),
        Ok("robust-trial-v1"),
        "-6 must explicitly opt into the versioned trial policy"
    );
    assert!(
        !profile.is_empty()
            && profile.len() <= 128
            && profile
                .chars()
                .all(|ch| ch.is_ascii_alphanumeric() || matches!(ch, '-' | '_' | '.'))
    );
    let artifacts = if mode == "send" {
        let paths = artifact_paths();
        assert!(
            !paths.0.exists() && !paths.1.exists(),
            "one-use call ID already consumed"
        );
        Some(paths)
    } else {
        None
    };
    let now = now_seconds();
    assert!(
        (OBSERVED_AT..EXPIRES_AT).contains(&now),
        "Owner attestation not yet valid or expired"
    );
    let (pool, runtime_url) = fresh_artifact_fixture().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    let source_head = if pipeline_probe {
        Some(source::clone_current_dev_source(&repo))
    } else {
        repository(&repo); // Existing S02 fixture; not the installed Work.
        None
    };
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("active-jev-s02-mvp-{}", Uuid::new_v4());
    let bootstrap = PgStore::connect(&runtime_url, 4).await.unwrap();
    let (workspace, keys) = signed_fixture_budget_with_token_ceilings(
        &bootstrap,
        &enrolled,
        &workspace_key,
        if pipeline_probe { 2 } else { 1 },
        24_000,
        2_000,
    )
    .await;
    let task = Uuid::new_v4();
    let body = owner_artifact_body(workspace, task);
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let issuance_socket = root.join("issuance.sock");
    let issuance_server = start_server(
        &issuance_socket,
        Arc::new(WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )),
    )
    .await;
    let native_session = Uuid::new_v4().to_string();
    let mut owner = Mcp::start(
        &issuance_socket,
        &owner_host,
        &native_session,
        &workspace_key,
    )
    .await;
    assert_eq!(
        id(&route(&mut owner, "command", "workspace.open", json!({})).await["workspace"]["id"]),
        workspace
    );
    let registered = artifact_route(
        &mut owner,
        "command",
        "slice.pipeline.evidence_artifact.register",
        json!({
            "request_id":Uuid::new_v4(), "digest":digest, "size":body.len(),
            "format":"application/vnd.tect.matrix-operating-evidence+json;version=1",
            "provenance":OWNER_SOURCE,"target":format!("matrix-task:{task}@1"),
        }),
    )
    .await;
    let artifact_id = id(&registered["artifact"]["artifact_id"]);
    let finalized = artifact_route(
        &mut owner,
        "command",
        "slice.pipeline.evidence_artifact.finalize",
        json!({
            "request_id":Uuid::new_v4(),"artifact_id":artifact_id,"revision":1,"body":body,
        }),
    )
    .await;
    assert_eq!(finalized["artifact"]["readiness"], "ready");
    let read = artifact_route(
        &mut owner,
        "query",
        "slice.pipeline.evidence_artifact.read",
        json!({
            "artifact_id":artifact_id,"revision":1,"offset":0,"limit":65536,
        }),
    )
    .await;
    assert_eq!(read["fragment"], body);
    owner.finish().await;
    issuance_server.abort();

    let approval = ApprovedMatrixEvidenceArtifact {
        tenant_id: enrolled.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        artifact_id,
        artifact_revision: 1,
        sha256: digest.clone(),
        policy_version: "owner-attested-s02-isolated/1".into(),
        max_age_seconds: 86_400,
    };
    let validator = PgMatrixEvidenceValidator::new(
        PgPool::connect_with(PgConnectOptions::from_str(&runtime_url).unwrap())
            .await
            .unwrap(),
        approval.clone(),
    );
    let scale: tect_domain::MatrixFact<String> =
        serde_json::from_value(owner_input()["envelope"]["scale"].clone()).unwrap();
    let scale_fact = RequiredMatrixFact {
        path: "/envelope/scale".into(),
        value_digest: format!("{:x}", Sha256::digest(serde_json::to_vec(&scale).unwrap())),
    };
    let reference = format!("pipeline-evidence:{artifact_id}@1");
    let accepted = validator
        .validate(workspace, task, 1, &scale_fact, &reference, now)
        .await
        .unwrap();
    assert_eq!(accepted.content_digest, digest);
    assert!(
        validator
            .validate(workspace, task, 2, &scale_fact, &reference, now)
            .await
            .is_err()
    );
    assert!(
        validator
            .validate(workspace, Uuid::new_v4(), 1, &scale_fact, &reference, now)
            .await
            .is_err()
    );
    assert!(
        validator
            .validate(workspace, task, 1, &scale_fact, &reference, EXPIRES_AT)
            .await
            .is_err()
    );
    assert!(
        validator
            .validate(
                workspace,
                task,
                1,
                &scale_fact,
                "pipeline-evidence:wrong@1",
                now
            )
            .await
            .is_err()
    );
    let capture = Arc::new(Mutex::new(None));
    let live = Arc::new(Mutex::new(None));
    let enabled = Arc::new(AtomicBool::new(false));
    let preflight_provider = Arc::new(CaptureProvider {
        inner: native_provider(&profile, "preflight-no-send".into()),
        body: capture.clone(),
        live: live.clone(),
    });
    let preflight_socket = root.join("matrix-preflight.sock");
    let preflight_server = start_server(
        &preflight_socket,
        service(
            &runtime_url,
            keys.clone(),
            approval.clone(),
            preflight_provider,
            Arc::new(SwitchedBudget(enabled.clone())),
        )
        .await,
    )
    .await;
    let mut owner = Mcp::start(
        &preflight_socket,
        &owner_host,
        &native_session,
        &workspace_key,
    )
    .await;
    let (source, candidate) = if let Some(head) = source_head.as_deref() {
        source::ready_active_jev_source_candidate(&mut owner, &repo, head).await
    } else {
        ready_source_candidate(&mut owner, &repo).await
    };
    owner.call("open_workspace", json!({})).await;
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(&pool)
            .await
            .unwrap();
    let proposal = route(
        &mut owner,
        "command",
        "engineering.matrix.context.propose",
        json!({
            "request_id":Uuid::new_v4(),"locator":locator(program),"expected_context_revision":0,
            "patches":owner_declarations(),
        }),
    )
    .await;
    let confirmed = route(
        &mut owner,
        "command",
        "engineering.matrix.context.confirm",
        json!({
            "request_id":Uuid::new_v4(),"locator":locator(program),
            "proposal_revision":proposal["proposal"]["revision"],
            "proposal_digest":proposal["proposal"]["digest"],
            "owner_response_ref":"owner-approval:active-jev-s02-sprint-declarations-2026-09-28",
        }),
    )
    .await;
    assert_eq!(confirmed["confirmation"]["proposal_revision"], 1);
    let recorded = route(
        &mut owner,
        "command",
        "task.source.record",
        json!({
            "task_id":task,"revision":1,"expected_current_revision":0,"request_id":Uuid::new_v4(),
            "input":owner_input(),"choice_set":owner_choices(task),
            "requirements_locator":locator(program),
        }),
    )
    .await;
    let configured = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional", "provider_profile_ref":{"id":profile},
            "model_configuration":{"model":MODEL},
        }),
    )
    .await;
    assert_eq!(configured["mode"], "optional");
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
        &preflight_socket,
        &verifier_host,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    independent.call("open_workspace", json!({})).await;
    let parsed: EngineeringMatrixInput = serde_json::from_value(recorded["input"].clone()).unwrap();
    let context: EffectiveMatrixRequirements = serde_json::from_value(
        route(
            &mut owner,
            "query",
            "engineering.matrix.context.effective.get",
            json!({"locator":locator(program)}),
        )
        .await,
    )
    .unwrap();
    let facts = required_matrix_operating_facts(&context, &parsed).unwrap();
    assert!(!facts.is_empty());
    let evidence: Vec<_> = facts
        .iter()
        .map(|fact| json!({"fact_path":fact.path,"evidence_ref":reference}))
        .collect();
    let missing = advice(&mut owner, task, &format!("missing-{}", Uuid::new_v4())).await;
    assert_eq!(missing["state"], "no_call");
    assert!(capture.lock().unwrap().is_none());
    let validated = route(&mut independent, "command", "engineering.matrix.verify", json!({
        "task_id":task,"expected_revision":1,"input_digest":recorded["input_digest"],"evidence":evidence,
    })).await;
    assert_eq!(validated["schema"], "tect.context-matrix-verification/1");
    assert_eq!(validated["facts"].as_array().unwrap().len(), facts.len());
    let cards = route(
        &mut independent,
        "query",
        "scope.advisory.card",
        json!({
            "task_id":task,"expected_task_revision":1
        }),
    )
    .await;
    let card_ids: Vec<&str> = cards["mandatory_cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| card["id"].as_str().unwrap())
        .collect();
    assert_eq!(card_ids, ["EM02-SCOPE@0.1", "EM02-PROTECT@0.1"]);
    let mut full_cards = Vec::new();
    for card_id in &card_ids {
        full_cards.push(
            route(
                &mut independent,
                "query",
                "scope.advisory.card",
                json!({
                    "task_id":task,"expected_task_revision":1,"detail":"full","card_id":card_id
                }),
            )
            .await,
        );
    }
    let full_cards: [Value; 2] = full_cards.try_into().unwrap();
    for binding in validated["facts"].as_array().unwrap() {
        assert_eq!(binding["content_digest"], digest);
    }
    let skipped = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,
            "request_key":format!("skip-{}",Uuid::new_v4()),"request_preference":"skip",
        }),
    )
    .await;
    assert_eq!(skipped["state"], "no_call");
    assert!(capture.lock().unwrap().is_none());
    let disabled = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":1,"mode":"disabled",
        }),
    )
    .await;
    assert_eq!(disabled["mode"], "disabled");
    let off = advice(&mut owner, task, &format!("off-{}", Uuid::new_v4())).await;
    assert_eq!(off["state"], "no_call");
    assert!(capture.lock().unwrap().is_none());
    let restored = route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":2,"mode":"optional",
            "provider_profile_ref":{"id":profile},"model_configuration":{"model":MODEL},
        }),
    )
    .await;
    assert_eq!(restored["mode"], "optional");
    let preflight = advice(&mut owner, task, &format!("preflight-{}", Uuid::new_v4())).await;
    assert_eq!(preflight["state"], "no_call"); // Budget deliberately denies authorization.
    let wire = capture
        .lock()
        .unwrap()
        .take()
        .expect("verified bound request was not prepared");
    let wire_digest = format!("{:x}", Sha256::digest(&wire));
    let total_dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(total_dispatches, 0);
    let policy_calls: i64 = sqlx::query_scalar(
        "SELECT provider_calls FROM advisory_budget_policies WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(policy_calls, if pipeline_probe { 2 } else { 1 });
    let reservations: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_budget_reservations WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(reservations, 0);
    // The same still-open Owner MCP client must own any later positive send.
    let session: Uuid = sqlx::query_scalar(
        "SELECT id FROM agent_sessions WHERE tenant_id=$1 AND workspace_id=$2 \
         AND native_session_id=$3 AND revoked=false",
    )
    .bind(enrolled.tenant_id)
    .bind(workspace)
    .bind(&native_session)
    .fetch_one(&pool)
    .await
    .unwrap();
    let captured_session: Uuid = sqlx::query_scalar(
        "SELECT session_id FROM advisory_opportunity WHERE workspace_id=$1 AND id=$2",
    )
    .bind(workspace)
    .bind(id(&preflight["opportunity_id"]))
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        session, captured_session,
        "original authorized session changed"
    );
    println!(
        "s02_preflight call_id={CALL_ID} workspace={workspace} task={task}@1 artifact={artifact_id}@1 artifact_sha256={digest} verification_digest={} profile={profile} request_bytes={} request_sha256={wire_digest} dispatches=0 owner_source={OWNER_SOURCE} alternatives=matrix-local-evidence-first,matrix-trust-first",
        validated["verification_digest"],
        wire.len()
    );
    if mode == "preflight" {
        independent.finish().await;
        owner.finish().await;
        preflight_server.abort();
        return;
    }

    let synthetic_case = if synthetic_mode {
        Some(
            std::env::var("JEV_MATRIX_SYNTHETIC_SELECTION")
                .expect("synthetic mode requires none, recommended, alternate or abstain"),
        )
    } else {
        None
    };
    let synthetic_calls = Arc::new(AtomicUsize::new(usize::from(
        synthetic_case.as_deref() == Some("abstain"),
    )));
    let mut send_marker = None;
    let live_provider: Arc<dyn MatrixAdviceProvider> = if mode == "send" {
        let (request_path, marker_path) = artifacts.unwrap();
        exclusive_write(&request_path, &wire);
        assert_eq!(fs::read(&request_path).unwrap(), wire);
        println!(
            "review {} ({} bytes; sha256={wire_digest})",
            request_path.display(),
            wire.len()
        );
        println!("enter exactly: SEND JEV MATRIX {wire_digest}");
        std::io::stdout().flush().unwrap();
        assert!(
            confirmation_matches(&mut std::io::stdin().lock(), &wire_digest),
            "confirmation absent or mismatched; zero sends"
        );
        assert!(
            now_seconds() < EXPIRES_AT,
            "attestation expired before send"
        );
        let key = std::env::var("TYPESAFE_API_KEY").expect("subprocess-only API key required");
        assert!(!key.trim().is_empty());
        let reviewed = Arc::new(ReviewedProvider {
            inner: native_provider(&profile, key),
            reviewed: Arc::new(wire),
        });
        send_marker = Some(marker_path);
        reviewed
    } else {
        assert!(synthetic_mode);
        assert!(
            std::env::var("TYPESAFE_API_KEY").is_err(),
            "synthetic mode cannot hold a credential"
        );
        Arc::new(SyntheticNativeTrialProvider {
            parser: native_provider(&profile, "synthetic-fixture-never-sent".into()),
            calls: synthetic_calls.clone(),
        })
    };
    // Keep this ORIGINAL connection; no second open_workspace request is made.
    let pre_send: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(pre_send, 0);
    *live.lock().unwrap() = Some(live_provider);
    enabled.store(true, Ordering::SeqCst);
    if let Some(marker_path) = send_marker {
        exclusive_write(
            &marker_path,
            format!("call_id={CALL_ID}\nrequest_sha256={wire_digest}\n").as_bytes(),
        );
    }
    let advised = advice(&mut owner, task, &format!("{mode}-{}", Uuid::new_v4())).await;
    if synthetic_mode {
        assert_eq!(
            synthetic_calls.load(Ordering::SeqCst),
            if synthetic_case.as_deref() == Some("abstain") {
                2
            } else {
                1
            }
        );
    }
    let dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(&pool)
            .await
            .unwrap();
    if advised["state"] != "advised" {
        assert_eq!(dispatches, 0, "no-call cannot create a dispatch");
        independent.finish().await;
        owner.finish().await;
        preflight_server.abort();
        return;
    }
    assert_eq!(dispatches, 1, "exactly one durable dispatch required");
    let audit: (i32, String, String, String, Option<String>, i64) = sqlx::query_as(
        "SELECT d.attempt_number,d.payload_digest,d.state,d.send_certainty,d.outcome,\
         (SELECT count(*) FROM advisory_budget_reservations r WHERE r.dispatch_id=d.id) \
         FROM advisory_dispatch d WHERE d.workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(audit.0, 1);
    assert_eq!(audit.1, wire_digest);
    assert_eq!(audit.5, 1, "one durable signed budget reservation required");
    println!(
        "s02_{mode}_result call_id={CALL_ID} workspace={workspace} task={task}@1 opportunity={} state={} dispatches={dispatches} dispatch_state={} send_certainty={} outcome={:?} budget_reservations={}",
        advised["opportunity_id"], advised["state"], audit.2, audit.3, audit.4, audit.5
    );
    let owner_read = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":advised["request_key"]
        }),
    )
    .await;
    let verifier_read = route(
        &mut independent,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":advised["request_key"]
        }),
    )
    .await;
    assert_eq!(
        owner_read["current_advice"],
        verifier_read["current_advice"]
    );
    let advice = &owner_read["current_advice"];
    if synthetic_case.as_deref() == Some("abstain") {
        assert_eq!(advice["outcome"]["status"], "abstained");
    }
    if advice["outcome"]["status"] == "ranked" {
        let uncertainty = &advice["trial_uncertainty"];
        assert_eq!(
            uncertainty["policy_version"],
            tect_domain::MATRIX_NATIVE_ROBUST_TRIAL_POLICY_VERSION
        );
        assert_eq!(
            uncertainty["digest_linkage"]["advice_digest"],
            advice["advice_digest"]
        );
        let ranked = advice["outcome"]["ranked_choice_ids"]
            .as_array()
            .expect("ranked advice must name choices");
        let choices = owner_choices(task);
        let eligible: Vec<String> = choices["candidates"]
            .as_array()
            .unwrap()
            .iter()
            .map(|choice| choice["candidate_id"].as_str().unwrap().to_owned())
            .collect();
        assert_eq!(ranked.len(), eligible.len());
        assert!(
            ranked
                .iter()
                .all(|choice| eligible.iter().any(|id| choice == id))
        );
        assert_eq!(
            ranked[0], uncertainty["choice_selected_candidate_id"],
            "Choice and top Score must agree"
        );
        assert_eq!(uncertainty["scores"].as_array().unwrap().len(), 2);
        println!(
            "s02_ranked advice_digest={} policy_id={} policy_version={} policy_digest={} choice_id={} choice_confidence={} choice_mass={} scores={}",
            advice["advice_digest"],
            uncertainty["policy_id"],
            uncertainty["policy_version"],
            uncertainty["policy_digest"],
            uncertainty["choice_selected_candidate_id"],
            uncertainty["choice_confidence"],
            uncertainty["choice_selected_answer_probability"],
            json!(uncertainty["scores"].as_array().unwrap().iter().map(|score| json!({
                "candidate_id":score["candidate_id"],"score_confidence":score["score_confidence"]
            })).collect::<Vec<_>>())
        );
        println!(
            "enter exactly one Owner selection: SELECT JEV MATRIX {} <choice_id>",
            advice["advice_digest"].as_str().unwrap()
        );
        std::io::stdout().flush().unwrap();
        let selected = if synthetic_mode {
            let line = match synthetic_case.as_deref().unwrap() {
                "none" => String::new(),
                "recommended" => format!(
                    "SELECT JEV MATRIX {} {}\n",
                    advice["advice_digest"].as_str().unwrap(),
                    ranked[0].as_str().unwrap()
                ),
                "alternate" => format!(
                    "SELECT JEV MATRIX {} {}\n",
                    advice["advice_digest"].as_str().unwrap(),
                    ranked[1].as_str().unwrap()
                ),
                _ => panic!("unsupported synthetic selection case"),
            };
            continuation::selection_confirmation(
                &mut std::io::Cursor::new(line),
                advice["advice_digest"].as_str().unwrap(),
                &eligible,
            )
        } else {
            continuation::selection_confirmation(
                &mut std::io::stdin().lock(),
                advice["advice_digest"].as_str().unwrap(),
                &eligible,
            )
        };
        if let Some(choice_id) = selected {
            assert!(
                now_seconds() < EXPIRES_AT,
                "Owner facts expired before selection"
            );
            let choice = choices["candidates"]
                .as_array()
                .unwrap()
                .iter()
                .find(|choice| choice["candidate_id"] == choice_id)
                .unwrap();
            let disposition = route(
                &mut owner,
                "command",
                "engineering.matrix.disposition.record",
                json!({
                    "request_id":Uuid::new_v4(),"task_id":task,"expected_task_revision":1,
                    "expected_input_digest":recorded["input_digest"],
                    "expected_choice_set_digest":recorded["choice_set_digest"],
                    "opportunity_id":advised["opportunity_id"],"basis":"after_advice",
                    "advice_id":advice["advice_id"],"advice_digest":advice["advice_digest"],
                    "decision":{"outcome":"selected","selected_choice_id":choice_id}
                }),
            )
            .await;
            let owner_disposition = route(
                &mut owner,
                "query",
                "engineering.matrix.disposition.get",
                json!({
                    "task_id":task,"request_id":disposition["request_id"]
                }),
            )
            .await;
            assert_eq!(owner_disposition["advice_digest"], advice["advice_digest"]);
            let scope = route(
                &mut owner,
                "command",
                "scope.open",
                json!({
                    "request_id":Uuid::new_v4(),
                    "candidate_set_id":source["candidate_set"]["id"],
                    "candidate_set_revision":source["candidate_set"]["revision"],
                    "candidate_snapshot_id":source["snapshot"]["id"],
                    "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]
                }),
            )
            .await;
            let selection = json!({
                "task_id":task,"task_revision":1,"disposition_id":disposition["disposition_id"],
                "selected_choice_id":choice_id,"expected_input_digest":recorded["input_digest"],
                "expected_choice_set_digest":recorded["choice_set_digest"],
                "expected_verification_digest":validated["verification_digest"],
                "mapped_draft_node_indices":[0,1,2]
            });
            let draft = continuation::owner_case_draft(choice, &full_cards);
            let mut save = planning_save(&scope["created"]["planning"], selection);
            save["draft"] = draft.clone();
            let mut shape = save.clone();
            shape.as_object_mut().unwrap().remove("kind");
            let typed: tect_domain::SaveSliceCandidateDraft = serde_json::from_value(shape)
                .unwrap_or_else(|error| panic!("selected save shape invalid: {error}"));
            typed
                .draft
                .validate()
                .expect("selected draft must satisfy native rules");
            let caller_request_id = save["request_id"].clone();
            let saved = route(&mut owner, "command", "slice.candidates.save", save).await;
            let verifier_advice = route(
                &mut independent,
                "query",
                "engineering.advisory.get",
                json!({
                    "task_id":task,"request_key":advised["request_key"]
                }),
            )
            .await;
            assert_eq!(
                verifier_advice["current_advice"]["advice_digest"],
                advice["advice_digest"]
            );
            let effect = route(&mut independent, "query", "engineering.matrix.planning_effect.get", json!({
                "candidate_set_id":saved["candidate_set"]["id"],"caller_request_id":caller_request_id
            })).await;
            assert_eq!(effect["material"]["task_id"], json!(task));
            assert_eq!(effect["material"]["task_revision"], 1);
            assert_eq!(
                effect["material"]["disposition_id"],
                disposition["disposition_id"]
            );
            assert_eq!(
                effect["material"]["verification_digest"],
                validated["verification_digest"]
            );
            assert_eq!(
                effect["material"]["catalogue_version"],
                cards["catalogue_version"]
            );
            assert_eq!(
                effect["material"]["selected_choice"]["candidate_id"],
                choice_id
            );
            let nodes = effect["material"]["nodes"].as_array().unwrap();
            assert_eq!(
                nodes.len(),
                3,
                "both mandatory cards must survive the caller save"
            );
            for (index, node) in nodes.iter().enumerate() {
                assert_eq!(node["draft_index"], index);
                assert_eq!(node["body"]["includes"], draft["nodes"][index]["includes"]);
            }
            let cards_after = route(
                &mut independent,
                "query",
                "scope.advisory.card",
                json!({
                    "task_id":task,"expected_task_revision":1
                }),
            )
            .await;
            assert_eq!(cards_after, cards);
            let verified_effect = route(&mut independent, "command", "engineering.matrix.planning_effect.verify", json!({
                "request_id":Uuid::new_v4(),"candidate_set_id":saved["candidate_set"]["id"],
                "caller_request_id":caller_request_id,
                "expected_result_revision":effect["material"]["result_revision"],
                "expected_effect_digest":effect["effect_digest"],"verdict":"matches",
                "summary":"Exact trial-selected Work and both applicable EM02 duties survive caller save."
            })).await;
            assert_eq!(verified_effect["verdict"], "matches");
            println!(
                "s02_effect task={task}@1 advice_digest={} disposition={} choice={} effect_digest={} verifier={} mandatory_cards=EM02-SCOPE@0.1,EM02-PROTECT@0.1",
                advice["advice_digest"],
                disposition["disposition_id"],
                choice_id,
                effect["effect_digest"],
                verified_effect["verifier_principal_id"]
            );
            if pipeline_probe {
                assert_eq!(choice_id, "matrix-local-evidence-first");
                let ready = support::review(&mut owner, &saved).await;
                assert_eq!(ready["candidate_set"]["status"], "ready");
                let effects: i64 = sqlx::query_scalar(
                    "SELECT count(*) FROM matrix_planning_selection_links WHERE task_id=$1",
                )
                .bind(task)
                .fetch_one(&pool)
                .await
                .unwrap();
                assert_eq!(effects, 1);
                independent.finish().await;
                owner.finish().await;
                preflight_server.abort();
                pipeline::prepare_owner_case(pipeline::OwnerCasePipeline {
                    pool: &pool,
                    runtime_url: &runtime_url,
                    keys: keys.clone(),
                    approval: approval.clone(),
                    workspace_key: &workspace_key,
                    owner_host: &owner_host,
                    root: &root,
                    task,
                    recorded: &recorded,
                    validated: &validated,
                    disposition: &disposition,
                    matrix_effect: &verified_effect,
                    saved: &saved,
                    ready: &ready,
                    source_head: source_head.as_deref().unwrap(),
                    mode: &mode,
                    workspace,
                    enrolled: &enrolled,
                    verifier_host: &verifier_host,
                })
                .await;
                return;
            }
        } else {
            println!("s02_selection absent_or_mismatched; no disposition or caller save");
        }
    } else {
        println!("s02_advice not_ranked; no disposition or caller save");
    }
    let dispositions: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_matrix_disposition WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(workspace)
    .bind(id(&advised["opportunity_id"]))
    .fetch_one(&pool)
    .await
    .unwrap();
    let effects: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM matrix_planning_selection_links WHERE workspace_id=$1 AND task_id=$2",
    )
    .bind(workspace)
    .bind(task)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispositions, effects,
        "disposition and effect must be paired"
    );
    if advice["outcome"]["status"] != "ranked" || synthetic_case.as_deref() == Some("none") {
        assert_eq!(dispositions, 0, "no selection may create a disposition");
        assert_eq!(effects, 0, "no selection may create a planning effect");
    }
    independent.finish().await;
    owner.finish().await;
    preflight_server.abort();
}

#[test]
fn one_shot_guard_is_exact_and_owner_attestation_never_refreshes() {
    let body: Value =
        serde_json::from_str(&owner_artifact_body(Uuid::new_v4(), Uuid::new_v4())).unwrap();
    assert_eq!(body["scale"]["observed_at"], OBSERVED_AT);
    assert_eq!(body["scale"]["expires_at"], EXPIRES_AT);
    for line in ["", "SEND JEV MATRIX abc", "SEND JEV MATRIX wrong\n"] {
        assert!(!confirmation_matches(
            &mut std::io::Cursor::new(line),
            "abc"
        ));
    }
    assert!(confirmation_matches(
        &mut std::io::Cursor::new("SEND JEV MATRIX abc\n"),
        "abc"
    ));
    let task = Uuid::new_v4();
    let choices = owner_choices(task);
    assert_eq!(choices["candidates"].as_array().unwrap().len(), 2);
    assert_eq!(
        choices["candidates"][0]["candidate_id"],
        "matrix-local-evidence-first"
    );
    assert_eq!(
        choices["candidates"][1]["candidate_id"],
        "matrix-trust-first"
    );
}
