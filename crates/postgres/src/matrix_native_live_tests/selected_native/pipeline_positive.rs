//! Positive App→generic raw SQL sealer reproducer; never a genuine provider/Owner case.
use super::*;
use tect_application::{PipelineProviderIdentity, Store};
include!("pipeline_positive_fixture.rs");

type GuardResult<T> = std::result::Result<T, String>;
type SendingRow = (Uuid, String, String, Vec<u8>);

async fn guard_tenant(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    tenant: Uuid,
) -> GuardResult<()> {
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant.to_string())
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn temporary_raw(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: (Uuid, Uuid, Uuid),
    sending: &SendingRow,
    response: &[u8],
) -> std::result::Result<sqlx::postgres::PgQueryResult, sqlx::Error> {
    sqlx::query("INSERT INTO advisory_provider_observations (tenant_id,workspace_id,opportunity_id,dispatch_id,configuration_digest,request_sha256,response_payload,response_sha256,http_status,original_input_tokens,original_output_tokens,elapsed_ms,original_transport_outcome,response_complete,original_transport_context) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,200,NULL,NULL,1,'received',true,$9)")
        .bind(ids.0).bind(ids.1).bind(ids.2).bind(sending.0).bind(&sending.1).bind(&sending.2)
        .bind(response).bind(format!("{:x}", Sha256::digest(response)))
        .bind(json!({"send_certainty":"sent","outcome":"provider_response",
            "raw_response_ref":null,"provider_failure_code":null}))
        .execute(&mut **tx).await
}

async fn probe_savepoint(tx: &mut sqlx::Transaction<'_, sqlx::Postgres>) -> GuardResult<()> {
    sqlx::query("SAVEPOINT guard_probe")
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    Ok(())
}

async fn finish_probe(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    result: std::result::Result<sqlx::postgres::PgQueryResult, sqlx::Error>,
    expected: Option<&str>,
) -> GuardResult<()> {
    let checked = match (result, expected) {
        (Ok(value), None) if value.rows_affected() == 1 => Ok(()),
        (Err(sqlx::Error::Database(value)), Some(message))
            if value.code().as_deref() == Some("23514") && value.message() == message =>
        {
            Ok(())
        }
        (result, expected) => Err(format!(
            "guard probe expected={expected:?} actual={result:?}"
        )),
    };
    // Finish the savepoint even when the assertion result is negative; outer TX is still owned.
    sqlx::query("ROLLBACK TO SAVEPOINT guard_probe")
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    sqlx::query("RELEASE SAVEPOINT guard_probe")
        .execute(&mut **tx)
        .await
        .map_err(|e| e.to_string())?;
    checked
}

fn unequal_digest(value: &str) -> String {
    if value == "0".repeat(64) {
        "1".repeat(64)
    } else {
        "0".repeat(64)
    }
}

async fn runtime_guard_probes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: (Uuid, Uuid, Uuid),
    captured: &(Vec<u8>, Vec<u8>),
) -> GuardResult<(Value, SendingRow)> {
    guard_tenant(tx, ids.0).await?;
    let baseline = rows_in(tx, ids.0, ids.1, ids.2)
        .await
        .map_err(|e| e.to_string())?;
    let sending: SendingRow = sqlx::query_as("SELECT id,configuration_digest,payload_digest,request_payload FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3 AND state='sending'")
        .bind(ids.0).bind(ids.1).bind(ids.2).fetch_one(&mut **tx).await.map_err(|e| e.to_string())?;
    if baseline["advisory_dispatch"].as_array().map(Vec::len) != Some(1)
        || baseline["advisory_provider_observations"]
            .as_array()
            .map(Vec::len)
            != Some(0)
        || sending.3 != captured.0
    {
        return Err("paused Sending/request/raw-count precondition".into());
    }
    probe_savepoint(tx).await?;
    let valid = temporary_raw(tx, ids, &sending, &captured.1).await;
    finish_probe(tx, valid, None).await?;
    for field in 0..3 {
        let mut wrong = sending.clone();
        match field {
            0 => wrong.1 = unequal_digest(&sending.1),
            1 => wrong.2 = unequal_digest(&sending.2),
            _ => {
                wrong.0 = Uuid::new_v4();
                let exists: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3)")
                    .bind(ids.0).bind(ids.1).bind(wrong.0).fetch_one(&mut **tx).await.map_err(|e| e.to_string())?;
                if exists || wrong.0 == sending.0 {
                    return Err("missing-dispatch probe identity exists".into());
                }
            }
        }
        probe_savepoint(tx).await?;
        let invalid = temporary_raw(tx, ids, &wrong, &captured.1).await;
        finish_probe(tx, invalid, Some("Matrix committed dispatch mismatch")).await?;
    }
    Ok((baseline, sending))
}

