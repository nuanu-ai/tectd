//! Owned disposable admin fixture mutations only; never a production write path.
use super::*;

async fn artifacts(pool: &PgPool, tenant: Uuid, workspace: Uuid) -> Value {
    let mut values = serde_json::Map::new();
    for table in [
        "slice_pipeline_runs",
        "slice_pipeline_phase_attempts",
        "slice_pipeline_phase_outputs",
        "slice_pipeline_output_bindings",
        "slice_pipeline_inputs",
        "slice_pipeline_receipts",
        "pipeline_delivery_receipts",
        "pipeline_knowledge_manifests",
        "knowledge_owned_copies",
        "pipeline_research_checkpoints",
    ] {
        let sql = format!(
            "SELECT COALESCE(jsonb_agg(to_jsonb(r) ORDER BY to_jsonb(r)::text),'[]'::jsonb) FROM {table} r WHERE tenant_id=$1 AND workspace_id=$2"
        );
        let rows: Value = sqlx::query_scalar(&sql)
            .bind(tenant)
            .bind(workspace)
            .fetch_one(pool)
            .await
            .unwrap();
        values.insert(table.into(), rows);
    }
    Value::Object(values)
}

pub(super) async fn reject_definition_drift(
    pools: (&PgPool, &PgPool),
    service: &WorkspaceService,
    context: &RequestContext,
    ids: (Uuid, Uuid),
    begin: &BeginPipelineRun,
    definitions: &Definitions,
) {
    let baseline = artifacts(pools.0, ids.0, ids.1).await;
    for column in [
        "verification_plan_source_definition_version",
        "verification_plan_source_definition_digest",
    ] {
        let original = if column.ends_with("version") {
            &definitions.0.version
        } else {
            &definitions.0.digest
        };
        let drift = if column.ends_with("version") {
            format!("{original}-fixture-drift")
        } else if original == &"0".repeat(64) {
            "1".repeat(64)
        } else {
            "0".repeat(64)
        };
        let sql = format!(
            "UPDATE native_slices SET {column}=$4 WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3"
        );
        let changed = sqlx::query(&sql)
            .bind(ids.0)
            .bind(ids.1)
            .bind(begin.slice_id)
            .bind(drift)
            .execute(pools.0)
            .await
            .unwrap();
        assert_eq!(changed.rows_affected(), 1);
        let result = service
            .pipeline_run_begin(context, begin, definitions, &RunGuard)
            .await;
        // Restore even when the service unexpectedly succeeds; assertions follow restoration.
        sqlx::query(&sql)
            .bind(ids.0)
            .bind(ids.1)
            .bind(begin.slice_id)
            .bind(original)
            .execute(pools.0)
            .await
            .unwrap();
        assert_eq!(result, Err(Error::StaleContext), "{column}");
        assert_eq!(artifacts(pools.0, ids.0, ids.1).await, baseline, "{column}");
        assert_eq!(
            readback(pools.1, ids.0, ids.1).await["slice_pipeline_runs"],
            json!([])
        );
    }
}

