use super::*;
use tect_application::request_diagnostics::{RequestTrace, scope};

fn complete(trace: &std::sync::Arc<RequestTrace>) -> Value {
    trace.finish();
    let value = trace.snapshot("test");
    assert!(serde_json::to_vec(&value).unwrap().len() <= 65536);
    assert_eq!(value["dropped_counter_labels"], 0);
    assert_eq!(value["counter_labels_complete"], true);
    assert_eq!(value["stage_aggregate_labels_complete"], true);
    eprintln!(
        "ordinary_context diagnostics counters={} cap={} dropped=0",
        value["counters"].as_object().unwrap().len(),
        value["counter_label_cap"]
    );
    value
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn failure_paths(
    store: &PgStore,
    auth: &HostAuth,
    native: &str,
    workspace: Uuid,
    run: Uuid,
    baseline: &PipelineRunContext,
    pool: &PgPool,
    original: &str,
    sequence: &str,
    reader_tag: &str,
) {
    // The candidate's bounded transport wrapper must suppress an oversized
    // triple before fetching JSON bodies, discard the candidate, then succeed
    // with the exact unchanged singleton reader result.
    sqlx::query(&format!("ALTER SEQUENCE public.{sequence} RESTART WITH 1"))
        .execute(pool)
        .await
        .unwrap();
    for overflow in [
        "to_jsonb(repeat('x',8388609))",
        "'{}'::jsonb FROM generate_series(1,8193)",
    ] {
        sqlx::query(&format!("ALTER SEQUENCE public.{sequence} RESTART WITH 1"))
            .execute(pool)
            .await
            .unwrap();
        let injection = format!(
            "IF p_workspace='{workspace}'::uuid THEN PERFORM nextval('public.{sequence}'); IF jsonb_array_length(p_requests)>1 THEN RETURN QUERY SELECT 1::bigint,(p_requests->0->>'unit_id')::uuid,(p_requests->0->>'revision')::bigint,(p_requests->0->>'event_id')::uuid,(p_requests->0->>'include_revision')::boolean,{overflow}; RETURN; END IF; END IF;"
        );
        sqlx::raw_sql(&original.replacen("BEGIN", &format!("BEGIN\n{injection}"), 1))
            .execute(pool)
            .await
            .unwrap();
        let trace = RequestTrace::new("test");
        let result = scope(
            Some(trace.clone()),
            context(store, auth, native, workspace, run, true),
        )
        .await
        .unwrap()
        .unwrap();
        assert_eq!(&result, baseline);
        let snapshot = complete(&trace);
        assert_eq!(snapshot["counters"]["proof.candidate_fallbacks"], 1);
        assert!(
            snapshot["counters"]
                .get("proof.candidate_adopted")
                .is_none()
        );
        assert_eq!(snapshot["counters"]["proof.native_batch_calls"], 23);
        assert_eq!(snapshot["counters"]["proof.candidate_returned_rows"], 0);
        let attempts: i64 =
            sqlx::query_scalar(&format!("SELECT last_value FROM public.{sequence}"))
                .fetch_one(pool)
                .await
                .unwrap();
        assert_eq!(attempts, 23);
        eprintln!(
            "ordinary_context actual overflow: {overflow} suppressed; one refused candidate plus22 singleton reads; DTO+receipt parity"
        );
        sqlx::raw_sql(original).execute(pool).await.unwrap();
    }

    // Hold a dedicated advisory barrier after nextval proves entry into the
    // vector query. Dropping the future occurs inside candidate SQL, while the
    // parent transaction is still owned by the context future.
    let barrier = i64::from_le_bytes(Uuid::new_v4().as_bytes()[..8].try_into().unwrap());
    let mut held = pool.begin().await.unwrap();
    sqlx::query("SELECT pg_advisory_xact_lock($1)")
        .bind(barrier)
        .execute(&mut *held)
        .await
        .unwrap();
    let before: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pipeline_delivery_receipts WHERE workspace_id=$1 AND run_id=$2",
    )
    .bind(workspace)
    .bind(run)
    .fetch_one(pool)
    .await
    .unwrap();
    sqlx::query(&format!("ALTER SEQUENCE public.{sequence} RESTART WITH 1"))
        .execute(pool)
        .await
        .unwrap();
    let injection = format!(
        "IF p_workspace='{workspace}'::uuid THEN PERFORM nextval('public.{sequence}'); IF jsonb_array_length(p_requests)>1 THEN PERFORM pg_advisory_xact_lock({barrier}); END IF; END IF;"
    );
    sqlx::raw_sql(&original.replacen("BEGIN", &format!("BEGIN\n{injection}"), 1))
        .execute(pool)
        .await
        .unwrap();
    let trace = RequestTrace::new("test");
    let mut pending = Box::pin(scope(
        Some(trace.clone()),
        context(store, auth, native, workspace, run, true),
    ));
    tokio::time::timeout(std::time::Duration::from_secs(5),async {
        tokio::select! {
            _ = &mut pending => panic!("candidate must wait at barrier"),
            _ = async {
                loop {
                    let entered:bool=sqlx::query_scalar(&format!("SELECT is_called FROM public.{sequence}")).fetch_one(pool).await.unwrap();
                    let blocked:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity WHERE application_name=$1 AND wait_event='advisory')").bind(reader_tag).fetch_one(pool).await.unwrap();
                    if entered && blocked { break; }
                    tokio::time::sleep(std::time::Duration::from_millis(10)).await;
                }
            } => {}
        }
    }).await.expect("candidate reaches deterministic SQL barrier");
    drop(pending);
    held.rollback().await.unwrap();
    // A later context obtains the same session/workspace fence: SQLx's deferred
    // transaction rollback completed and the request scope was discarded.
    sqlx::raw_sql(original).execute(pool).await.unwrap();
    let recovered = tokio::time::timeout(
        std::time::Duration::from_secs(5),
        context(store, auth, native, workspace, run, true),
    )
    .await
    .unwrap()
    .unwrap()
    .unwrap();
    assert_eq!(&recovered, baseline);
    let attempts: i64 = sqlx::query_scalar(&format!("SELECT last_value FROM public.{sequence}"))
        .fetch_one(pool)
        .await
        .unwrap();
    assert_eq!(attempts, 1, "cancelled candidate dispatches no fallback");
    let snapshot = complete(&trace);
    assert_eq!(snapshot["counters"]["proof.native_batch_calls"], 1);
    assert!(
        snapshot["counters"]
            .get("proof.candidate_adopted")
            .is_none()
    );
    assert!(
        snapshot["counters"]
            .get("proof.candidate_fallbacks")
            .is_none()
    );
    let after: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM pipeline_delivery_receipts WHERE workspace_id=$1 AND run_id=$2",
    )
    .bind(workspace)
    .bind(run)
    .fetch_one(pool)
    .await
    .unwrap();
    assert_eq!(before, after);
    let stuck:i64=sqlx::query_scalar("SELECT count(*) FROM pg_stat_activity WHERE application_name=$1 AND state LIKE 'idle in transaction%'").bind(reader_tag).fetch_one(pool).await.unwrap();
    assert_eq!(stuck, 0);
    eprintln!(
        "ordinary_context actual cancellation: candidate SQL barrier entered; one dispatch; no adopt/fallback/receipt; transaction+pool recover"
    );
}