async fn admin_guard_probes(
    tx: &mut sqlx::Transaction<'_, sqlx::Postgres>,
    ids: (Uuid, Uuid, Uuid),
    sending: &SendingRow,
    response: &[u8],
) -> GuardResult<()> {
    guard_tenant(tx, ids.0).await?;
    let inserted = temporary_raw(tx, ids, sending, response)
        .await
        .map_err(|e| e.to_string())?;
    if inserted.rows_affected() != 1 {
        return Err("admin temporary raw insert did not affect one row".into());
    }
    for sql in [
        "UPDATE advisory_provider_observations SET elapsed_ms=elapsed_ms+1 WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3",
        "DELETE FROM advisory_provider_observations WHERE tenant_id=$1 AND workspace_id=$2 AND dispatch_id=$3",
    ] {
        probe_savepoint(tx).await?;
        let invalid = sqlx::query(sql)
            .bind(ids.0)
            .bind(ids.1)
            .bind(sending.0)
            .execute(&mut **tx)
            .await;
        finish_probe(tx, invalid, Some("Matrix raw observation is immutable")).await?;
    }
    probe_savepoint(tx).await?;
    let wrong_hash = unequal_digest(&format!("{:x}", Sha256::digest(response)));
    let invalid = sqlx::query("UPDATE advisory_dispatch SET response_payload=$4,pipeline_response_sha256=$5,input_tokens=NULL,output_tokens=NULL,latency_ms=1,state='sealed',send_certainty='sent',outcome='provider_response',raw_response_ref=NULL,sealed_at=pg_catalog.clock_timestamp() WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3 AND state='sending'")
        .bind(ids.0).bind(ids.1).bind(sending.0).bind(response).bind(wrong_hash).execute(&mut **tx).await;
    finish_probe(
        tx,
        invalid,
        Some("pipeline seal differs from committed raw receipt"),
    )
    .await
}

async fn rollback_guard(
    tx: &mut Option<sqlx::Transaction<'_, sqlx::Postgres>>,
    result: &mut GuardResult<()>,
) {
    if let Some(owned) = tx.take()
        && let Err(error) = owned.rollback().await
    {
        *result = Err(format!(
            "prior={result:?}; explicit rollback failed: {error}"
        ));
    }
}

