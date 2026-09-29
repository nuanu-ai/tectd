//! Synthetic catalogue breadth through public V2 routes and operator-pinned bytes.
//! These fixture observations are not current Owner facts or production evidence.
use super::*;

struct Case {
    name: &'static str,
    mode: &'static str,
    payment: bool,
    exposed: bool,
    demand: bool,
    hotfix: bool,
    expected: &'static [&'static str],
}

const CASES: &[Case] = &[
    Case {
        name: "demo",
        mode: "demo",
        payment: false,
        exposed: false,
        demand: false,
        hotfix: false,
        expected: &["EM02-SCOPE@0.1"],
    },
    Case {
        name: "exposed-payment-mvp",
        mode: "mvp",
        payment: true,
        exposed: true,
        demand: false,
        hotfix: false,
        expected: &["EM02-SCOPE@0.1", "EM02-PROTECT@0.1", "EM02-OPERATE@0.1"],
    },
    Case {
        name: "production-demand-capacity",
        mode: "production",
        payment: false,
        exposed: true,
        demand: true,
        hotfix: false,
        expected: &["EM02-SCOPE@0.1", "EM02-OPERATE@0.1", "EM02-CAPACITY@0.1"],
    },
    Case {
        name: "production-urgent-hotfix",
        mode: "production",
        payment: true,
        exposed: true,
        demand: true,
        hotfix: true,
        expected: &[
            "EM02-SCOPE@0.1",
            "EM02-PROTECT@0.1",
            "EM02-OPERATE@0.1",
            "EM02-CAPACITY@0.1",
            "EM02-HOTFIX@0.1",
        ],
    },
];

fn case_declarations(case: &Case) -> Vec<Value> {
    let mut values = declarations(case.mode);
    if case.hotfix {
        values[1] = declaration("intent", json!({"kind":"production_hotfix"}));
        values[2] = declaration("urgency", json!("urgent synthetic repair"));
    }
    if case.demand {
        values[5] = declaration(
            "demand_commitment",
            json!("synthetic 100 requests per second"),
        );
    }
    if case.hotfix {
        values[6] = declaration("latency_commitment", json!("synthetic p95 under 200 ms"));
    }
    values
}

fn case_input(case: &Case) -> Value {
    let mut value = input();
    if case.payment {
        value["affected_guarantees"] = json!({
            "state":"known","value":["payment"],"provenance":"fixture observation"
        });
    }
    value["actual_exposure"]["value"] = json!(case.exposed);
    value["urgent_repair"]["value"] = json!(case.hotfix);
    if case.demand {
        value["demand_commitment"] = json!({
            "state":"known","value":"exceeds_verified_limit","provenance":"fixture observation"
        });
    }
    if case.hotfix {
        value["latency_commitment"] = json!({
            "state":"known","value":"within_verified_limit","provenance":"fixture observation"
        });
    }
    value
}

fn case_artifact(case: &Case, workspace: Uuid, task: Uuid, now: i64) -> String {
    let input = case_input(case);
    let mut body: Value =
        serde_json::from_str(&artifact_body(workspace, task, now - 10, now + 600)).unwrap();
    for (field, fact) in [
        ("affected_guarantees", &input["affected_guarantees"]),
        ("actual_exposure", &input["actual_exposure"]),
        ("urgent_repair", &input["urgent_repair"]),
    ] {
        body[field] = sourced(fact.clone(), now - 10, now + 600);
    }
    if case.demand {
        body["demand_commitment"] =
            sourced(input["demand_commitment"].clone(), now - 10, now + 600);
    }
    if case.hotfix {
        body["latency_commitment"] =
            sourced(input["latency_commitment"].clone(), now - 10, now + 600);
    }
    serde_json::to_string(&body).unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 2)]
#[ignore = "requires a fresh owned PostgreSQL 18.6 cluster and explicit identity guard"]
async fn public_v2_synthetic_catalogue_breadth_with_exact_approved_artifacts() {
    if relaunch_with_isolated_codex_home(
        "s02_approved_artifact::s02_breadth::public_v2_synthetic_catalogue_breadth_with_exact_approved_artifacts",
    ) {
        return;
    }
    let (pool, runtime_url) = fresh_artifact_fixture().await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    for case in CASES {
        run_case(&pool, &runtime_url, case).await;
    }
}

