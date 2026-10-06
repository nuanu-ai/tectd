//! Historical data in the current isolated schema; never a public legacy begin.
use super::*;

pub(super) async fn seed(pool: &PgPool, client: &mut Mcp, scope: &Value, slice: &Value) -> Uuid {
    let definition: Value = serde_json::from_str(include_str!(
        "../../../host/src/pipeline_definitions/tests/fixtures/lightweight-tdd-0.4.0-native.skills.1.json"
    )).unwrap();
    assert_eq!(
        definition["digest"],
        "b80b3472ebf4acc38996fa1946a2fe76e1b17fbcc39c6594f87a00e63a437768"
    );
    let slice_id = Uuid::parse_str(slice["id"].as_str().unwrap()).unwrap();
    let scope_id = Uuid::parse_str(scope.as_str().unwrap()).unwrap();
    let (tenant, workspace): (Uuid, Uuid) = sqlx::query_as(
        "SELECT tenant_id,workspace_id FROM native_slices WHERE id=$1 AND scope_id=$2",
    )
    .bind(slice_id)
    .bind(scope_id)
    .fetch_one(pool)
    .await
    .unwrap();
    let state = client.call("get_state", json!({})).await;
    let actor = Uuid::parse_str(state["session"]["id"].as_str().unwrap()).unwrap();
    let actual_actor: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM agent_sessions WHERE id=$1 AND tenant_id=$2 AND workspace_id=$3)")
        .bind(actor).bind(tenant).bind(workspace).fetch_one(pool).await.unwrap();
    assert!(
        actual_actor,
        "historical actor is the authenticated onboarded session"
    );
    let run = Uuid::new_v4();
    let attempt = Uuid::new_v4();
    let output = Uuid::new_v4();
    let first = &definition["phases"][0];
    let next = &definition["phases"][1];
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO slice_pipeline_runs(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,revision,definition_kind,definition_version,definition_digest,definition,delivery_mode,qualification_reason,current_phase_id,current_phase_ordinal,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,$5,$6,2,$7,$8,$9,$10,'whole','Historical immutable retirement fixture',$11,$12,$13,$14)")
        .bind(run).bind(tenant).bind(workspace).bind(scope_id).bind(slice_id).bind(slice["revision"].as_i64().unwrap())
        .bind(definition["kind"].as_str().unwrap()).bind(definition["version"].as_str().unwrap()).bind(definition["digest"].as_str().unwrap()).bind(&definition)
        .bind(next["id"].as_str().unwrap()).bind(next["ordinal"].as_i64().unwrap() as i32).bind(Uuid::new_v4()).bind(json!({"fixture":"historical-retirement"}))
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO slice_pipeline_phase_attempts(id,tenant_id,workspace_id,run_id,phase_id,phase_ordinal,attempt,outcome,transition,actor_session_id,request_id,request_payload) VALUES($1,$2,$3,$4,$5,$6,1,'completed','continue',$7,$8,$9)")
        .bind(attempt).bind(tenant).bind(workspace).bind(run).bind(first["id"].as_str().unwrap()).bind(first["ordinal"].as_i64().unwrap() as i32).bind(actor).bind(Uuid::new_v4()).bind(json!({"fixture":"retained historical attempt"}))
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO slice_pipeline_phase_outputs(id,tenant_id,workspace_id,run_id,attempt_id,phase_id,phase_ordinal,revision,body,producer_context_id,body_digest,fields,dispositions,skill_reads,resource_reads,artifacts,validator_receipts) VALUES($1,$2,$3,$4,$5,$6,$7,1,'retained historical output','historical-retirement-fixture','a7910d5070d15bcebde635dc14a5c595d7381153e69867a57196b74eeac076ca','{}','[]','[]','[]','[]','[]')")
        .bind(output).bind(tenant).bind(workspace).bind(run).bind(attempt).bind(first["id"].as_str().unwrap()).bind(first["ordinal"].as_i64().unwrap() as i32)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("INSERT INTO slice_pipeline_output_bindings(tenant_id,workspace_id,run_id,phase_id,phase_ordinal,output_id,output_revision) VALUES($1,$2,$3,$4,$5,$6,1)")
        .bind(tenant).bind(workspace).bind(run).bind(first["id"].as_str().unwrap()).bind(first["ordinal"].as_i64().unwrap() as i32).bind(output)
        .execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    run
}

