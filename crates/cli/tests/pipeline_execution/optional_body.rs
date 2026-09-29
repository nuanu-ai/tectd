use super::*;

pub(super) async fn run() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").expect("TECT_TEST_ADMIN_URL required");
    let runtime_url =
        std::env::var("TECT_TEST_RUNTIME_URL").expect("TECT_TEST_RUNTIME_URL required");
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").expect("TECT_TEST_RUNTIME_ROLE required");
    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("pipeline-optional-body.sock");
    let runtime = tagged_url(
        &runtime_url,
        &format!("tect-pipeline-optional-body-{}", Uuid::new_v4()),
    );
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let key = format!("pipeline-optional-body-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    let (source, candidate) = ready_source_candidate(&mut client, &repo).await;
    let opened_scope = route(
        &mut client,
        "command",
        "scope.open",
        json!({"request_id":Uuid::new_v4(),
            "candidate_set_id":source["candidate_set"]["id"],
            "candidate_set_revision":source["candidate_set"]["revision"],
            "candidate_snapshot_id":source["snapshot"]["id"],
            "candidate_id":candidate["id"],"candidate_revision":candidate["revision"]}),
    )
    .await;
    let saved = save(
        &mut client,
        &opened_scope["created"]["planning"],
        lightweight_draft(),
    )
    .await;
    let reviewed = review(&mut client, &saved).await;
    let work = &reviewed["draft"]["nodes"][0];
    let opened_slice = route(
        &mut client,
        "command",
        "slice.open",
        open_slice(&reviewed, work, Uuid::new_v4()),
    )
    .await;
    let slice = &opened_slice["created"];
    let begun = route(
        &mut client,
        "command",
        "slice.pipeline.begin",
        json!({"request_id":Uuid::new_v4(),
            "scope_id":reviewed["scope"]["id"],"slice_id":slice["id"],
            "slice_revision":slice["revision"],"delivery_mode":"phasewise",
            "definition_version":"0.7.0-native.k1k5",
            "qualification_reason":"Verify the bounded v0.7 optional-body persistence contract."}),
    )
    .await;
    let initial = begun["created"].clone();
    let run_id = id(&initial["run"]["id"]);
    assert_eq!(initial["run"]["definition_version"], "0.7.0-native.k1k5");
    assert_eq!(initial["run"]["current_phase_id"], "K1");

    let mut k1_request = completion_request(&initial, "completed", "continue", None, false);
    assert!(k1_request["output"].get("body").is_some());
    k1_request["output"].as_object_mut().unwrap().remove("body");
    for (field, value) in [
        ("fit", "bounded_understood"),
        ("parent", "current_confirmed"),
        ("preflight", "current_clear"),
        ("authority", "authorized"),
        ("route", "none"),
    ] {
        k1_request["output"]["fields"][field] = json!(value);
    }
    let k1_request_id = id(&k1_request["request_id"]);
    let after_k1 = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        k1_request,
    )
    .await["context"]
        .clone();
    assert_eq!(after_k1["run"]["current_phase_id"], "K2");
    let k1_attempt = after_k1["attempts"]
        .as_array()
        .unwrap()
        .iter()
        .find(|attempt| attempt["phase_id"] == "K1")
        .unwrap();
    let k1_output_id = id(&k1_attempt["output_id"]);
    let k1_digest = k1_attempt["output_digest"].as_str().unwrap().to_owned();

    let k2 = after_k1["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K2")
        .unwrap();
    let explicit_body = "explicit body survives the pinned output read";
    let mut k2_output = phase_output(k2, "optional-body-K2", "completed", "continue");
    k2_output["body"] = json!(explicit_body);
    for (field, value) in [
        ("isolation", "confirmed"),
        ("ownership", "confirmed"),
        ("overlap", "clear"),
        ("route", "none"),
    ] {
        k2_output["fields"][field] = json!(value);
    }
    let k2_request_id = Uuid::new_v4();
    let after_k2 = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":k2_request_id,"run_id":run_id,
            "run_revision":after_k1["run"]["revision"],"phase_id":"K2",
            "outcome":"completed","transition":"continue","output":k2_output,
            "consumed_outputs":[],"consumed_inputs":[],"publish_blocked_result":false}),
    )
    .await["context"]
        .clone();
    assert_eq!(after_k2["run"]["current_phase_id"], "K3");

    let rows: Vec<(Uuid, String, serde_json::Value, String)> = sqlx::query_as(
        "SELECT id,body,fields,body_digest FROM slice_pipeline_phase_outputs \
         WHERE run_id=$1 AND phase_id IN ('K1','K2') ORDER BY phase_ordinal",
    )
    .bind(run_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(rows.len(), 2);
    assert_eq!(rows[0].0, k1_output_id);
    assert_eq!(rows[0].1, "");
    assert!(rows[0].2["fit"].as_str().is_some());
    assert_eq!(rows[0].3, k1_digest);
    assert_eq!(rows[1].1, explicit_body);
    assert!(rows[1].2["source_provenance"].as_str().is_some());

    let evidence: serde_json::Value = sqlx::query_scalar(
        "SELECT evidence_refs FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND request_id=$2",
    )
    .bind(run_id)
    .bind(k2_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(evidence.as_array().unwrap().iter().any(|reference| {
        reference["kind"] == "output"
            && reference["reference"] == k1_output_id.to_string()
            && reference["phase_id"] == "K1"
            && reference["revision"] == 1
            && reference["digest"] == k1_digest
    }));
    let k1_attempt_count: i64 = sqlx::query_scalar(
        "SELECT pg_catalog.count(*) FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND request_id=$2",
    )
    .bind(run_id)
    .bind(k1_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(k1_attempt_count, 1);

    let empty_output = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id,"view":"output","output_id":k1_output_id,"digest":k1_digest}),
    )
    .await;
    assert!(empty_output.get("body").is_none());
    let explicit_output = route(
        &mut client,
        "query",
        "slice.pipeline.context",
        json!({"run_id":run_id,"view":"output","output_id":rows[1].0,"digest":rows[1].3}),
    )
    .await;
    assert_eq!(explicit_output["body"], explicit_body);

    let k3 = after_k2["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K3")
        .unwrap();
    let mut k3_rework_output = phase_output(k3, "rework-K3", "waiting_input", "continue");
    k3_rework_output["fields"]["review_mode"] = json!("self");
    let k3_rework_request_id = Uuid::new_v4();
    let k3_rework_request = json!({
        "request_id":k3_rework_request_id,"run_id":run_id,
        "run_revision":after_k2["run"]["revision"],"phase_id":"K3",
        "outcome":"waiting_input","transition":"continue","output":k3_rework_output,
        "consumed_outputs":[],"consumed_inputs":[],"revisit_phase_id":"K2",
        "publish_blocked_result":false
    });
    let reworked = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        k3_rework_request.clone(),
    )
    .await;
    let after_rework = reworked["context"].clone();
    assert_eq!(after_rework["run"]["status"], "active");
    assert_eq!(after_rework["run"]["current_phase_id"], "K2");
    assert_eq!(after_rework["run"]["current_phase_ordinal"], 2);
    assert_eq!(
        after_rework["run"]["revision"].as_i64().unwrap(),
        after_k2["run"]["revision"].as_i64().unwrap() + 1
    );

    let replayed_rework = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        k3_rework_request,
    )
    .await;
    assert_eq!(replayed_rework, reworked);
    let persisted_rework: (String, String, Option<String>, i64) = sqlx::query_as(
        "SELECT outcome,transition,revisit_phase_id,attempt FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND request_id=$2",
    )
    .bind(run_id)
    .bind(k3_rework_request_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        persisted_rework,
        (
            "waiting_input".into(),
            "continue".into(),
            Some("K2".into()),
            1
        )
    );

    let k2 = after_rework["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K2")
        .unwrap();
    let mut k2_retry_output = phase_output(k2, "retry-K2", "completed", "continue");
    for (field, value) in [
        ("isolation", "confirmed"),
        ("ownership", "confirmed"),
        ("overlap", "clear"),
        ("route", "none"),
    ] {
        k2_retry_output["fields"][field] = json!(value);
    }
    let after_k2_retry = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":Uuid::new_v4(),"run_id":run_id,
            "run_revision":after_rework["run"]["revision"],"phase_id":"K2",
            "outcome":"completed","transition":"continue","output":k2_retry_output,
            "consumed_outputs":[],"consumed_inputs":[],"publish_blocked_result":false}),
    )
    .await["context"]
        .clone();
    assert_eq!(after_k2_retry["run"]["status"], "active");
    assert_eq!(after_k2_retry["run"]["current_phase_id"], "K3");

    let k3 = after_k2_retry["definition"]["phases"]
        .as_array()
        .unwrap()
        .iter()
        .find(|phase| phase["id"] == "K3")
        .unwrap();
    let mut k3_pass_output = phase_output(k3, "fresh-K3", "completed", "continue");
    k3_pass_output["fields"]["review_mode"] = json!("self");
    let after_k3_pass = route(
        &mut client,
        "command",
        "slice.pipeline.phase.complete",
        json!({"request_id":Uuid::new_v4(),"run_id":run_id,
            "run_revision":after_k2_retry["run"]["revision"],"phase_id":"K3",
            "outcome":"completed","transition":"continue","output":k3_pass_output,
            "consumed_outputs":[],"consumed_inputs":[],"publish_blocked_result":false}),
    )
    .await["context"]
        .clone();
    assert_eq!(after_k3_pass["run"]["current_phase_id"], "K4");
    let attempt_counts: Vec<(String, i64)> = sqlx::query_as(
        "SELECT phase_id,pg_catalog.count(*) FROM slice_pipeline_phase_attempts \
         WHERE run_id=$1 AND phase_id IN ('K2','K3') GROUP BY phase_id ORDER BY phase_id",
    )
    .bind(run_id)
    .fetch_all(&pool)
    .await
    .unwrap();
    assert_eq!(attempt_counts, vec![("K2".into(), 2), ("K3".into(), 2)]);

    let migration_count: i64 =
        sqlx::query_scalar("SELECT pg_catalog.count(*) FROM _sqlx_migrations")
            .fetch_one(&pool)
            .await
            .unwrap();
    assert_eq!(
        migration_count,
        admin::current_schema_version(),
        "installed migration count differs from the current schema version"
    );
    let body_check: String = sqlx::query_scalar(
        "SELECT pg_catalog.pg_get_constraintdef(oid) FROM pg_catalog.pg_constraint \
         WHERE conrelid='slice_pipeline_phase_outputs'::regclass \
           AND conname='slice_pipeline_phase_outputs_body_check'",
    )
    .fetch_one(&pool)
    .await
    .unwrap();
    assert!(body_check.contains("2097152"), "{body_check}");
    assert!(!body_check.contains("btrim"), "{body_check}");
    client.finish().await;
}