async fn paused_guard_checks(
    runtime: &PgPool,
    admin: &PgPool,
    ids: (Uuid, Uuid, Uuid),
    captured: tokio::sync::oneshot::Receiver<(Vec<u8>, Vec<u8>)>,
    release: tokio::sync::oneshot::Sender<()>,
) -> GuardResult<()> {
    let started = tokio::time::Instant::now();
    let deadline = started + std::time::Duration::from_secs(3);
    let capture = tokio::time::timeout_at(deadline, captured).await;
    let mut result = Ok(());
    let mut runtime_tx = None;
    let mut admin_tx = None;
    let mut snapshot_tx = None;
    let mut material = None;
    let mut baseline = None;
    let mut sending = None;
    match capture {
        Ok(Ok(value)) => material = Some(value),
        value => result = Err(format!("guard capture unavailable: {value:?}")),
    }
    // BEGIN/acquisition is outside cancellable SQL work. Once returned, every TX has an owner
    // here and is explicitly rolled back outside the absolute work timeout before release.
    if result.is_ok() && tokio::time::Instant::now() >= deadline {
        result = Err("guard work deadline before runtime BEGIN".into());
    }
    if result.is_ok() {
        match runtime.begin().await {
            Ok(tx) => runtime_tx = Some(tx),
            Err(error) => result = Err(error.to_string()),
        }
    }
    if let (Some(tx), Some(material)) = (runtime_tx.as_mut(), material.as_ref()) {
        match tokio::time::timeout_at(deadline, runtime_guard_probes(tx, ids, material)).await {
            Ok(Ok((before, row))) => {
                baseline = Some(before);
                sending = Some(row);
            }
            Ok(Err(error)) => result = Err(error),
            Err(_) => result = Err("runtime guard absolute work deadline".into()),
        }
    }
    rollback_guard(&mut runtime_tx, &mut result).await;
    if result.is_ok() && tokio::time::Instant::now() >= deadline {
        result = Err("guard work deadline before admin BEGIN".into());
    }
    if result.is_ok() {
        match admin.begin().await {
            Ok(tx) => admin_tx = Some(tx),
            Err(error) => result = Err(error.to_string()),
        }
    }
    if let (Some(tx), Some(material), Some(sending)) =
        (admin_tx.as_mut(), material.as_ref(), sending.as_ref())
    {
        match tokio::time::timeout_at(deadline, admin_guard_probes(tx, ids, sending, &material.1))
            .await
        {
            Ok(value) => result = value,
            Err(_) => result = Err("admin guard absolute work deadline".into()),
        }
    }
    rollback_guard(&mut admin_tx, &mut result).await;
    if result.is_ok() && tokio::time::Instant::now() >= deadline {
        result = Err("guard work deadline before snapshot BEGIN".into());
    }
    if result.is_ok() {
        match runtime.begin().await {
            Ok(tx) => snapshot_tx = Some(tx),
            Err(error) => result = Err(error.to_string()),
        }
    }
    if let Some(tx) = snapshot_tx.as_mut() {
        let snapshot = async {
            guard_tenant(tx, ids.0).await?;
            rows_in(tx, ids.0, ids.1, ids.2)
                .await
                .map_err(|e| e.to_string())
        };
        match tokio::time::timeout_at(deadline, snapshot).await {
            Ok(Ok(after)) if baseline.as_ref() == Some(&after) => {}
            Ok(Ok(_)) => result = Err("five-table snapshot changed after rollback".into()),
            Ok(Err(error)) => result = Err(error),
            Err(_) => result = Err("snapshot absolute work deadline".into()),
        }
    }
    rollback_guard(&mut snapshot_tx, &mut result).await;
    // No early return (including capture failure), assertions or panic may strand the server.
    if release.send(()).is_err() {
        result = Err(format!(
            "prior={result:?}; response release receiver dropped"
        ));
    }
    println!(
        "PIPELINE_GUARDS work_deadline_ms=3000 total_acquisition_work_rollback_release_ms={} success={}",
        started.elapsed().as_millis(),
        result.is_ok()
    );
    result
}