pub(super) async fn rows(pool: &PgPool, run: Uuid) -> Value {
    let row: Value =
        sqlx::query_scalar("SELECT to_jsonb(r) FROM slice_pipeline_runs r WHERE id=$1")
            .bind(run)
            .fetch_one(pool)
            .await
            .unwrap();
    let mut result = json!({"run":row});
    for (name, query) in [
        (
            "attempts",
            "SELECT to_jsonb(t) FROM slice_pipeline_phase_attempts t WHERE run_id=$1 ORDER BY id",
        ),
        (
            "outputs",
            "SELECT to_jsonb(t) FROM slice_pipeline_phase_outputs t WHERE run_id=$1 ORDER BY id",
        ),
        (
            "bindings",
            "SELECT to_jsonb(t) FROM slice_pipeline_output_bindings t WHERE run_id=$1 ORDER BY phase_id",
        ),
        (
            "inputs",
            "SELECT to_jsonb(t) FROM slice_pipeline_inputs t WHERE run_id=$1 ORDER BY id",
        ),
        (
            "pipeline_receipts",
            "SELECT to_jsonb(t) FROM slice_pipeline_receipts t WHERE run_id=$1 ORDER BY operation,request_id",
        ),
        (
            "receipts",
            "SELECT to_jsonb(t) FROM slice_pipeline_run_migrations t WHERE predecessor_run_id=$1 ORDER BY migration_id",
        ),
        (
            "successors",
            "SELECT to_jsonb(r) FROM slice_pipeline_runs r JOIN slice_pipeline_run_migrations m ON m.successor_run_id=r.id WHERE m.predecessor_run_id=$1 ORDER BY r.id",
        ),
    ] {
        let rows: Vec<Value> = sqlx::query_scalar(query)
            .bind(run)
            .fetch_all(pool)
            .await
            .unwrap();
        result[name] = json!(rows);
    }
    result
}

pub(super) async fn assert_retained(pool: &PgPool, before: &Value, run: Uuid) {
    let mut after = rows(pool, run).await;
    assert_eq!(after["run"]["status"], "superseded");
    assert_eq!(
        after["run"]["revision"],
        before["run"]["revision"].as_i64().unwrap() + 1
    );
    after["run"]["status"] = before["run"]["status"].clone();
    after["run"]["revision"] = before["run"]["revision"].clone();
    assert_eq!(after["run"], before["run"]);
    for field in ["attempts", "outputs", "bindings"] {
        assert_eq!(after[field], before[field], "retained {field}");
    }
}

pub(super) async fn assert_retired_boundaries(
    pool: &PgPool,
    client: &mut Mcp,
    context: &ResolvedPipeline,
    params: &Value,
) {
    let run = Uuid::parse_str(context.run()["id"].as_str().unwrap()).unwrap();
    let before = rows(pool, run).await;
    let write = route_error(
        client,
        "command",
        "slice.pipeline.input",
        json!({
            "request_id":Uuid::new_v4(), "run_id":context.run()["id"],
            "run_revision":context.run()["revision"], "phase_id":context.run()["current_phase_id"],
            "input":"Historical run must refuse new writes."
        }),
    )
    .await;
    assert_eq!(
        write["error"]["refusal"]["code"],
        "LEGACY_MIGRATION_REQUIRED"
    );
    assert_eq!(before, rows(pool, run).await);
    for status in ["completed", "escalated"] {
        sqlx::query("UPDATE slice_pipeline_runs SET status=$2,current_phase_id=NULL,current_phase_ordinal=NULL WHERE id=$1")
            .bind(run).bind(status).execute(pool).await.unwrap();
        let terminal_before = rows(pool, run).await;
        let mut request = params.clone();
        request["request_id"] = json!(Uuid::new_v4());
        request["idempotency_key"] = json!(format!("terminal-{}", Uuid::new_v4()));
        let denied = client.call_error("pipeline_run_migrate", request).await;
        assert_eq!(denied["error"]["code"], "forbidden");
        assert_eq!(
            terminal_before,
            rows(pool, run).await,
            "no terminal successor or receipt"
        );
    }
    sqlx::query("UPDATE slice_pipeline_runs SET status=$2,current_phase_id=$3,current_phase_ordinal=$4 WHERE id=$1")
        .bind(run).bind(before["run"]["status"].as_str().unwrap())
        .bind(before["run"]["current_phase_id"].as_str().unwrap())
        .bind(before["run"]["current_phase_ordinal"].as_i64().unwrap() as i32)
        .execute(pool).await.unwrap();
    assert_eq!(before, rows(pool, run).await);
}