pub(super) async fn replay_and_legacy_null(
    pools: (&PgPool, &PgPool),
    service: &WorkspaceService,
    owner_context: &RequestContext,
    ids: (Uuid, Uuid),
    begin: &BeginPipelineRun,
    begun: &PipelineRunContext,
    definitions: &Definitions,
) {
    let baseline = artifacts(pools.0, ids.0, ids.1).await;
    sqlx::query("UPDATE native_slices SET revision=revision+1,state='blocked' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(ids.0).bind(ids.1).bind(begin.slice_id).execute(pools.0).await.unwrap();
    let current: (i64, String) = sqlx::query_as(
        "SELECT revision,state FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3",
    )
    .bind(ids.0)
    .bind(ids.1)
    .bind(begin.slice_id)
    .fetch_one(pools.0)
    .await
    .unwrap();
    let replay = service
        .pipeline_run_begin(owner_context, begin, definitions, &RunGuard)
        .await;
    let mut changed_payload = begin.clone();
    changed_payload.qualification_reason.push_str(" changed");
    let changed = service
        .pipeline_run_begin(owner_context, &changed_payload, definitions, &RunGuard)
        .await;
    let other = admin::enroll_host(pools.0, None, vec![]).await.unwrap();
    assert_ne!(other.tenant_id, ids.0);
    let other_context = context(
        &other.auth,
        &format!("plan-replay-other-{}", Uuid::new_v4()),
    );
    service.open_workspace(&other_context).await.unwrap();
    let unauthorized = service
        .pipeline_run_begin(&other_context, begin, definitions, &RunGuard)
        .await;
    sqlx::query("UPDATE native_slices SET revision=$4,state='open' WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(ids.0).bind(ids.1).bind(begin.slice_id).bind(begin.slice_revision)
        .execute(pools.0).await.unwrap();
    assert_eq!(current, (begin.slice_revision + 1, "blocked".into()));
    // Replay decodes the retained origin result. The transient insertion flag is
    // serde-skipped; persisted context and delivery receipt must remain exact.
    assert!(begun.delivery_fresh, "creation inserts one receipt");
    let Ok(BeginPipelineRunOutcome::Replay(replayed)) = &replay else {
        panic!("retained replay required");
    };
    assert!(!replayed.delivery_fresh, "replay inserts no fresh receipt");
    let mut retained = begun.clone();
    retained.delivery_fresh = false;
    assert_eq!(replay, Ok(BeginPipelineRunOutcome::Replay(retained)));
    assert_eq!(changed, Err(Error::InputConflict));
    assert!(matches!(
        unauthorized,
        Err(Error::NotFound | Error::Forbidden)
    ));
    assert_eq!(artifacts(pools.0, ids.0, ids.1).await, baseline);

    // Separate owned row simulates pre-binding legacy NULL shape. It neither
    // erases the selected Slice's plan nor claims a normal unadvised Owner flow.
    let legacy_id = Uuid::new_v4();
    sqlx::query("INSERT INTO native_slices(id,tenant_id,workspace_id,scope_id,candidate_id,candidate_revision,opening_snapshot_id,title,outcome,pipeline,origin_request_id,origin_payload) SELECT $4,tenant_id,workspace_id,scope_id,$5,candidate_revision,opening_snapshot_id,title,outcome,pipeline,$6,'{}'::jsonb FROM native_slices WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3")
        .bind(ids.0).bind(ids.1).bind(begin.slice_id).bind(legacy_id)
        .bind(Uuid::new_v4()).bind(Uuid::new_v4()).execute(pools.0).await.unwrap();
    let mut legacy_begin = begin.clone();
    legacy_begin.request_id = Uuid::new_v4();
    legacy_begin.slice_id = legacy_id;
    legacy_begin.slice_revision = 1;
    legacy_begin.source_checkpoint = None;
    let BeginPipelineRunOutcome::Created(legacy) = service
        .pipeline_run_begin(owner_context, &legacy_begin, definitions, &RunGuard)
        .await
        .unwrap()
    else {
        panic!("fresh legacy-shape fixture run required");
    };
    let query: PipelineRunContextQuery =
        serde_json::from_value(json!({"run_id":legacy.run.id})).unwrap();
    let PipelineContextResponse::Current(read) = service
        .pipeline_context(owner_context, &query, &RunGuard)
        .await
        .unwrap()
    else {
        panic!("current PG context required");
    };
    for run in [&legacy.run, &read.run] {
        assert_eq!(run.selected_option_id, None);
        assert_eq!(run.verification_plan_id, None);
        assert_eq!(run.verification_plan_version, None);
        assert_eq!(run.verification_plan_digest, None);
    }
    assert_eq!(read.run.id, legacy.run.id);
    let rows = readback(pools.1, ids.0, ids.1).await;
    let row = rows["slice_pipeline_runs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|row| row["id"] == json!(legacy.run.id))
        .unwrap();
    for column in [
        "selected_option_id",
        "verification_plan_id",
        "verification_plan_version",
        "verification_plan_digest",
    ] {
        assert!(row[column].is_null(), "{column}");
    }
}