#[allow(clippy::too_many_arguments)]
pub(super) async fn mixed(
    client: &mut Mcp,
    repo: &std::path::Path,
    store: &PgStore,
    auth: &HostAuth,
    native: &str,
    workspace: Uuid,
    pool: &PgPool,
    original: &str,
    table: &str,
) {
    let mut document = runbook("procedure", "mixed-creation-revalidation");
    document["planning_briefs"] = json!([{"local_id":"program","stage":"program","instruction":"Use this exact declared procedure.","purpose":"Declared planning procedure","conditions":[],"exceptions":[],"selectors":{}}]);
    // Two actual canonical bindings reference one creation key; precollection
    // must deduplicate without dropping either rendered binding.
    let mut duplicate = document["bindings"][0].clone();
    duplicate["purpose"] = json!("reference");
    document["bindings"].as_array_mut().unwrap().push(duplicate);
    let publication = commit_create(client, document.clone()).await;
    let included = publication.receipt["applied_operations"][0]["unit_id"].clone();
    let omitted:Uuid=sqlx::query_scalar("SELECT unit_id FROM knowledge_revisions WHERE workspace_id=$1 AND unit_id<>$2 ORDER BY unit_id LIMIT 1").bind(workspace).bind(Uuid::parse_str(included.as_str().unwrap()).unwrap()).fetch_one(pool).await.unwrap();
    for unit in [included.clone(), json!(omitted)] {
        let observed = route(
            client,
            "query",
            "knowledge.unit",
            json!({"unit_id":unit,"revision":1}),
        )
        .await;
        let source = json!({"kind":"snapshot","snapshot":{"title":"Fresh exact native read","uri":format!("urn:ordinary:revalidation:{}",unit.as_str().unwrap()),"text":format!("Fresh exact observation of unit {} revision {}",observed["document"]["unit_id"],observed["document"]["revision"]),"observed_at":"2026-10-04T00:00:00Z","evidence_kind":"runtime_verification"}});
        commit_single(client,SingleOperation {
            operation:"revalidate",unit_id:Some(unit),expected_revision:Some(1),expected_lifecycle:Some("active"),document:None,
            revalidation:Some(json!({"sources":[source.clone()],"evidence_basis":"Fresh exact fixture observation.","valid_until":"2030-09-14T09:00:00Z","review_due_at":"2027-09-14T09:00:00Z"})),successor:None,replacement_bindings:json!([]),sources:json!([source.clone()]),knowledge_kind:json!("procedure"),profiles:json!(["general","runbook"]),erasure:"not_required",authored_followup:false,
        }).await;
    }
    let begun = begin(client, repo, "mixed-projected-proofs").await;
    let run = Uuid::parse_str(begun["run"]["id"].as_str().unwrap()).unwrap();
    sqlx::query("UPDATE slice_pipeline_runs SET inquiry=$2 WHERE id=$1").bind(run).bind(json!({"topic_level":"program","task_context":{},"completion":{"kind":"research","allow_inconclusive":false}})).execute(pool).await.unwrap();
    let scalar = context(store, auth, native, workspace, run, false)
        .await
        .unwrap();
    let injection = format!(
        "IF p_workspace='{workspace}'::uuid AND current_setting('transaction_read_only')='off' THEN INSERT INTO public.{table} VALUES(p_workspace,'batch',p_requests); END IF;"
    );
    sqlx::raw_sql(&original.replacen("BEGIN", &format!("BEGIN\n{injection}"), 1))
        .execute(pool)
        .await
        .unwrap();
    sqlx::query(&format!("DELETE FROM public.{table}"))
        .execute(pool)
        .await
        .unwrap();
    let trace = RequestTrace::new("test");
    assert_eq!(
        scope(
            Some(trace.clone()),
            context(store, auth, native, workspace, run, true)
        )
        .await
        .unwrap(),
        scalar
    );
    let requests: Vec<Value> = sqlx::query_scalar(&format!(
        "SELECT requests FROM public.{table} WHERE workspace=$1"
    ))
    .bind(workspace)
    .fetch_all(pool)
    .await
    .unwrap();
    assert_eq!(requests.len(), 2);
    assert_eq!(requests[0].as_array().unwrap().len(), 24);
    assert_eq!(requests[1].as_array().unwrap().len(), 1);
    assert_eq!(requests[1][0]["unit_id"], included);
    assert_eq!(requests[1][0]["include_revision"], false);
    let snapshot = complete(&trace);
    assert_eq!(snapshot["counters"]["proof.candidate_keys"], 25);
    assert_eq!(snapshot["counters"]["proof.candidate_adopted"], 1);
    assert_eq!(snapshot["counters"]["proof.native_batch_calls"], 2);
    assert!(
        snapshot["counters"]
            .get("proof.candidate_fallbacks")
            .is_none()
    );
    eprintln!(
        "ordinary_context actual mixed proof:24 deduplicated creation keys +1 rendered revalidation; omitted revalidation excluded; two bindings retained; global candidate budget25; DTO+receipt parity"
    );
    sqlx::raw_sql(original).execute(pool).await.unwrap();
}