/// Synthetic retained committed receipt, not a claim of a new migration/publication.
pub(super) async fn assert_old_receipt_replay(
    pool: &PgPool,
    client: &mut Mcp,
    root: &Path,
    template: &Value,
) {
    use sha2::{Digest, Sha256};
    use tect_domain::{PipelineRunMigrationCommand, PipelineRunMigrationOutcome};
    let repo = root.join("retained-receipt-source");
    repository(&repo);
    let (old, _) = run_fixture(client, &repo, pool).await;
    let predecessor = Uuid::parse_str(old.run()["id"].as_str().unwrap()).unwrap();
    let original = rows(pool, predecessor).await;
    let tenant = Uuid::parse_str(original["run"]["tenant_id"].as_str().unwrap()).unwrap();
    let workspace = Uuid::parse_str(original["run"]["workspace_id"].as_str().unwrap()).unwrap();
    let definition: Value =
        sqlx::query_scalar("SELECT definition FROM slice_pipeline_runs WHERE id=$1")
            .bind(Uuid::parse_str(template.as_str().unwrap()).unwrap())
            .fetch_one(pool)
            .await
            .unwrap();
    let successor = Uuid::new_v4();
    let command: PipelineRunMigrationCommand = serde_json::from_value(json!({
        "request_id":Uuid::new_v4(),"predecessor_run_id":predecessor,"expected_revision":2,
        "idempotency_key":format!("retained-synthetic-{}",Uuid::new_v4()),
        "successor_definition_version":definition["version"],"mappings":mapping(&old)
    }))
    .unwrap();
    command.validate().unwrap();
    let outcome = PipelineRunMigrationOutcome {
        migration_id: Uuid::new_v4(),
        predecessor_run_id: predecessor,
        successor_run_id: successor,
        predecessor_revision: 2,
        successor_definition_version: definition["version"].as_str().unwrap().into(),
        successor_definition_digest: definition["digest"].as_str().unwrap().into(),
        status: "committed".into(),
    };
    let mappings_digest = Sha256::digest(serde_json::to_vec(&command.mappings).unwrap())
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect::<String>();
    let payload = serde_json::to_value(&command).unwrap();
    let result = serde_json::to_value(&outcome).unwrap();
    let mut tx = pool.begin().await.unwrap();
    sqlx::query("INSERT INTO slice_pipeline_runs(id,tenant_id,workspace_id,scope_id,slice_id,slice_revision,definition_kind,definition_version,definition_digest,definition,delivery_mode,qualification_reason,current_phase_id,current_phase_ordinal,origin_request_id,origin_payload) VALUES($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,'whole','Synthetic retained receipt successor',$11,1,$12,$13)")
        .bind(successor).bind(tenant).bind(workspace)
        .bind(Uuid::parse_str(original["run"]["scope_id"].as_str().unwrap()).unwrap())
        .bind(Uuid::parse_str(original["run"]["slice_id"].as_str().unwrap()).unwrap())
        .bind(original["run"]["slice_revision"].as_i64().unwrap()).bind(definition["kind"].as_str().unwrap())
        .bind(&outcome.successor_definition_version).bind(&outcome.successor_definition_digest).bind(&definition)
        .bind(definition["phases"][0]["id"].as_str().unwrap()).bind(command.request_id).bind(&payload)
        .execute(&mut *tx).await.unwrap();
    sqlx::query("UPDATE slice_pipeline_runs SET status='superseded',revision=3 WHERE id=$1")
        .bind(predecessor)
        .execute(&mut *tx)
        .await
        .unwrap();
    sqlx::query("INSERT INTO slice_pipeline_run_migrations(tenant_id,workspace_id,migration_id,idempotency_key,request_payload,predecessor_run_id,successor_run_id,predecessor_revision,expected_revision,predecessor_definition_version,predecessor_definition_digest,successor_definition_version,successor_definition_digest,mappings,mappings_digest,result_payload,status) VALUES($1,$2,$3,$4,$5,$6,$7,2,2,$8,$9,$10,$11,$12,$13,$14,'committed')")
        .bind(tenant).bind(workspace).bind(outcome.migration_id).bind(&command.idempotency_key).bind(&payload)
        .bind(predecessor).bind(successor).bind(old.run()["definition_version"].as_str().unwrap())
        .bind(old.run()["definition_digest"].as_str().unwrap()).bind(&outcome.successor_definition_version)
        .bind(&outcome.successor_definition_digest).bind(serde_json::to_value(&command.mappings).unwrap())
        .bind(mappings_digest).bind(&result).execute(&mut *tx).await.unwrap();
    tx.commit().await.unwrap();
    let before = rows(pool, predecessor).await;
    let replay = client.call("pipeline_run_migrate", payload.clone()).await;
    let expected_keys: std::collections::BTreeSet<_> = [
        "migration_id",
        "predecessor_run_id",
        "successor_run_id",
        "predecessor_revision",
        "successor_definition_version",
        "successor_definition_digest",
        "status",
        "actions",
        "recommended_action",
    ]
    .into_iter()
    .collect();
    assert_eq!(
        replay
            .as_object()
            .unwrap()
            .keys()
            .map(String::as_str)
            .collect::<std::collections::BTreeSet<_>>(),
        expected_keys
    );
    assert_eq!(replay["status"], "replayed");
    assert_eq!(
        replay["actions"],
        json!([{"kind":"ready_call","tool":"query","arguments":{"route":"slice.pipeline.context","params":{"run_id":outcome.successor_run_id}}}])
    );
    assert!(replay.get("recommended_action").is_some_and(Value::is_null));
    let mut receipt = replay.clone();
    receipt.as_object_mut().unwrap().remove("actions");
    receipt
        .as_object_mut()
        .unwrap()
        .remove("recommended_action");
    let mut receipt: PipelineRunMigrationOutcome = serde_json::from_value(receipt).unwrap();
    receipt.status = "committed".into();
    let stored: PipelineRunMigrationOutcome = serde_json::from_value(result.clone()).unwrap();
    assert_eq!(
        receipt, stored,
        "original committed receipt identity retained"
    );
    assert_eq!(
        before,
        rows(pool, predecessor).await,
        "exact replay writes nothing"
    );
    let mut changed = payload;
    changed["mappings"][0]["successor_obligation_id"] = json!("changed-valid-obligation");
    let conflict = client.call_error("pipeline_run_migrate", changed).await;
    assert_eq!(conflict["error"]["code"], "input_conflict");
    assert_eq!(before, rows(pool, predecessor).await);
}

