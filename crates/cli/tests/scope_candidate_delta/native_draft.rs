use super::*;
#[allow(dead_code)]
#[path = "../native_planning/support.rs"]
mod support;

fn guard(params: &mut Value, context: &Value) {
    let manifest = &context["planning_knowledge"]["manifest"];
    if manifest["id"].is_string() {
        params["consumed_knowledge"] = json!({"manifest_id":manifest["id"],
            "digest":manifest["digest"],"workspace_generation":manifest["workspace_generation"]});
    }
}
fn write(context: &Value, kind: &str, body: Value) -> Value {
    let set = &context["candidate_set"];
    let mut params = json!({"kind":kind,"candidate_set_id":set["id"],
        "revision":set["revision"],"snapshot_id":context["snapshot"]["id"],
        "input_cursor":set["latest_input"],"request_id":Uuid::new_v4()});
    params[kind] = body;
    guard(&mut params, context);
    params
}
fn candidate(local: &str, goal: &str) -> Value {
    json!({"identity":{"local":local},"title":local,"outcome":format!("Deliver {local}"),
        "trigger":"An authorized caller requests the behavior","delivered_behavior":format!("Caller observes {local}"),
        "proof":"Integration checks retain deterministic restart and replay evidence",
        "includes":[],"excludes":[],"dependencies":[],"evidence":[],"coverage_goals":[{"local":goal}]})
}
fn goal(local: &str, candidate: &str, source: Uuid) -> Value {
    json!({"identity":{"local":local},"text":format!("Deliver {candidate}"),
        "source_ref_id":source,"resolution":{"kind":"candidate","reference":{"local":candidate}}})
}
fn review(ids: &[Value]) -> Value {
    json!({"verdict":"ready","summary":"Every bounded candidate has explicit behavior and proof",
        "findings":[],"protected_change_reviews":[],"candidate_decisions":ids.iter().map(|id|
        json!({"candidate_id":id,"decision":"accept","rationale":"Explicit bounded outcome and deterministic proof"})).collect::<Vec<_>>()})
}
async fn details(client: &mut Mcp, mutation: Value) -> Value {
    CandidateFixture::from_mutation(mutation)
        .read_details(client)
        .await
        .value
}
async fn open(client: &mut Mcp, context: &Value, candidate: &Value) -> Value {
    support::route(client,"command","scope.open",json!({"request_id":Uuid::new_v4(),
        "candidate_set_id":context["candidate_set"]["id"],"candidate_set_revision":context["candidate_set"]["revision"],
        "candidate_snapshot_id":context["snapshot"]["id"],"candidate_id":candidate["id"],
        "candidate_revision":candidate["revision"]})).await
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn candidate_delta_requires_explicit_native_draft() {
    let pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    admin::migrate(&pool, &std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap())
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let socket = root.join("delta-native.sock");
    let runtime = tagged_url(
        &std::env::var("TECT_TEST_RUNTIME_URL").unwrap(),
        &format!("delta-native-{}", Uuid::new_v4()),
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
        &format!("delta-native-{}", Uuid::new_v4()),
    )
    .await;
    let program = support::ready_program(&mut client, &repo).await;
    let begun = client
        .call(
            "begin_candidate_set",
            json!({"request_id":Uuid::new_v4(),
        "program_id":program["id"],"program_revision":program["revision"],"boundary":"ongoing",
        "input":"Deliver scaffold and shared contracts, preserving both completed Scopes."}),
        )
        .await;
    let overview = CandidateFixture::from_mutation(begun)
        .read_overview(&mut client)
        .await
        .value;
    let source = planning_ref(&overview["context"]);
    let set = id(&overview["context"]["candidate_set"]["id"]);
    let draft = json!({"boundary":"ongoing","goals":[goal("g1","contracts",source),goal("g2","scaffold",source)],
        "candidates":[candidate("contracts","g1"),candidate("scaffold","g2")],"evidence":[],"blockers":[]});
    let saved = client
        .call(
            "save_candidate_set",
            write(&overview["context"], "draft", draft),
        )
        .await;
    let saved = details(&mut client, saved).await;
    let old = saved["draft"]["candidates"].as_array().unwrap().clone();
    let old_ids = old.iter().map(|c| c["id"].clone()).collect::<Vec<_>>();
    let reviewed = client
        .call(
            "save_candidate_set",
            write(&saved["context"], "review", review(&old_ids)),
        )
        .await;
    let reviewed = details(&mut client, reviewed).await;
    for c in &old {
        assert_eq!(
            open(&mut client, &reviewed["context"], c).await["disposition"],
            "created"
        );
    }
    let original_scopes: Vec<(Uuid,Uuid,i64)> = sqlx::query_as("SELECT id,source_candidate_id,source_candidate_revision FROM native_scopes WHERE source_candidate_set_id=$1 ORDER BY id")
        .bind(set).fetch_all(&pool).await.unwrap();
    assert_eq!(original_scopes.len(), 2);

    let amendment = client
        .call(
            "record_candidate_input",
            json!({"candidate_set_id":set,
        "revision":reviewed["context"]["candidate_set"]["revision"],"request_id":Uuid::new_v4(),
        "input":"Add append-only event persistence with deterministic restart and replay proof."}),
        )
        .await;
    let added = Uuid::parse_str("79f253e1-b398-4b88-a2a7-0e5f0c501e59").unwrap();
    let receipt = delta(&mut client,json!({"candidate_set_id":set,"expected_revision":amendment["candidate_set"]["revision"],
        "idempotency_key":"native-draft-boundary","operations":[{"operation":"candidate.add","candidate_id":added,
        "title":"Append-only event store","outcome":"Restart and replay retain append-only events"}]})).await;
    assert_eq!(
        receipt["actions"][0]["arguments"]["params"]["candidate_set_id"],
        set.to_string()
    );
    assert_eq!(
        receipt["actions"][0]["arguments"]["route"],
        "scope.candidates.context"
    );
    assert!(
        receipt["next_step"]
            .as_str()
            .unwrap()
            .contains("local labels")
    );
    let delta_before: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM scope_candidate_delta_candidates c WHERE candidate_set_id=$1 AND candidate_id=$2")
        .bind(set).bind(added).fetch_one(&pool).await.unwrap();
    let refreshed = client.call("refresh_candidate_set",json!({"candidate_set_id":set,
        "revision":receipt["to_revision"],"request_id":Uuid::new_v4(),"program_revision":program["revision"]})).await;
    let refreshed = details(&mut client, refreshed).await;
    assert_eq!(refreshed["draft"]["candidates"], json!(old));
    assert_eq!(
        refreshed["context"]["candidate_set"]["status"],
        "review_required"
    );
    let reviews_before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_reviews WHERE candidate_set_id=$1",
    )
    .bind(set)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        reviews_before, 1,
        "refresh must not invent review decisions"
    );
    for (ids, missing) in [
        (vec![old_ids[0].clone(), json!(added)], true),
        (
            vec![old_ids[0].clone(), old_ids[1].clone(), json!(added)],
            false,
        ),
    ] {
        let rejected = client
            .call_error(
                "save_candidate_set",
                write(&refreshed["context"], "review", review(&ids)),
            )
            .await;
        assert_eq!(rejected["error"]["code"], "invalid_arguments");
        assert_eq!(
            rejected["error"]["details"]["pointer"],
            "/params/review/candidate_decisions"
        );
        let diagnostic = rejected["error"]["details"]["reason"]
            .as_str()
            .expect("review reason");
        assert!(diagnostic.contains(&added.to_string()), "{rejected}");
        assert!(diagnostic.contains("unknown candidate IDs"), "{rejected}");
        assert_eq!(
            diagnostic.contains("missing candidate IDs"),
            missing,
            "{rejected}"
        );
    }

    // An explicit complete draft provides the design that a delta title cannot supply.
    let mut complete = refreshed["draft"].clone();
    for key in [
        "delta",
        "protected_changes",
        "pending_question",
        "empty_disposition",
    ] {
        complete.as_object_mut().unwrap().remove(key);
    }
    let refs = &refreshed["context"]["snapshot"]["source_refs"];
    let current_source = refs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "planning_input" && r["input_sequence"] == 1)
        .unwrap()["id"]
        .clone();
    for g in complete["goals"].as_array_mut().unwrap() {
        g["identity"] = json!({"id":g["id"],"revision":g["revision"]});
        g["resolution"]["reference"] = json!({"id":g["resolution"]["id"]});
        g["resolution"].as_object_mut().unwrap().remove("id");
        g["source_ref_id"] = current_source.clone();
        if g["exact_quote"].is_null() {
            g.as_object_mut().unwrap().remove("exact_quote");
        }
        for key in ["id", "revision"] {
            g.as_object_mut().unwrap().remove(key);
        }
    }
    for c in complete["candidates"].as_array_mut().unwrap() {
        c["identity"] = json!({"id":c["id"],"revision":c["revision"]});
        c["coverage_goals"] = json!(
            c["coverage_goal_ids"]
                .as_array()
                .unwrap()
                .iter()
                .map(|id| json!({"id":id}))
                .collect::<Vec<_>>()
        );
        c["evidence"] = json!([]);
        for key in ["id", "revision", "coverage_goal_ids", "evidence_ids"] {
            c.as_object_mut().unwrap().remove(key);
        }
    }
    let new_source = refs
        .as_array()
        .unwrap()
        .iter()
        .find(|r| r["kind"] == "planning_input" && r["input_sequence"] == 2)
        .unwrap()["id"]
        .clone();
    complete["goals"]
        .as_array_mut()
        .unwrap()
        .push(goal("g3", "event_store", id(&new_source)));
    complete["candidates"]
        .as_array_mut()
        .unwrap()
        .push(candidate("event_store", "g3"));
    let saved = client
        .call(
            "save_candidate_set",
            write(&refreshed["context"], "draft", complete),
        )
        .await;
    let saved = details(&mut client, saved).await;
    assert_eq!(
        &saved["draft"]["candidates"].as_array().unwrap()[..2],
        old.as_slice()
    );
    let native_new = saved["draft"]["candidates"][2].clone();
    assert_ne!(native_new["id"], added.to_string());
    let all_ids = saved["draft"]["candidates"]
        .as_array()
        .unwrap()
        .iter()
        .map(|c| c["id"].clone())
        .collect::<Vec<_>>();
    let reviewed = client
        .call(
            "save_candidate_set",
            write(&saved["context"], "review", review(&all_ids)),
        )
        .await;
    let reviewed = details(&mut client, reviewed).await;
    assert_eq!(
        open(&mut client, &reviewed["context"], &native_new).await["disposition"],
        "created"
    );
    let final_scopes: Vec<(Uuid,Uuid,i64)> = sqlx::query_as("SELECT id,source_candidate_id,source_candidate_revision FROM native_scopes WHERE source_candidate_set_id=$1 ORDER BY id")
        .bind(set).fetch_all(&pool).await.unwrap();
    assert_eq!(final_scopes.len(), 3);
    assert!(original_scopes.iter().all(|s| final_scopes.contains(s)));
    let delta_after: Value = sqlx::query_scalar("SELECT to_jsonb(c) FROM scope_candidate_delta_candidates c WHERE candidate_set_id=$1 AND candidate_id=$2")
        .bind(set).bind(added).fetch_one(&pool).await.unwrap();
    assert_eq!(delta_before, delta_after);
    let review_count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM scope_candidate_reviews WHERE candidate_set_id=$1",
    )
    .bind(set)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(review_count, 2);
    println!(
        "delta_native_boundary set={set} old_scopes=2 final_scopes=3 delta_candidate={added} native_candidate={} rejected_reviews=2",
        native_new["id"]
    );
    client.finish().await;
}
