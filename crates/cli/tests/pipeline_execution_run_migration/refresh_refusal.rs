//! Public refresh admission and immutable replay on isolated persisted fixtures.
use super::*;

async fn stored_run(pool: &PgPool, run: Uuid) -> Value {
    sqlx::query_scalar("SELECT to_jsonb(r) FROM slice_pipeline_runs r WHERE id=$1")
        .bind(run)
        .fetch_one(pool)
        .await
        .unwrap()
}

async fn retained_rows(pool: &PgPool, run: Uuid) -> Value {
    let row = stored_run(pool, run).await;
    let tenant = Uuid::parse_str(row["tenant_id"].as_str().unwrap()).unwrap();
    let workspace = Uuid::parse_str(row["workspace_id"].as_str().unwrap()).unwrap();
    let mut result = historical_seed::rows(pool, run).await;
    for (name, query) in [
        (
            "manifest_history",
            "SELECT to_jsonb(t) FROM pipeline_knowledge_manifests t WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY id",
        ),
        (
            "owned_copies",
            "SELECT to_jsonb(t) FROM knowledge_owned_copies t WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY id",
        ),
        (
            "refresh_receipts",
            "SELECT to_jsonb(t) FROM knowledge_command_receipts t WHERE tenant_id=$1 AND workspace_id=$2 AND operation='refresh' ORDER BY request_id",
        ),
        (
            "knowledge_state",
            "SELECT to_jsonb(t) FROM workspace_knowledge_state t WHERE tenant_id=$1 AND workspace_id=$2",
        ),
        (
            "publication_history",
            "SELECT to_jsonb(t) FROM knowledge_publication_events t WHERE tenant_id=$1 AND workspace_id=$2 ORDER BY id",
        ),
    ] {
        let rows: Vec<Value> = sqlx::query_scalar(query)
            .bind(tenant)
            .bind(workspace)
            .fetch_all(pool)
            .await
            .unwrap();
        result[name] = json!(rows);
    }
    result
}

fn request(row: &Value) -> Value {
    assert!(row["revision"].as_i64().unwrap() > 0);
    assert!(!row["current_phase_id"].as_str().unwrap().is_empty());
    json!({"request_id":Uuid::new_v4(),"run_id":row["id"],
        "run_revision":row["revision"],"phase_id":row["current_phase_id"]})
}

async fn refuses_without_persistence(pool: &PgPool, client: &mut Mcp, run: Uuid) {
    let before = retained_rows(pool, run).await;
    let denied = route_error(
        client,
        "command",
        "pipeline.knowledge_refresh",
        request(&before["run"]),
    )
    .await;
    assert_eq!(denied["error"]["code"], "LEGACY_MIGRATION_REQUIRED");
    let refusal = &denied["error"]["refusal"];
    assert_eq!(refusal["code"], "LEGACY_MIGRATION_REQUIRED");
    assert_eq!(refusal["rule"], "WP6-LIGHTWEIGHT-RETIRED-01");
    assert_eq!(refusal["path"], "arguments.params.run_id");
    assert_eq!(
        refusal["expected"],
        "current Lightweight K1-K5 0.7.1-native.k1k5"
    );
    assert_eq!(refusal["actual"], before["run"]["status"]);
    assert_eq!(
        refusal["next_action"],
        "get_current_context_and_use_exact_migration_action"
    );
    assert_eq!(refusal["required"], "current_lightweight_k1k5");
    assert_eq!(
        before,
        retained_rows(pool, run).await,
        "refused refresh writes no run, manifest, copies, receipt, generation or history"
    );
}

/// Call after DK activation and historical seed, before migration changes its status.
pub(super) async fn retired(pool: &PgPool, client: &mut Mcp, context: &ResolvedPipeline) {
    let run = Uuid::parse_str(context.run()["id"].as_str().unwrap()).unwrap();
    let row = stored_run(pool, run).await;
    assert_eq!(row["status"], "active");
    let definition: tect_domain::PipelineDefinitionSnapshot =
        serde_json::from_value(row["definition"].clone()).unwrap();
    assert!(tect_domain::is_retired_lightweight(&definition));
    assert_eq!(row["revision"], context.run()["revision"]);
    assert_eq!(row["current_phase_id"], context.run()["current_phase_id"]);
    refuses_without_persistence(pool, client, run).await;
}