async fn run_case(pool: &PgPool, runtime_url: &str, case: &Case) {
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let enrolled = admin::enroll_host(pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let workspace_key = format!("matrix-breadth-{}-{}", case.name, Uuid::new_v4());
    let store = PgStore::connect(runtime_url, 4).await.unwrap();
    let (workspace, owner_keys) = signed_fixture_budget(&store, &enrolled, &workspace_key).await;
    let task = Uuid::new_v4();
    let now = i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap();
    let body = case_artifact(case, workspace, task, now);
    let digest = format!("{:x}", Sha256::digest(body.as_bytes()));
    let owner_host = root.join("owner.json");
    host_file(&owner_host, &enrolled.auth);
    let issuance_socket = root.join("issuance.sock");
    let issuance_service = Arc::new(WorkspaceService::new(
        Arc::new(PgStore::connect(runtime_url, 4).await.unwrap()),
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
    assert_eq!(id(&opened["workspace"]["id"]), workspace, "{}", case.name);
    let registered = artifact_route(
        &mut issuer,
        "command",
        "slice.pipeline.evidence_artifact.register",
        json!({"request_id":Uuid::new_v4(),"digest":digest,"size":body.len(),
            "format":"application/vnd.tect.matrix-operating-evidence+json;version=1",
            "provenance":"synthetic-test-only-operator-pinned", "target":format!("matrix-task:{task}@1")}),
    )
    .await;
    let artifact_id = id(&registered["artifact"]["artifact_id"]);
    let finalized = artifact_route(
        &mut issuer,
        "command",
        "slice.pipeline.evidence_artifact.finalize",
        json!({"request_id":Uuid::new_v4(),"artifact_id":artifact_id,"revision":1,"body":body}),
    )
    .await;
    assert_eq!(finalized["artifact"]["readiness"], "ready", "{}", case.name);
    let read = artifact_route(
        &mut issuer,
        "query",
        "slice.pipeline.evidence_artifact.read",
        json!({"artifact_id":artifact_id,"revision":1,"offset":0,"limit":65536}),
    )
    .await;
    assert_eq!(read["fragment"], body, "{}", case.name);
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
        policy_version: "synthetic-breadth-approval/1".into(),
        max_age_seconds: 600,
    };
    let runtime_pool = PgPool::connect_with(PgConnectOptions::from_str(runtime_url).unwrap())
        .await
        .unwrap();
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(
                PgStore::connect(runtime_url, 4)
                    .await
                    .unwrap()
                    .with_budget_owner_keys(owner_keys),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(
            runtime_pool,
            approval.clone(),
        ))),
    );
    let socket = root.join("case.sock");
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
    let (source, _) = ready_source_candidate(&mut owner, &repo).await;
    let program: Uuid =
        sqlx::query_scalar("SELECT program_id FROM scope_candidate_sets WHERE id=$1")
            .bind(id(&source["candidate_set"]["id"]))
            .fetch_one(pool)
            .await
            .unwrap();
    propose_confirm(&mut owner, program, 0, case_declarations(case)).await;
    let effective = route(
        &mut owner,
        "query",
        "engineering.matrix.context.effective.get",
        json!({"locator":locator(program)}),
    )
    .await;
    let case_input = case_input(case);
    let recorded = route(
        &mut owner,
        "command",
        "task.source.record",
        json!({"task_id":task,"revision":1,"expected_current_revision":0,
            "request_id":Uuid::new_v4(),"input":case_input,"choice_set":choice(task),
            "requirements_locator":locator(program)}),
    )
    .await;
    let verifier = admin::prepare_verifier_enrollment(pool, enrolled.tenant_id, workspace)
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
    let mut wrong = evidence.clone();
    wrong[0]["evidence_ref"] = json!(format!("pipeline-evidence:{}@1", Uuid::new_v4()));
    let denied = route_error(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({"task_id":task,"expected_revision":1,
            "input_digest":recorded["input_digest"],"evidence":wrong}),
    )
    .await;
    assert_eq!(
        denied["error"]["code"], "forbidden",
        "{}: {denied}",
        case.name
    );
    let verified = route(
        &mut independent,
        "command",
        "engineering.matrix.verify",
        json!({"task_id":task,"expected_revision":1,
            "input_digest":recorded["input_digest"],"evidence":evidence}),
    )
    .await;
    assert_eq!(verified["facts"].as_array().unwrap().len(), facts.len());
    assert!(
        verified["facts"]
            .as_array()
            .unwrap()
            .iter()
            .all(|binding| binding["content_digest"] == digest)
    );
    let legacy = route(
        &mut independent,
        "query",
        "scope.advisory.card",
        json!({"task_id":task,"expected_task_revision":1}),
    )
    .await;
    // The legacy route stays owner-reported; V2 cards have a separate route.
    assert_eq!(legacy["resolved"], false, "{}: {legacy}", case.name);
    assert_eq!(
        legacy["source_verification_status"],
        "owner_reported_pending_independent_verification"
    );
    let cards_request = json!({"task_id":task,"expected_task_revision":1,
        "operating_verification_digest":verified["verification_digest"]});
    let summary = route(
        &mut independent,
        "query",
        "engineering.matrix.cards.get",
        cards_request.clone(),
    )
    .await;
    let owner_summary = route(
        &mut owner,
        "query",
        "engineering.matrix.cards.get",
        cards_request.clone(),
    )
    .await;
    assert_eq!(owner_summary, summary, "{}: owner/verifier", case.name);
    assert_eq!(
        summary["schema"],
        "tect.engineering-matrix-verified-cards/1"
    );
    assert_eq!(
        summary["resolution_status"],
        "confirmed_requirements_validated_operating_evidence"
    );
    assert_eq!(summary["task_id"], json!(task));
    assert_eq!(summary["task_revision"], 1);
    assert_eq!(summary["input_digest"], recorded["input_digest"]);
    assert_eq!(
        summary["frozen_snapshot_id"],
        recorded["requirements_snapshot_id"]
    );
    assert_eq!(
        summary["authority_schema"],
        recorded["context_authority_schema"]
    );
    assert_eq!(
        summary["requirements_semantic_digest"],
        recorded["requirements_semantic_digest"]
    );
    assert_eq!(
        summary["operating_verification_digest"],
        verified["verification_digest"]
    );
    assert_eq!(summary["policy_version"], "synthetic-breadth-approval/1");
    assert!(summary["selected_card"].is_null());
    let selected: Vec<&str> = summary["mandatory_cards"]
        .as_array()
        .unwrap()
        .iter()
        .map(|card| card["id"].as_str().unwrap())
        .collect();
    assert_eq!(selected, case.expected, "{}: {summary}", case.name);
    assert_eq!(summary["catalogue_version"], "EM02-INITIAL@0.1");
    for card in summary["mandatory_cards"].as_array().unwrap() {
        let full = route(
            &mut independent,
            "query",
            "engineering.matrix.cards.get",
            json!({"task_id":task,"expected_task_revision":1,
                "operating_verification_digest":verified["verification_digest"],
                "card_id":card["id"]}),
        )
        .await;
        assert_eq!(full["mandatory_cards"], summary["mandatory_cards"]);
        assert_eq!(full["selected_card"]["id"], card["id"]);
        assert_eq!(full["selected_card"]["summary"], card["summary"]);
        assert!(
            full["selected_card"]["body"]
                .as_str()
                .is_some_and(|body| !body.is_empty())
        );
        assert_eq!(full["catalogue_version"], summary["catalogue_version"]);
    }
    for (name, params, expected_code) in [
        (
            "wrong-digest",
            json!({"task_id":task,"expected_task_revision":1,
            "operating_verification_digest":"f".repeat(64)}),
            "stale_revision",
        ),
        (
            "wrong-task",
            json!({"task_id":Uuid::new_v4(),"expected_task_revision":1,
            "operating_verification_digest":verified["verification_digest"]}),
            "not_found",
        ),
        (
            "wrong-revision",
            json!({"task_id":task,"expected_task_revision":2,
            "operating_verification_digest":verified["verification_digest"]}),
            "stale_revision",
        ),
        (
            "nonmandatory-card",
            json!({"task_id":task,"expected_task_revision":1,
            "operating_verification_digest":verified["verification_digest"],
            "card_id":"EM02-NOT-MANDATORY@0.1"}),
            "not_found",
        ),
    ] {
        let denied = route_error(
            &mut independent,
            "query",
            "engineering.matrix.cards.get",
            params,
        )
        .await;
        assert_eq!(
            denied["error"]["code"], expected_code,
            "{} {name}: {denied}",
            case.name
        );
    }
    if case.name == "demo" {
        let mut other_workspace = Mcp::start(
            &socket,
            &owner_host,
            &Uuid::new_v4().to_string(),
            &format!("matrix-other-workspace-{}", Uuid::new_v4()),
        )
        .await;
        other_workspace.call("open_workspace", json!({})).await;
        let denied = route_error(
            &mut other_workspace,
            "query",
            "engineering.matrix.cards.get",
            cards_request.clone(),
        )
        .await;
        assert_eq!(denied["error"]["code"], "not_found");
        other_workspace.finish().await;

        let foreign = admin::enroll_host(pool, None, vec![root.to_string_lossy().into_owned()])
            .await
            .unwrap();
        assert_ne!(foreign.tenant_id, enrolled.tenant_id);
        let foreign_host = root.join("foreign.json");
        host_file(&foreign_host, &foreign.auth);
        let mut other_tenant = Mcp::start(
            &socket,
            &foreign_host,
            &Uuid::new_v4().to_string(),
            &format!("matrix-other-tenant-{}", Uuid::new_v4()),
        )
        .await;
        other_tenant.call("open_workspace", json!({})).await;
        let denied = route_error(
            &mut other_tenant,
            "query",
            "engineering.matrix.cards.get",
            cards_request.clone(),
        )
        .await;
        assert_eq!(denied["error"]["code"], "not_found");
        other_tenant.finish().await;

        for (label, validator) in [
            ("missing-validator", None),
            (
                "stale-artifact",
                Some(PgMatrixEvidenceValidator::new(
                    PgPool::connect_with(PgConnectOptions::from_str(runtime_url).unwrap())
                        .await
                        .unwrap(),
                    ApprovedMatrixEvidenceArtifact {
                        max_age_seconds: 1,
                        ..approval.clone()
                    },
                )),
            ),
        ] {
            let mut isolated_service = WorkspaceService::new(
                Arc::new(PgStore::connect(runtime_url, 4).await.unwrap()),
                Arc::new(tect_host::GitSourceInspector),
                Arc::new(tect_host::LocalSetupFiles),
            );
            if let Some(validator) = validator {
                isolated_service =
                    isolated_service.with_matrix_evidence_validator(Arc::new(validator));
            }
            let denied_socket = root.join(format!("{label}.sock"));
            let listener = UnixListener::bind(&denied_socket).unwrap();
            std::fs::set_permissions(&denied_socket, std::fs::Permissions::from_mode(0o600))
                .unwrap();
            let denied_server =
                tokio::spawn(tect_host::serve(listener, Arc::new(isolated_service)));
            let mut denied_client = Mcp::start(
                &denied_socket,
                &owner_host,
                &Uuid::new_v4().to_string(),
                &workspace_key,
            )
            .await;
            denied_client.call("open_workspace", json!({})).await;
            let denied = route_error(
                &mut denied_client,
                "query",
                "engineering.matrix.cards.get",
                cards_request.clone(),
            )
            .await;
            assert_eq!(denied["error"]["code"], "forbidden", "{label}: {denied}");
            denied_client.finish().await;
            denied_server.abort();
        }
    }
    let dispatches: i64 =
        sqlx::query_scalar("SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1")
            .bind(workspace)
            .fetch_one(pool)
            .await
            .unwrap();
    let effects: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM matrix_planning_selection_links WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!((dispatches, effects), (0, 0), "{}", case.name);

    if case.demand {
        propose_confirm(
            &mut owner,
            program,
            1,
            vec![json!({"operation":"remove","path":"demand_commitment"})],
        )
        .await;
        let missing_task = Uuid::new_v4();
        let missing = route_error(
            &mut owner,
            "command",
            "task.source.record",
            json!({"task_id":missing_task,"revision":1,"expected_current_revision":0,
                "request_id":Uuid::new_v4(),"input":case_input,
                "choice_set":choice(missing_task),"requirements_locator":locator(program)}),
        )
        .await;
        assert_eq!(missing["error"]["code"], "invalid_arguments", "{missing}");
        let stale = route_error(
            &mut independent,
            "query",
            "engineering.matrix.cards.get",
            cards_request,
        )
        .await;
        assert_eq!(
            stale["error"]["code"], "stale_revision",
            "{}: {stale}",
            case.name
        );
    }
    independent.finish().await;
    owner.finish().await;
    server.abort();
}
