use super::*;

struct ChangedProgramGuidance;
struct SecondProgramGuidance;

impl ProgramGuidance for ChangedProgramGuidance {
    fn planning_method(&self) -> PlanningMethodSnapshot {
        let body = "Changed trusted Program method body.";
        PlanningMethodSnapshot {
            id: "tectd-program".into(),
            version: "test-changed".into(),
            digest: "6b0844eb49b9f1850f9f582a1e2bd3800e434071426ff90816d6663a41adef04".into(),
            body: body.into(),
            origin_refs: vec!["test:tectd-program@changed".into()],
        }
    }
}

impl ProgramGuidance for SecondProgramGuidance {
    fn planning_method(&self) -> PlanningMethodSnapshot {
        PlanningMethodSnapshot {
            id: "tectd-program".into(),
            version: "test-changed-again".into(),
            digest: "d2fa157f2b730eef0a002c03e3baaf403136f049264c4e451a35b5129e6ba359".into(),
            body: "Second changed trusted Program method body.".into(),
            origin_refs: vec!["test:tectd-program@changed-again".into()],
        }
    }
}

struct ProgramGuard;

impl ProgramOutputGuard for ProgramGuard {
    fn input_bytes(&self, input: &str) -> Result<i64> {
        Ok(input.len() as i64)
    }

    fn check(&self, _program: &Program) -> Result<()> {
        Ok(())
    }
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
async fn changed_program_method_is_stale_until_explicit_refresh() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let socket = root.join("program-guidance.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-program-guidance-{}", Uuid::new_v4()),
    );
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, Vec::new()).await.unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace = format!("program-guidance-{}", Uuid::new_v4().simple());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace).await;
    client.call("open_workspace", json!({})).await;
    let started = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Capture the original Program method.",
                "task_context":{"target_iris":["urn:tect:test:program-method"],
                    "environment_iris":[]}}),
        )
        .await;
    let program = Uuid::parse_str(started["program"]["id"].as_str().unwrap()).unwrap();
    let service = WorkspaceService::new(
        Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
        Arc::new(GitSourceInspector),
        Arc::new(LocalSetupFiles),
    );
    let context = RequestContext {
        auth: enrollment.auth.clone(),
        native_session_id: native,
        workspace_key: workspace,
    };
    let guidance = ChangedProgramGuidance;
    let stale = service
        .get_program(&context, program, None, 25, &guidance)
        .await
        .unwrap();
    assert_eq!(
        stale.program.planning_knowledge.unwrap().stale_reasons,
        vec!["planning_method"]
    );
    let rejected = service
        .save_program(
            &context,
            &SaveProgram {
                program_id: program,
                revision: 1,
                input_cursor: 0,
                name: TextPatch::Set(Some("Must not save under stale guidance".into())),
                intent: TextPatch::Unchanged,
                basis: TextPatch::Unchanged,
                boundaries: TextPatch::Unchanged,
                constraints: TextPatch::Unchanged,
                success: TextPatch::Unchanged,
                working_notes: TextPatch::Unchanged,
                pending_question: TextPatch::Unchanged,
                complete: false,
                consumed_knowledge: None,
            },
            &guidance,
            &ProgramGuard,
        )
        .await;
    assert_eq!(rejected.unwrap_err(), Error::StaleContext);
    let refresh_request = RefreshProgramKnowledge {
        program_id: program,
        revision: 1,
        input_cursor: 0,
        request_id: Uuid::new_v4(),
        task_context: None,
    };
    let refreshed = service
        .refresh_program_knowledge(&context, &refresh_request, &guidance, &ProgramGuard)
        .await
        .unwrap();
    assert!(
        refreshed
            .planning_knowledge
            .as_ref()
            .unwrap()
            .stale_reasons
            .is_empty()
    );
    let preserved_context = &refreshed
        .planning_knowledge
        .as_ref()
        .unwrap()
        .manifest
        .as_ref()
        .unwrap()
        .task_context;
    assert_eq!(
        preserved_context.target_iris,
        Some(vec!["urn:tect:test:program-method".into()])
    );
    assert_eq!(preserved_context.environment_iris, Some(Vec::new()));
    let replay_under_new_method = service
        .refresh_program_knowledge(
            &context,
            &refresh_request,
            &SecondProgramGuidance,
            &ProgramGuard,
        )
        .await
        .unwrap();
    assert_eq!(
        replay_under_new_method
            .planning_knowledge
            .as_ref()
            .unwrap()
            .stale_reasons,
        vec!["planning_method"]
    );
    assert_eq!(
        replay_under_new_method
            .planning_knowledge
            .unwrap()
            .manifest
            .unwrap()
            .needs
            .method
            .version,
        "test-changed"
    );
    let fresh = service
        .get_program(&context, program, None, 25, &guidance)
        .await
        .unwrap();
    assert!(
        fresh
            .program
            .planning_knowledge
            .unwrap()
            .stale_reasons
            .is_empty()
    );
    sqlx::query(
        "UPDATE planning_knowledge_manifests SET policy_version='obsolete-test-policy' \
         WHERE owner_id=$1 AND id=(SELECT id FROM planning_knowledge_manifests \
           WHERE owner_id=$1 ORDER BY created_at DESC,id DESC LIMIT 1)",
    )
    .bind(program)
    .execute(&pool)
    .await
    .unwrap();
    let stale_policy = service
        .get_program(&context, program, None, 25, &guidance)
        .await
        .unwrap();
    assert_eq!(
        stale_policy
            .program
            .planning_knowledge
            .unwrap()
            .stale_reasons,
        vec!["planning_policy"]
    );
    let policy_rejected = service
        .save_program(
            &context,
            &SaveProgram {
                program_id: program,
                revision: 2,
                input_cursor: 0,
                name: TextPatch::Set(Some("Must not save under stale policy".into())),
                intent: TextPatch::Unchanged,
                basis: TextPatch::Unchanged,
                boundaries: TextPatch::Unchanged,
                constraints: TextPatch::Unchanged,
                success: TextPatch::Unchanged,
                working_notes: TextPatch::Unchanged,
                pending_question: TextPatch::Unchanged,
                complete: false,
                consumed_knowledge: None,
            },
            &guidance,
            &ProgramGuard,
        )
        .await;
    assert_eq!(policy_rejected.unwrap_err(), Error::StaleContext);
    let policy_refreshed = service
        .refresh_program_knowledge(
            &context,
            &RefreshProgramKnowledge {
                program_id: program,
                revision: 2,
                input_cursor: 0,
                request_id: Uuid::new_v4(),
                task_context: Some(Default::default()),
            },
            &guidance,
            &ProgramGuard,
        )
        .await
        .unwrap();
    assert!(
        policy_refreshed
            .planning_knowledge
            .as_ref()
            .unwrap()
            .stale_reasons
            .is_empty()
    );
    assert_eq!(
        policy_refreshed
            .planning_knowledge
            .unwrap()
            .manifest
            .unwrap()
            .task_context,
        Default::default()
    );
    client.finish().await;
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
