use super::*;
use crate::support::{ready_source_candidate, repository};

fn draft() -> Value {
    json!({"coverage_summary":"Selected Matrix choice b bounds this diagnosis",
        "nodes":[{"kind":"work","identity":{"local":"choice-b"},
            "title":"Investigate selected approach b",
            "outcome":"The selected approach has a documented diagnosis",
            "includes":["selected approach b"],"excludes":["deployment"],
            "dependencies":[],"proof":["Synthetic diagnosis is recorded"],
            "pipeline":"slice.debug-root-cause",
            "pipeline_reason":"The selected approach needs a bounded diagnosis",
            "source_result_ids":[]}],"supersessions":[]})
}

fn save_request(planning: &Value, selection: Value) -> Value {
    let mut request = json!({
        "kind":"draft","scope_id":planning["scope"]["id"],
        "candidate_set_id":planning["candidate_set"]["id"],
        "revision":planning["candidate_set"]["revision"],
        "snapshot_id":planning["snapshot"]["id"],
        "input_cursor":planning["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),"draft":draft(),"matrix_selection":selection
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

async fn counts(pool: &PgPool, workspace: Uuid, set: Uuid) -> (i64, i64, i64) {
    sqlx::query_as(
        "SELECT (SELECT count(*) FROM native_planning_receipts \
            WHERE workspace_id=$1 AND entity_id=$2 AND operation='save_slice_draft'), \
            (SELECT count(*) FROM matrix_planning_selection_links \
            WHERE workspace_id=$1 AND candidate_set_id=$2), \
            (SELECT revision FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2)",
    )
    .bind(workspace)
    .bind(set)
    .fetch_one(pool)
    .await
    .unwrap()
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "writes only the pinned disposable PostgreSQL 18.6 fixture at migration 59"]
async fn selected_matrix_disposition_saves_native_draft_with_atomic_link() {
    // The shared guard still requires migration 58 for its original test. This
    // selection test requires the same cluster and identities at migration 59.
    let (pool, runtime_url) = disposable_pair_at_version(59).await;
    admin::migrate(&pool, "tect_ci").await.unwrap();
    let version: i64 = sqlx::query_scalar("SELECT max(version) FROM _sqlx_migrations")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version, 60);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("matrix-selection.sock");
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(Evidence(Arc::new(AtomicBool::new(false)))))
        .with_matrix_advisory_adapters(
            Arc::new(Provider(Arc::new(AtomicUsize::new(0)))),
            Arc::new(Budget),
        ),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));

    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let owner_config = root.join("owner.json");
    host_file(&owner_config, &enrolled.auth);
    let workspace_key = format!("mcp-matrix-selection-{}", Uuid::new_v4());
    let mut owner = Mcp::start(
        &socket,
        &owner_config,
        &Uuid::new_v4().to_string(),
        &workspace_key,
    )
    .await;
    let (source, candidate) = ready_source_candidate(&mut owner, &repo).await;
    let reopened = owner.call("open_workspace", json!({})).await;
    let workspace = Uuid::parse_str(reopened["workspace"]["id"].as_str().unwrap()).unwrap();
    let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let verifier_config = root.join("verifier.json");
    host_file(&verifier_config, &verifier.auth);
    let mut independent = Mcp::start(
        &socket,
        &verifier_config,
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
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":PROFILE},"model_configuration":{"model":MODEL}
        }),
    )
    .await;
    let task = Uuid::new_v4();
    let recorded = record_task(&mut owner, task, &["a", "b"]).await;
    verify(&mut independent, &recorded, task).await;
    let advice_key = format!("selection-{}", Uuid::new_v4());
    let advised = route(
        &mut owner,
        "command",
        "engineering.advisory.request",
        json!({
            "task_id":task,"expected_task_revision":1,
            "request_key":advice_key
        }),
    )
    .await;
    assert_eq!(advised["state"], "advised");
    let current = route(
        &mut owner,
        "query",
        "engineering.advisory.get",
        json!({
            "task_id":task,"request_key":advice_key
        }),
    )
    .await;
    let selected = disposition(
        &recorded,
        task,
        &advised,
        "after_advice",
        Some(&current["current_advice"]),
        json!({"outcome":"selected","selected_choice_id":"b"}),
    );
    let disposition = route(
        &mut owner,
        "command",
        "engineering.matrix.disposition.record",
        selected,
    )
    .await;
    let verification_digest: String = sqlx::query_scalar(
        "SELECT matrix_verification_digest FROM advisory_opportunity WHERE id=$1",
    )
    .bind(Uuid::parse_str(advised["opportunity_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    let selection = json!({
        "task_id":task,"task_revision":1,"disposition_id":disposition["disposition_id"],
        "selected_choice_id":"b","expected_input_digest":recorded["input_digest"],
        "expected_choice_set_digest":recorded["choice_set_digest"],
        "expected_verification_digest":verification_digest,
        "mapped_draft_node_indices":[0]
    });

    let opened = route(
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
    let planning = &opened["created"]["planning"];
    let scope_id = Uuid::parse_str(planning["scope"]["id"].as_str().unwrap()).unwrap();
    let set_id = Uuid::parse_str(planning["candidate_set"]["id"].as_str().unwrap()).unwrap();
    let baseline = counts(&pool, workspace, set_id).await;
    assert_eq!((baseline.0, baseline.1), (0, 0));

    let mut bad_digest = save_request(planning, selection.clone());
    bad_digest["matrix_selection"]["expected_verification_digest"] = json!("0".repeat(64));
    assert_error(
        &route_error(&mut owner, "command", "slice.candidates.save", bad_digest).await,
        &["stale_context"],
    );
    assert_eq!(counts(&pool, workspace, set_id).await, baseline);
    let mut out_of_range = save_request(planning, selection.clone());
    out_of_range["matrix_selection"]["mapped_draft_node_indices"] = json!([1]);
    assert_error(
        &route_error(&mut owner, "command", "slice.candidates.save", out_of_range).await,
        &["invalid_arguments"],
    );
    assert_eq!(counts(&pool, workspace, set_id).await, baseline);
    let mut duplicate = save_request(planning, selection.clone());
    duplicate["matrix_selection"]["mapped_draft_node_indices"] = json!([0, 0]);
    assert_error(
        &route_error(&mut owner, "command", "slice.candidates.save", duplicate).await,
        &["invalid_arguments"],
    );
    assert_eq!(counts(&pool, workspace, set_id).await, baseline);
    let mut wrong_choice = save_request(planning, selection.clone());
    wrong_choice["matrix_selection"]["selected_choice_id"] = json!("a");
    assert_error(
        &route_error(&mut owner, "command", "slice.candidates.save", wrong_choice).await,
        &["stale_context"],
    );
    assert_eq!(counts(&pool, workspace, set_id).await, baseline);

    let request = save_request(planning, selection);
    let saved = route(
        &mut owner,
        "command",
        "slice.candidates.save",
        request.clone(),
    )
    .await;
    let result_revision = saved["candidate_set"]["revision"].as_i64().unwrap();
    assert_eq!(
        counts(&pool, workspace, set_id).await,
        (1, 1, result_revision)
    );
    let row: (
        String,
        Uuid,
        Uuid,
        i64,
        Uuid,
        Uuid,
        String,
        Value,
        Value,
        Value,
    ) = sqlx::query_as(
        "SELECT r.operation,r.entity_id,r.request_id,l.result_revision,l.scope_id, \
                l.disposition_id,l.selected_choice_id,l.mapped_nodes, \
                r.request_payload,r.result_payload \
         FROM matrix_planning_selection_links l JOIN native_planning_receipts r \
           ON (r.tenant_id,r.workspace_id,r.entity_id,r.operation,r.request_id)= \
              (l.tenant_id,l.workspace_id,l.candidate_set_id,l.operation,l.caller_request_id) \
         WHERE l.workspace_id=$1 AND l.candidate_set_id=$2 AND l.caller_request_id=$3",
    )
    .bind(workspace)
    .bind(set_id)
    .bind(Uuid::parse_str(request["request_id"].as_str().unwrap()).unwrap())
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(row.0, "save_slice_draft");
    assert_eq!(row.1, set_id);
    assert_eq!(json!(row.2), request["request_id"]);
    assert_eq!(row.3, result_revision);
    assert_eq!(row.4, scope_id);
    assert_eq!(json!(row.5), request["matrix_selection"]["disposition_id"]);
    assert_eq!(row.6, "b");
    assert_eq!(row.8["matrix_selection"], request["matrix_selection"]);
    assert_eq!(row.9["candidate_set"]["revision"], json!(result_revision));
    let mapped = row.7.as_array().unwrap();
    assert_eq!(mapped.len(), 1);
    let receipt_node = &row.9["draft"]["nodes"][0];
    let saved_node = &saved["draft"]["nodes"][0];
    assert_eq!(row.8["draft"]["nodes"][0]["identity"]["local"], "choice-b");
    assert_eq!(mapped[0]["draft_index"], 0);
    assert_eq!(mapped[0]["node_id"], receipt_node["id"]);
    assert_eq!(mapped[0]["node_revision"], receipt_node["revision"]);
    assert_eq!(mapped[0]["node_id"], saved_node["id"]);
    assert_eq!(mapped[0]["node_revision"], saved_node["revision"]);
    let persisted_draft: Value = sqlx::query_scalar(
        "SELECT payload FROM slice_candidate_drafts \
         WHERE workspace_id=$1 AND candidate_set_id=$2 AND set_revision=$3",
    )
    .bind(workspace)
    .bind(set_id)
    .bind(result_revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(mapped[0]["node_id"], persisted_draft["nodes"][0]["id"]);
    assert_eq!(
        mapped[0]["node_revision"],
        persisted_draft["nodes"][0]["revision"]
    );

    let replay = route(
        &mut owner,
        "command",
        "slice.candidates.save",
        request.clone(),
    )
    .await;
    assert_eq!(replay["candidate_set"]["revision"], json!(result_revision));
    assert_eq!(
        counts(&pool, workspace, set_id).await,
        (1, 1, result_revision)
    );
    let mut changed = request;
    changed["draft"]["coverage_summary"] = json!("Changed draft on same request id");
    assert_error(
        &route_error(&mut owner, "command", "slice.candidates.save", changed).await,
        &["input_conflict"],
    );
    assert_eq!(
        counts(&pool, workspace, set_id).await,
        (1, 1, result_revision)
    );
    independent.finish().await;
    owner.finish().await;
    server.abort();
}