#[tokio::test]
#[ignore = "requires disposable PostgreSQL 18.6 and separate admin/runtime; synthetic loopback HTTP only"]
async fn positive_pipeline_recommendation_retains_raw_and_replays_without_second_send() {
    tokio::time::timeout(std::time::Duration::from_secs(90), async {
        let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
        let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
        let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
        assert_ne!(admin_url, runtime_url);
        let pool = PgPool::connect(&admin_url).await.unwrap();
        admin::migrate(&pool, &role).await.unwrap();
        let version: String = sqlx::query_scalar("SHOW server_version_num").fetch_one(&pool).await.unwrap();
        assert_eq!(version, "180006");
        let runtime = PgPool::connect(&runtime_url).await.unwrap();
        let user: String = sqlx::query_scalar("SELECT current_user").fetch_one(&runtime).await.unwrap();
        assert_eq!(user, role);
        let plain = Arc::new(PgStore::connect(&runtime_url, 4).await.unwrap());
        let owner = admin::enroll_host(&pool, None, vec![]).await.unwrap();
        let owner_context = context(&owner.auth, &format!("pipeline-positive-{}", Uuid::new_v4()));
        let initial = service(Arc::clone(&plain));
        let workspace = initial.open_workspace(&owner_context).await.unwrap().workspace.unwrap().id;
        let (program, source, effective) = bound_source(&initial, &pool, &owner, &owner_context, workspace).await;
        let (planning_service, selection, matrix_sends) = matrix::selected(
            Arc::clone(&plain), &pool, &owner, &owner_context, workspace, &source, &effective).await;
        let definitions = PositiveDefinitions::new();
        let guidance = PositiveGuidance(definitions.clone());
        let planning = native::planning_with_guidance(&planning_service, &owner_context, program, &guidance).await;
        let save = native::save_request(&planning, selection);
        let saved = planning_service.save_slice_candidate_draft(&owner_context, &save, &guidance, &native::Guard).await.unwrap();
        let work = saved.draft.as_ref().unwrap().nodes[0].clone();
        let verifier = admin::prepare_verifier_enrollment(&pool, owner.tenant_id, workspace)
            .await.unwrap().try_commit().await.unwrap();
        assert_ne!(verifier.principal_id, owner.principal_id);
        let verifier_context = context(&verifier.auth, &owner_context.workspace_key);
        planning_service.open_workspace(&verifier_context).await.unwrap();
        let effect = planning_service.get_matrix_planning_effect(&verifier_context, save.candidate_set_id, save.request_id).await.unwrap();
        planning_service.verify_matrix_planning_effect(&verifier_context, &VerifyMatrixPlanningEffect {
            request_id: Uuid::new_v4(), candidate_set_id: save.candidate_set_id,
            caller_request_id: save.request_id, expected_result_revision: saved.candidate_set.revision,
            expected_effect_digest: effect.effect_digest, verdict: MatrixPlanningEffectVerdict::Matches,
            summary: "Synthetic independent saved Matrix effect".into(),
        }).await.unwrap();
        let ready = planning_service.review_slice_candidate_set(&owner_context, &ReviewSliceCandidateSet {
            scope_id: saved.scope.id, candidate_set_id: saved.candidate_set.id,
            revision: saved.candidate_set.revision, snapshot_id: saved.snapshot.id,
            input_cursor: saved.candidate_set.input_cursor, request_id: Uuid::new_v4(),
            consumed_knowledge: native::knowledge(saved.planning_knowledge.as_ref()),
            review: SliceCandidateReviewDraft { verdict: SlicePlanReviewVerdict::Ready,
                summary: "Synthetic ready fixture".into(), findings: vec![] },
        }, &guidance, &native::Guard).await.unwrap();
        let cards = planning_service.compose_matrix_cards(&owner_context, source.revision.task_id,
            source.revision.revision).await.unwrap();
        let (budget, keys) = signed_test_budget(workspace, owner.principal_id);
        let positive = Arc::new(plain.as_ref().clone().with_budget_owner_keys(keys));
        let mut unit = positive.begin(tect_application::TransactionMode::ReadWrite).await.unwrap();
        unit.authenticate(&owner.auth).await.unwrap();
        unit.set_tenant(owner.tenant_id).await.unwrap();
        unit.advisory_budget_policy_store().unwrap().install_budget_policy(workspace, &budget).await.unwrap();
        unit.commit().await.unwrap();
        // Bind endpoint first, but spawn no fake task until legal Prepared capture succeeds.
        let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
        let endpoint = reqwest::Url::parse(&format!("http://{}/v1/systemone", listener.local_addr().unwrap())).unwrap();
        let identity = PipelineProviderIdentity { provider: "synthetic-pipeline".into(),
            model: "jev-1.13.0".into(), destination: endpoint.to_string(),
            wire_version: tect_host::jev_pipeline_recommendation::WIRE_VERSION.into() };
        let provider = tect_host::jev_pipeline_recommendation::JevPipelineProvider::new(tect_host::jev_pipeline_recommendation::JevPipelineConfig {
            identity: identity.clone(), endpoint, timeout: std::time::Duration::from_secs(5),
            maximum_request_bytes: tect_host::jev_pipeline_recommendation::MAX_REQUEST_BYTES,
            maximum_response_bytes: tect_host::jev_pipeline_recommendation::MAX_RESPONSE_BYTES,
        }, "synthetic-pipeline-only".into()).unwrap();
        let positive_service = service(positive)
            .with_pipeline_recommendation_definitions(Arc::new(definitions.clone()))
            .with_pipeline_compatibility_policy(Arc::new(FixedPipelineCompatibilityPolicy(
                positive_policy(&source, &cards, &definitions))))
            .with_pipeline_recommendation_provider(Arc::new(provider));
        positive_service.configure_advisory(&owner_context, &ConfigureWorkspaceAdvisory {
            expected_revision: 1, mode: WorkspaceAdvisoryMode::Optional,
            provider_profile_ref: Some(AdvisoryProviderProfileRef { id: identity.provider.clone() }),
            model_configuration: Some(AdvisoryModelConfiguration { model: identity.model.clone() }),
        }).await.unwrap();
        let prepared = positive_service.prepare_pipeline_recommendation(&owner_context, &PreparePipelineRecommendation {
            candidate_set_id: ready.candidate_set.id, expected_candidate_set_revision: ready.candidate_set.revision,
            work_node_id: work.id(), expected_work_node_revision: work.revision(),
            request_key: format!("positive-pipeline-{}", Uuid::new_v4()),
            session_preference: AdvisoryRequestPreference::UseWorkspace,
            request_preference: AdvisoryRequestPreference::UseWorkspace,
        }).await.unwrap();
        assert_eq!(prepared.opportunity.state, AdvisoryOpportunityState::Prepared);
        assert_eq!(prepared.opportunity.primary_reason, AdvisoryReason::RecommendationPrepared);
        assert!(prepared.manifest.has_bound_v2_authority());
        assert_eq!(prepared.manifest.options.len(), 2);
        assert_ne!(prepared.manifest.options[0].id, prepared.manifest.options[1].id);
        let before = readback(&runtime, owner.tenant_id, workspace).await;
        println!("PIPELINE_POSITIVE Prepared opportunity={} options=2 config={} budget={}",
            prepared.opportunity.id, prepared.opportunity.config_revision, budget.id());
        let (done, receiver) = tokio::sync::oneshot::channel();
        let (capture_sender, capture_receiver) = tokio::sync::oneshot::channel();
        let (release_sender, release_receiver) = tokio::sync::oneshot::channel();
        let server = serve_once(listener, receiver, capture_sender, release_receiver);
        let request = RunPipelineRecommendation { opportunity_id: prepared.opportunity.id };
        let (result, guard_result) = tokio::join!(
            positive_service.run_pipeline_recommendation(&owner_context, &request),
            paused_guard_checks(&runtime, &pool, (owner.tenant_id, workspace, prepared.opportunity.id),
                capture_receiver, release_sender),
        );
        // Defer all diagnostic assertions until the reached fake task is closed/awaited.
        let observed = rows(&runtime, owner.tenant_id, workspace, prepared.opportunity.id).await;
        let replay = if result.is_ok() {
            Some(positive_service.run_pipeline_recommendation(&owner_context, &request).await)
        } else { None };
        let _ = done.send(());
        let fake_join = server.await;
        let readback_sqlstate = observed.as_ref().err().and_then(|error|
            error.as_database_error().and_then(|database| database.code()));
        println!("PIPELINE_POSITIVE FIRST app_result={result:?} readback_ok={} fake_join_ok={} fake_panicked={} fake_cancelled={} readback_sqlstate={readback_sqlstate:?}",
            observed.is_ok(), fake_join.is_ok(),
            fake_join.as_ref().err().is_some_and(|error| error.is_panic()),
            fake_join.as_ref().err().is_some_and(|error| error.is_cancelled()));
        println!("PIPELINE_POSITIVE guard_result={guard_result:?} (3s work; awaited rollback/acquisition are not a hard pause bound)");
        guard_result.expect("all paused raw/seal guard probes and cleanup must pass");
        let captured = fake_join.unwrap();
        let observed = observed.unwrap();
        let states = observed["advisory_dispatch"].as_array().unwrap().iter()
            .map(|row| json!({"id":row["id"],"state":row["state"],
                "request_sha256":row["payload_digest"]})).collect::<Vec<_>>();
        println!("PIPELINE_POSITIVE first_result={result:?} dispatch={states:?} raw_count={} completed_requests={} accepted_connections={} request_bytes={} request_sha256={:x}",
            observed["advisory_provider_observations"].as_array().unwrap().len(),
            captured.completed_requests, captured.accepted_connections,
            captured.request.len(), Sha256::digest(&captured.request));
        assert_eq!(captured.completed_requests, 1);
        assert_eq!(captured.accepted_connections, 1, "second fake connection accepted during run/replay");
        if result.is_err() {
            let body: Vec<u8> = sqlx::query_scalar("SELECT request_payload FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3")
                .bind(owner.tenant_id).bind(workspace).bind(prepared.opportunity.id).fetch_one(&pool).await.unwrap();
            assert_eq!(body, captured.request);
            panic!("positive Pipeline App→generic raw sealer must succeed; result={result:?}");
        }
        let first = result.unwrap();
        assert_eq!(replay.unwrap().unwrap(), first);
        let PipelineRecommendationRun::Ranked { opportunity_id, dispatch_id, ranked_ids } = first else {
            panic!("positive fixture must rank, not NoCall/unknown/abstain");
        };
        assert_eq!(opportunity_id, prepared.opportunity.id);
        assert_eq!(ranked_ids, prepared.manifest.options.iter().map(|value| value.id.clone()).collect::<Vec<_>>());
        let after = rows(&runtime, owner.tenant_id, workspace, opportunity_id).await.unwrap();
        assert_eq!(after, observed, "replay changed receipt/accounting rows");
        for table in ["advisory_dispatch", "advisory_provider_observations", "advisory_budget_reservations", "advisory_budget_consumptions"] {
            assert_eq!(after[table].as_array().unwrap().len(), 1);
        }
        let raw = &after["advisory_provider_observations"][0];
        let dispatch = &after["advisory_dispatch"][0];
        assert_eq!(raw["tenant_id"], json!(owner.tenant_id));
        assert_eq!(raw["workspace_id"], json!(workspace));
        assert_eq!(raw["opportunity_id"], json!(opportunity_id));
        assert_eq!(raw["dispatch_id"], json!(dispatch_id));
        assert_eq!(raw["configuration_digest"], dispatch["configuration_digest"]);
        assert_eq!(raw["request_sha256"], json!(format!("{:x}", Sha256::digest(&captured.request))));
        assert_eq!(raw["response_sha256"], json!(format!("{:x}", Sha256::digest(&captured.response))));
        assert_eq!(dispatch["pipeline_response_sha256"], raw["response_sha256"]);
        assert_eq!(raw["response_complete"], json!(true));
        assert_eq!(dispatch["provider"], json!(identity.provider));
        assert_eq!(dispatch["model"], json!(identity.model));
        assert_eq!(after["advisory_opportunity"][0]["state"], json!("advised"));
        assert_eq!(after["advisory_budget_reservations"][0]["reserved_calls"], json!(1));
        let consumed = &after["advisory_budget_consumptions"][0];
        assert_eq!(consumed["calls"], json!(1));
        assert_eq!(consumed["input_tokens"], json!(20));
        assert_eq!(consumed["output_tokens"], json!(30));
        assert_eq!(consumed["unknown_usage"], json!(false));
        assert_eq!(consumed["exhausted_after_response"], json!(false));
        let payloads: (Vec<u8>, Vec<u8>) = sqlx::query_as("SELECT d.request_payload,o.response_payload FROM advisory_dispatch d JOIN advisory_provider_observations o ON (o.tenant_id,o.workspace_id,o.dispatch_id)=(d.tenant_id,d.workspace_id,d.id) WHERE d.tenant_id=$1 AND d.workspace_id=$2 AND d.id=$3")
            .bind(owner.tenant_id).bind(workspace).bind(dispatch_id).fetch_one(&pool).await.unwrap();
        assert_eq!(payloads, (captured.request.clone(), captured.response));
        let effects_after = readback(&runtime, owner.tenant_id, workspace).await;
        for table in ["native_slices", "slice_pipeline_runs", "pipeline_open_effect_attestations"] {
            assert_eq!(before[table], effects_after[table], "advice automatically created {table}");
        }
        assert_eq!(matrix_sends.load(Ordering::SeqCst), 0);
        println!("PIPELINE_POSITIVE PASS opportunity={opportunity_id} dispatch={dispatch_id} completed_requests=1 accepted_connections=1 request_bytes={} request_sha256={:x} replay_equal=true",
            captured.request.len(), Sha256::digest(&captured.request));
    }).await.expect("bounded positive Pipeline fixture");
}