async fn assert_ready_empty_completion(pool: &PgPool, run: Uuid) {
    let attempts: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(a) FROM slice_pipeline_phase_attempts a WHERE run_id=$1 AND phase_id IN ('K1','K2') AND outcome='completed' ORDER BY phase_ordinal")
        .bind(run).fetch_all(pool).await.unwrap();
    assert_eq!(
        attempts.len(),
        2,
        "both actual current K1/K2 completions are required"
    );
    for (attempt, phase) in attempts.iter().zip(["K1", "K2"]) {
        assert_eq!(attempt["phase_id"], phase);
        assert!(attempt["request_payload"]["consumed_knowledge"].is_null());
        let manifests: Vec<Value> = sqlx::query_scalar("SELECT to_jsonb(m) FROM pipeline_knowledge_manifests m JOIN slice_pipeline_phase_attempts a ON a.tenant_id=m.tenant_id AND a.workspace_id=m.workspace_id AND a.run_id=m.run_id AND a.phase_id=m.phase_id AND m.run_revision=(a.request_payload->>'run_revision')::bigint WHERE a.id=$1 AND m.created_at<=a.created_at ORDER BY m.id")
            .bind(Uuid::parse_str(attempt["id"].as_str().unwrap()).unwrap())
            .fetch_all(pool).await.unwrap();
        assert!(
            !manifests.is_empty(),
            "stored ready manifest must predate actual {phase} completion, not a later refresh"
        );
        for manifest in manifests {
            // Capture inserts a DK-2 manifest only after capability readiness
            // and database identity checks; an unready capture returns None.
            assert_eq!(manifest["contract_version"], "dk-2");
            assert!(
                manifest["definition_version"]
                    .as_str()
                    .unwrap()
                    .starts_with("0.7")
            );
            assert_eq!(manifest["phase_id"], phase);
            assert_eq!(
                manifest["run_revision"],
                attempt["request_payload"]["run_revision"]
            );
            assert_eq!(manifest["selected"], json!([]));
            assert_eq!(manifest["selected_resources"], json!([]));
        }
        for field in [
            "knowledge_manifest_id",
            "knowledge_manifest_digest",
            "knowledge_workspace_generation",
        ] {
            assert!(
                attempt[field].is_null(),
                "empty selected knowledge has no {field}"
            );
        }
        assert!(
            attempt["evidence_refs"]
                .as_array()
                .unwrap()
                .iter()
                .all(|reference| reference["kind"] != "knowledge_manifest")
        );
    }
}

/// Call after the parent's successor assertions; refresh changes its revision.
pub(super) async fn current_replay_and_superseded(pool: &PgPool, client: &mut Mcp, run_id: &Value) {
    let run = Uuid::parse_str(run_id.as_str().unwrap()).unwrap();
    let original = stored_run(pool, run).await;
    let definition: tect_domain::PipelineDefinitionSnapshot =
        serde_json::from_value(original["definition"].clone()).unwrap();
    assert!(!tect_domain::is_retired_lightweight(&definition));
    assert_ne!(original["status"], "superseded");
    assert_ready_empty_completion(pool, run).await;
    let exact = request(&original);
    let refreshed = route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        exact.clone(),
    )
    .await;
    let manifest = refreshed
        .get("refreshed")
        .expect("actual mutable refresh outcome");
    let after_refresh = stored_run(pool, run).await;
    assert_eq!(
        after_refresh["revision"],
        original["revision"].as_i64().unwrap() + 1
    );
    assert_eq!(manifest["run_id"], original["id"]);
    assert_eq!(manifest["run_revision"], after_refresh["revision"]);
    assert_eq!(manifest["phase_id"], original["current_phase_id"]);
    assert_eq!(after_refresh["knowledge_manifest_id"], manifest["id"]);
    let receipt: Value = sqlx::query_scalar("SELECT result_payload FROM knowledge_command_receipts WHERE operation='refresh' AND request_id=$1")
        .bind(Uuid::parse_str(exact["request_id"].as_str().unwrap()).unwrap()).fetch_one(pool).await.unwrap();
    assert_eq!(
        receipt["refreshed"], *manifest,
        "real receipt from mutable refresh"
    );
    sqlx::query("UPDATE slice_pipeline_runs SET status='superseded' WHERE id=$1")
        .bind(run)
        .execute(pool)
        .await
        .unwrap();
    let superseded = retained_rows(pool, run).await;
    let replay = route(
        client,
        "command",
        "pipeline.knowledge_refresh",
        exact.clone(),
    )
    .await;
    assert_eq!(
        replay["replay"], *manifest,
        "exact authorized replay retains its original manifest"
    );
    assert_eq!(
        superseded,
        retained_rows(pool, run).await,
        "replay writes nothing"
    );
    let mut changed = exact;
    changed["run_revision"] = after_refresh["revision"].clone();
    let conflict = route_error(client, "command", "pipeline.knowledge_refresh", changed).await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    assert_eq!(
        superseded,
        retained_rows(pool, run).await,
        "changed receipt identity grants no exemption"
    );
    refuses_without_persistence(pool, client, run).await;
    sqlx::query("UPDATE slice_pipeline_runs SET status=$2 WHERE id=$1")
        .bind(run)
        .bind(after_refresh["status"].as_str().unwrap())
        .execute(pool)
        .await
        .unwrap();
    assert_eq!(
        after_refresh,
        stored_run(pool, run).await,
        "controlled status transition restores the complete refreshed run"
    );
}
