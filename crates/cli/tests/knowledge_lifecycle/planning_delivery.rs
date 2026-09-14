use super::*;

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn dk4_planning_briefs_are_delivered_at_their_reviewed_abstraction() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("dk4-planning.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk4-planning-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let mut client = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("dk4-planning-{}", Uuid::new_v4()),
    )
    .await;
    client.call("open_workspace", json!({})).await;
    let fixture: Value = serde_json::from_str(include_str!(
        "../../../postgres/src/knowledge_lifecycle/rdf/fixtures/planning-abstraction.json"
    ))
    .unwrap();
    commit_create(&mut client, fixture["document"].clone()).await;
    let registered = client.call("register_source", json!({"path":repo})).await;
    client
        .call(
            "select_worktrees",
            json!({"worktree_ids":[registered["id"]]}),
        )
        .await;

    let begun = client
        .call(
            "begin_program",
            json!({
                "request_id":Uuid::new_v4(),"input":"Plan the fixture-region service operation.",
                "task_context":fixture_task_context()
            }),
        )
        .await;
    let program_manifest = &begun["program"]["planning_knowledge"]["manifest"];
    assert_planning_abstraction(
        program_manifest,
        "Plan service operation within region R1; expansion beyond that boundary requires a new owner decision.",
    );
    let program = client.call("save_program", json!({
        "program_id":begun["program"]["id"],"revision":begun["program"]["revision"],"input_cursor":1,
        "name":"R1 service operation","intent":"Operate within the owner-approved regional boundary",
        "basis":"The published owner decision defines the region.","boundaries":"R1 only",
        "constraints":"Other regions remain refused.","success":"R1 works and another region is refused.",
        "complete":true,"consumed_knowledge":planning_guard(program_manifest)
    })).await;
    let candidates = client
        .call(
            "begin_candidate_set",
            json!({
                "request_id":Uuid::new_v4(),"program_id":program["program"]["id"],
                "program_revision":program["program"]["revision"],"boundary":"ongoing",
                "input":"Form the bounded R1 delivery Scope.","task_context":fixture_task_context()
            }),
        )
        .await;
    let context = &candidates["context"];
    let scope_manifest = &context["planning_knowledge"]["manifest"];
    assert_planning_abstraction(
        scope_manifest,
        "Keep R1 deployment capability and regional isolation verification in the same Scope boundary; other regions remain excluded.",
    );
    let inputs = client
        .call(
            "candidate_context",
            json!({
                "candidate_set_id":context["candidate_set"]["id"],"view":"inputs","limit":25
            }),
        )
        .await;
    let saved = route(&mut client,"command","scope.candidates.save",json!({
        "kind":"draft","candidate_set_id":context["candidate_set"]["id"],"revision":1,
        "snapshot_id":context["snapshot"]["id"],"input_cursor":1,"request_id":Uuid::new_v4(),
        "consumed_knowledge":planning_guard(scope_manifest),
        "draft":{"boundary":"ongoing","goals":[{"identity":{"local":"goal"},
          "text":"Deliver R1 operation with regional isolation proof","source_ref_id":inputs["items"][0]["input"]["source_ref_id"],
          "resolution":{"kind":"candidate","reference":{"local":"scope"}}}],"evidence":[],"candidates":[{
          "identity":{"local":"scope"},"title":"R1 operation and isolation proof",
          "outcome":"R1 works and another region is refused","trigger":"Owner-approved R1 boundary",
          "delivered_behavior":"Working R1 path with negative regional proof","proof":"Positive R1 and negative other-region checks",
          "includes":["R1 deployment","regional isolation verification"],"excludes":["other regions"],
          "dependencies":[],"coverage_goals":[{"local":"goal"}],"evidence":[]}],
          "blockers":[],"protected_changes":[]}
    })).await;
    let candidate = saved["draft"]["candidates"][0].clone();
    let reviewed = route(&mut client,"command","scope.candidates.save",json!({
        "kind":"review","candidate_set_id":saved["context"]["candidate_set"]["id"],
        "revision":saved["context"]["candidate_set"]["revision"],"snapshot_id":context["snapshot"]["id"],
        "input_cursor":saved["context"]["candidate_set"]["input_cursor"],"request_id":Uuid::new_v4(),
        "consumed_knowledge":planning_guard(scope_manifest),
        "review":{"verdict":"ready","summary":"The R1 Scope is vertical and bounded.","findings":[],
          "candidate_decisions":[{"candidate_id":candidate["id"],"decision":"accept","rationale":"One delivery boundary."}]}
    })).await;
    let ready = &reviewed["context"];
    let opened = route(&mut client,"command","scope.open",json!({
        "request_id":Uuid::new_v4(),"candidate_set_id":ready["candidate_set"]["id"],
        "candidate_set_revision":ready["candidate_set"]["revision"],"candidate_snapshot_id":ready["snapshot"]["id"],
        "candidate_id":candidate["id"],"candidate_revision":candidate["revision"],
        "task_context":fixture_task_context(),"consumed_knowledge":planning_guard(scope_manifest)
    })).await;
    let slice_manifest = &opened["created"]["planning"]["planning_knowledge"]["manifest"];
    assert_planning_abstraction(
        slice_manifest,
        "Plan vertical outcomes containing a working R1 path and negative proof that another region is refused.",
    );
    client.finish().await;
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn dk4_reference_warns_required_blocks_and_overlap_preserves_purposes() {
    if std::env::var("TECT_TEST_DK2").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("dk4-purpose.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-dk4-purpose-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("dk4-purpose-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &workspace_key).await;
    client.call("open_workspace", json!({})).await;

    let reference = commit_create(
        &mut client,
        planning_document("urn:tect:dk4:reference", &["reference"]),
    )
    .await;
    let reference_unit = reference.receipt["applied_operations"][0]["unit_id"].clone();
    mark_needs_review(&pool, &reference_unit).await;
    let begun = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Use the reference planning brief.",
                "task_context":{"target_iris":["urn:tect:dk4:reference"]}}),
        )
        .await;
    let refreshed = route(
        &mut client,
        "command",
        "program.knowledge.refresh",
        json!({"program_id":begun["program"]["id"],"revision":begun["program"]["revision"],
            "input_cursor":begun["program"]["input_cursor"],"request_id":Uuid::new_v4()}),
    )
    .await;
    let reference_status = &refreshed["program"]["planning_knowledge"];
    assert_eq!(
        reference_status["warnings"],
        json!(["reference_knowledge_needs_review"])
    );
    assert!(
        reference_status["stale_reasons"].is_null()
            || reference_status["stale_reasons"]
                .as_array()
                .is_some_and(Vec::is_empty)
    );
    let draft = client
        .call(
            "save_program",
            json!({
                "program_id":refreshed["program"]["id"],"revision":refreshed["program"]["revision"],
                "input_cursor":1,"name":"Reference warning","intent":"Retain nonblocking reference context",
                "basis":"Reviewed reference publication","boundaries":"One fixture workspace",
                "constraints":"Reference freshness remains visible","success":"Save succeeds with a warning",
                "complete":false
            }),
        )
        .await;
    let draft_id = Uuid::parse_str(draft["program"]["id"].as_str().unwrap()).unwrap();
    let draft_revision = draft["program"]["revision"].as_i64().unwrap();
    let draft_lineage: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM planning_knowledge_consumptions \
         WHERE relation_name='programs' AND row_id=$1 AND row_revision=$2 AND NOT redacted",
    )
    .bind(draft_id)
    .bind(draft_revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(draft_lineage, 1);
    let saved = client
        .call(
            "save_program",
            json!({"program_id":draft["program"]["id"],"revision":draft_revision,
                "input_cursor":1,"complete":true}),
        )
        .await;
    assert_eq!(saved["program"]["status"], "open");

    let overlap = commit_create(
        &mut client,
        planning_document("urn:tect:dk4:overlap", &["required", "reference"]),
    )
    .await;
    let overlap_unit = overlap.receipt["applied_operations"][0]["unit_id"].clone();
    let begun = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Use the overlapping required brief.",
                "task_context":{"target_iris":["urn:tect:dk4:overlap"]}}),
        )
        .await;
    let initially_selected = &begun["program"]["planning_knowledge"]["manifest"]["selected"];
    assert_eq!(initially_selected.as_array().unwrap().len(), 1);
    assert_eq!(
        initially_selected[0]["purposes"],
        json!(["required", "reference"])
    );
    assert_eq!(
        initially_selected[0]["why_included"]
            .as_array()
            .unwrap()
            .len(),
        2
    );
    let missing_required_guard = route_error(
        &mut client,
        "command",
        "program.save",
        json!({"program_id":begun["program"]["id"],"revision":begun["program"]["revision"],
            "input_cursor":1,"complete":false}),
    )
    .await;
    assert_eq!(missing_required_guard["error"]["code"], "stale_context");
    mark_needs_review(&pool, &overlap_unit).await;
    let refreshed = route(
        &mut client,
        "command",
        "program.knowledge.refresh",
        json!({"program_id":begun["program"]["id"],"revision":begun["program"]["revision"],
            "input_cursor":begun["program"]["input_cursor"],"request_id":Uuid::new_v4()}),
    )
    .await;
    let required_status = &refreshed["program"]["planning_knowledge"];
    assert!(
        required_status["stale_reasons"]
            .as_array()
            .unwrap()
            .iter()
            .any(|value| value == "knowledge_needs_review")
    );
    let blocked = route_error(
        &mut client,
        "command",
        "program.save",
        json!({"program_id":refreshed["program"]["id"],
            "revision":refreshed["program"]["revision"],"input_cursor":1,"complete":false,
            "consumed_knowledge":planning_guard(&required_status["manifest"])}),
    )
    .await;
    assert_eq!(blocked["error"]["code"], "stale_context");

    let mut future = planning_document("urn:tect:dk4:future", &["required"]);
    future["valid_from"] = json!("2035-01-01T00:00:00Z");
    commit_create(&mut client, future).await;
    let future = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Plan the future fixture.",
                "task_context":{"target_iris":["urn:tect:dk4:future"]}}),
        )
        .await;
    let future_manifest = &future["program"]["planning_knowledge"]["manifest"];
    assert_eq!(future_manifest["selected"], json!([]));
    assert!(
        future_manifest["unresolved_needs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["reason"] == "knowledge_not_effective")
    );

    let unknown = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Plan with intentionally unknown selectors.",
                "task_context":{}}),
        )
        .await;
    assert!(
        unknown["program"]["planning_knowledge"]["manifest"]["unresolved_needs"]
            .as_array()
            .unwrap()
            .iter()
            .any(|gap| gap["reason"] == "required_selector_context_missing")
    );

    let known_empty = client
        .call(
            "begin_program",
            json!({"request_id":Uuid::new_v4(),"input":"Plan with no applicable targets.",
                "task_context":{"target_iris":[]}}),
        )
        .await;
    let known_empty_manifest = &known_empty["program"]["planning_knowledge"]["manifest"];
    assert_eq!(known_empty_manifest["selected"], json!([]));
    assert_eq!(known_empty_manifest["unresolved_needs"], json!([]));
    client.finish().await;
}