pub(super) async fn assert_superseded_write(
    pool: &PgPool,
    client: &mut Mcp,
    old: &ResolvedPipeline,
) {
    let run = Uuid::parse_str(old.run()["id"].as_str().unwrap()).unwrap();
    let before = rows(pool, run).await;
    let denied = route_error(client, "command", "slice.pipeline.input", json!({
        "request_id":Uuid::new_v4(),"run_id":old.run()["id"],"run_revision":old.run()["revision"],
        "phase_id":old.run()["current_phase_id"],"input":"No new writes to a superseded historical run."
    })).await;
    assert_eq!(
        denied["error"]["refusal"]["code"],
        "LEGACY_MIGRATION_REQUIRED"
    );
    assert_eq!(before, rows(pool, run).await);
}

pub(super) fn v07_completion(context: &ResolvedPipeline, fields: Value) -> Value {
    json!({
        "request_id":Uuid::new_v4(),
        "run_id":context.run()["id"],
        "run_revision":context.run()["revision"],
        "phase_id":context.run()["current_phase_id"],
        "outcome":"completed",
        "transition":"continue",
        "output":{
            "producer_context_id":"pipeline-run-migration-public-mcp",
            "fields":fields,
            "verdict":"pass",
            "dispositions":["satisfied"]
        }
    })
}
