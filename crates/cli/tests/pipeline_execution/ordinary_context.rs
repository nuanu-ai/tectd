//! Owned private-database regression fixture. Instrumentation is restored even
//! if the measured task panics; it never changes production function sources.
use super::*;
use tect_application::{Store, TransactionMode};
use tect_domain::{HostAuth, PipelineRunContext};
use tect_postgres::PgStore;

fn probe_enabled(dk2: Option<&str>, probe: Option<&str>) -> bool {
    dk2 == Some("1") && probe == Some("1")
}

#[test]
fn ordinary_probe_requires_dedicated_opt_in() {
    assert!(!probe_enabled(Some("1"), None));
    assert!(!probe_enabled(Some("1"), Some("0")));
    assert!(!probe_enabled(None, Some("1")));
    assert!(probe_enabled(Some("1"), Some("1")));
}

async fn context(
    store: &PgStore,
    auth: &HostAuth,
    native: &str,
    workspace: Uuid,
    run: Uuid,
    ordinary: bool,
) -> tect_domain::Result<Option<PipelineRunContext>> {
    let mut tx = store.begin(TransactionMode::ReadWrite).await?;
    let identity = tx.authenticate(auth).await?;
    tx.set_tenant(identity.tenant_id).await?;
    tx.lock_native_session(auth.host_id, native).await?;
    let session = tx.session(auth.host_id, native).await?.unwrap();
    let value = if ordinary {
        tx.pipeline_run_ordinary_context(workspace, identity.principal_id, session.id, run)
            .await?
    } else {
        tx.pipeline_run_context(workspace, identity.principal_id, run)
            .await?
    };
    tx.commit().await?;
    Ok(value)
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn ordinary_context_reuses_exact_creation_proof_under_existing_fence() {
    if !probe_enabled(
        std::env::var("TECT_TEST_DK2").ok().as_deref(),
        std::env::var("TECT_TEST_ORDINARY_CONTEXT_PROBE")
            .ok()
            .as_deref(),
    ) {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let admin_endpoint = url::Url::parse(&admin_url).unwrap();
    let runtime_endpoint = url::Url::parse(&runtime).unwrap();
    assert!(
        admin_endpoint
            .path()
            .trim_start_matches('/')
            .starts_with("tect_ordinary_context_"),
        "probe requires a dedicated fixture database prefix"
    );
    assert_eq!(admin_endpoint.host_str(), runtime_endpoint.host_str());
    assert_eq!(admin_endpoint.port(), runtime_endpoint.port());
    assert_eq!(admin_endpoint.path(), runtime_endpoint.path());
    for parameter in ["host", "port"] {
        let value = |url: &url::Url| {
            url.query_pairs()
                .find(|(key, _)| key == parameter)
                .map(|(_, value)| value.into_owned())
        };
        assert_eq!(value(&admin_endpoint), value(&runtime_endpoint));
    }
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let runtime_identity = PgPool::connect(&runtime).await.unwrap();
    let identity_sql = "SELECT current_database(),(SELECT oid::bigint FROM pg_database WHERE datname=current_database()),pg_postmaster_start_time()::text,coalesce(inet_server_addr()::text,'unix'),coalesce(inet_server_port(),0)";
    let admin_identity: (String, i64, String, String, i32) =
        sqlx::query_as(identity_sql).fetch_one(&pool).await.unwrap();
    let runtime_identity_value: (String, i64, String, String, i32) = sqlx::query_as(identity_sql)
        .fetch_one(&runtime_identity)
        .await
        .unwrap();
    assert_eq!(
        admin_identity, runtime_identity_value,
        "admin/runtime must reach the same fixture database and server"
    );
    assert_eq!(
        admin_identity.0,
        admin_endpoint.path().trim_start_matches('/')
    );
    runtime_identity.close().await;
    // Serialize opt-in probes only. Ordinary suites cannot opt in by DK2 alone,
    // and foreign/read-only calls remain read-only while wrappers exist.
    let mut probe_lease = pool.acquire().await.unwrap();
    sqlx::query("SELECT pg_advisory_lock(hashtextextended('ordinary-context-test-probe',0))")
        .execute(&mut *probe_lease)
        .await
        .unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("ordinary-context.sock");
    let _daemon = Daemon::start(&runtime, socket.clone()).await;
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let native = Uuid::new_v4().to_string();
    let key = format!("ordinary-context-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &config, &native, &key).await;
    client.call("open_workspace", json!({})).await;
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(enrollment.tenant_id)
            .bind(&key)
            .fetch_one(&pool)
            .await
            .unwrap();
    let reader_tag = format!("ordinary-context-reader-{}", Uuid::new_v4());
    let store = PgStore::connect(&tagged_url(&runtime, &reader_tag), 4)
        .await
        .unwrap();
    let empty = begin(&mut client, &repo, "empty-ordinary").await;
    let empty_id = Uuid::parse_str(empty["run"]["id"].as_str().unwrap()).unwrap();
    // Missing knowledge state remains missing: no ensure_state side effect.
    sqlx::query("DELETE FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2")
        .bind(enrollment.tenant_id)
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
    let old = context(
        &store,
        &enrollment.auth,
        &native,
        workspace,
        empty_id,
        false,
    )
    .await
    .unwrap();
    let new = context(&store, &enrollment.auth, &native, workspace, empty_id, true)
        .await
        .unwrap();
    assert_eq!(old, new);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(count, 0);
    for index in 0..22 {
        commit_create(
            &mut client,
            runbook("procedure", &format!("ordinary-proof-{index}")),
        )
        .await;
    }
    let run = begin(&mut client, &repo, "twenty-two").await;
    assert_eq!(
        run["knowledge_resources"]["selected"]
            .as_array()
            .unwrap()
            .len(),
        22
    );
    let run_id = Uuid::parse_str(run["run"]["id"].as_str().unwrap()).unwrap();
    let mut foreign = Mcp::start(
        &socket,
        &config,
        &Uuid::new_v4().to_string(),
        &format!("foreign-{key}"),
    )
    .await;
    foreign.call("open_workspace", json!({})).await;
    let foreign_unit = commit_create(
        &mut foreign,
        runbook("procedure", "foreign-probe-workspace"),
    )
    .await
    .receipt["applied_operations"][0]["unit_id"]
        .clone();
    let foreign_run = begin(&mut foreign, &repo, "foreign-probe").await;
    let mut publisher_client =
        Mcp::start(&socket, &config, &Uuid::new_v4().to_string(), &key).await;
    publisher_client.call("open_workspace", json!({})).await;
    let revisions: Vec<(Uuid, String, String)> = sqlx::query_as(
        "SELECT unit_id,rdf_digest,unit_iri FROM knowledge_revisions WHERE workspace_id=$1",
    )
    .bind(workspace)
    .fetch_all(&pool)
    .await
    .unwrap();
    let events: Vec<(Uuid, Value, String)> = sqlx::query_as("SELECT id,event_payload,rdf_digest FROM knowledge_publication_events WHERE workspace_id=$1")
        .bind(workspace).fetch_all(&pool).await.unwrap();

    // Test-only wrappers count actual SQL calls and exact arguments. Their
    // original SECURITY DEFINER bodies and ACLs remain intact and are restored.
    let table = format!("ordinary_proof_count_{}", Uuid::new_v4().simple());
    let mut installation = pool.begin().await.unwrap();
    sqlx::query(&format!(
        "CREATE TABLE public.{table}(workspace uuid,kind text,requests jsonb)"
    ))
    .execute(&mut *installation)
    .await
    .unwrap();
    let mut originals = Vec::new();
    for (signature, injection) in [
        (
            "public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean)",
            format!(
                "IF p_workspace='{workspace}'::uuid AND current_setting('transaction_read_only')='off' THEN INSERT INTO public.{table} VALUES(p_workspace,'scalar',jsonb_build_array(jsonb_build_object('unit_id',p_unit,'revision',p_revision,'event_id',p_event,'include_revision',p_include_revision))); END IF;"
            ),
        ),
        (
            "public.tect_dk2_native_read_batch(uuid,uuid,jsonb)",
            format!(
                "IF p_workspace='{workspace}'::uuid AND current_setting('transaction_read_only')='off' THEN INSERT INTO public.{table} VALUES(p_workspace,'batch',p_requests); END IF;"
            ),
        ),
    ] {
        let original: String = sqlx::query_scalar("SELECT pg_get_functiondef($1::regprocedure)")
            .bind(signature)
            .fetch_one(&mut *installation)
            .await
            .unwrap();
        let instrumented = original.replacen("BEGIN", &format!("BEGIN\n{injection}"), 1);
        assert!(
            !original.contains("ordinary_proof_count_"),
            "refuse leftover foreign instrumentation"
        );
        sqlx::raw_sql(&instrumented)
            .execute(&mut *installation)
            .await
            .unwrap();
        originals.push(original);
    }
    installation.commit().await.unwrap();
    let measured_pool = pool.clone();
    let measured_table = table.clone();
    let measured_originals = originals.clone();
    let measured = tokio::spawn(async move {
        let scalar = context(&store, &enrollment.auth, &native, workspace, run_id, false).await.unwrap().unwrap();
        let counts: (i64, i64) = sqlx::query_as(&format!("SELECT count(*) FILTER(WHERE kind='scalar'),count(*) FILTER(WHERE kind='batch') FROM public.{measured_table} WHERE workspace=$1"))
            .bind(workspace).fetch_one(&measured_pool).await.unwrap();
        assert_eq!(counts, (44, 0));
        sqlx::query(&format!("DELETE FROM public.{measured_table} WHERE workspace=$1")).bind(workspace).execute(&measured_pool).await.unwrap();
        let lazy = context(&store, &enrollment.auth, &native, workspace, run_id, true).await.unwrap().unwrap();
        assert_eq!(scalar, lazy, "DTO, manifest, resource status and backend receipt parity");
        let counts: (i64, i64, i64, i64) = sqlx::query_as(&format!("SELECT count(*) FILTER(WHERE kind='scalar'),count(*) FILTER(WHERE kind='batch'),sum(jsonb_array_length(requests)),count(DISTINCT requests) FROM public.{measured_table} WHERE workspace=$1"))
            .bind(workspace).fetch_one(&measured_pool).await.unwrap();
        assert_eq!(counts, (0, 22, 22, 22));
        eprintln!("ordinary_context actual SQL proof: scalar=44; lazy single-key batch=22; distinct exact request keys=22; DTO+receipt equal");
        route(&mut foreign, "query", "slice.pipeline.context", json!({"run_id":foreign_run["run"]["id"]})).await;
        route(&mut foreign, "query", "knowledge.unit", json!({"unit_id":foreign_unit,"revision":1})).await;
        let own_unit = run["knowledge_resources"]["selected"][0]["unit_id"].clone();
        route(&mut client, "query", "knowledge.unit", json!({"unit_id":own_unit,"revision":1})).await;
        let total: i64 = sqlx::query_scalar(&format!("SELECT count(*) FROM public.{measured_table}"))
            .fetch_one(&measured_pool).await.unwrap();
        assert_eq!(total, 22, "foreign workspace and read-only knowledge calls add no counter writes");

        let units: Vec<(Uuid, Uuid, String)> = sqlx::query_as("SELECT r.unit_id,r.publication_event_id,r.rdf_digest FROM knowledge_bindings b JOIN knowledge_revisions r ON r.tenant_id=b.tenant_id AND r.workspace_id=b.workspace_id AND r.unit_id=b.unit_id AND r.revision=b.revision WHERE b.tenant_id=$1 AND b.workspace_id=$2 ORDER BY b.id")
            .bind(enrollment.tenant_id).bind(workspace).fetch_all(&measured_pool).await.unwrap();
        let later_event: (Value, String) = sqlx::query_as("SELECT event_payload,rdf_digest FROM knowledge_publication_events WHERE id=$1")
            .bind(units[1].1).fetch_one(&measured_pool).await.unwrap();
        // The earlier resource digest failure must win over a later erased
        // creation proof. An eager all-resource preload would reverse this.
        sqlx::query("UPDATE knowledge_revisions SET rdf_digest=$3 WHERE workspace_id=$1 AND unit_id=$2")
            .bind(workspace).bind(units[0].0).bind("0".repeat(64)).execute(&measured_pool).await.unwrap();
        sqlx::query("UPDATE knowledge_publication_events SET payload_erased=true,event_payload=NULL,rdf_digest=NULL WHERE id=$1")
            .bind(units[1].1).execute(&measured_pool).await.unwrap();
        for ordinary in [false, true] {
            assert_eq!(context(&store, &enrollment.auth, &native, workspace, run_id, ordinary).await,
                Err(tect_domain::Error::InternalInvariant));
        }
        sqlx::query("UPDATE knowledge_revisions SET rdf_digest=$3 WHERE workspace_id=$1 AND unit_id=$2")
            .bind(workspace).bind(units[0].0).bind(&units[0].2).execute(&measured_pool).await.unwrap();
        for ordinary in [false, true] {
            assert_eq!(context(&store, &enrollment.auth, &native, workspace, run_id, ordinary).await,
                Err(tect_domain::Error::KnowledgePayloadErased));
        }
        sqlx::query("UPDATE knowledge_publication_events SET payload_erased=false,event_payload=$2,rdf_digest=$3 WHERE id=$1")
            .bind(units[1].1).bind(later_event.0).bind(later_event.1).execute(&measured_pool).await.unwrap();
        // Fresh relational identity is compared even when the creation proof
        // can be reused inside this request.
        let iri: String = sqlx::query_scalar("SELECT unit_iri FROM knowledge_revisions WHERE workspace_id=$1 AND unit_id=$2")
            .bind(workspace).bind(units[0].0).fetch_one(&measured_pool).await.unwrap();
        sqlx::query("UPDATE knowledge_revisions SET unit_iri='urn:wrong-fresh-identity' WHERE workspace_id=$1 AND unit_id=$2")
            .bind(workspace).bind(units[0].0).execute(&measured_pool).await.unwrap();
        for ordinary in [false, true] {
            assert_eq!(context(&store, &enrollment.auth, &native, workspace, run_id, ordinary).await,
                Err(tect_domain::Error::InternalInvariant));
        }
        sqlx::query("UPDATE knowledge_revisions SET unit_iri=$3 WHERE workspace_id=$1 AND unit_id=$2")
            .bind(workspace).bind(units[0].0).bind(iri).execute(&measured_pool).await.unwrap();

        // Inactive state retains the scalar fallback and exact status.
        sqlx::query("UPDATE workspace_knowledge_state SET capability_ready=false WHERE tenant_id=$1 AND workspace_id=$2")
            .bind(enrollment.tenant_id).bind(workspace).execute(&measured_pool).await.unwrap();
        assert_eq!(context(&store, &enrollment.auth, &native, workspace, run_id, false).await.unwrap(),
            context(&store, &enrollment.auth, &native, workspace, run_id, true).await.unwrap());
        sqlx::query("UPDATE workspace_knowledge_state SET capability_ready=true WHERE tenant_id=$1 AND workspace_id=$2")
            .bind(enrollment.tenant_id).bind(workspace).execute(&measured_pool).await.unwrap();

        // A concurrent publisher fence blocks the context before any proof or
        // receipt writes. Dropping the pending future releases its session lock.
        let before: i64 = sqlx::query_scalar("SELECT count(*) FROM pipeline_delivery_receipts WHERE workspace_id=$1 AND run_id=$2")
            .bind(workspace).bind(run_id).fetch_one(&measured_pool).await.unwrap();
        let mut publisher = measured_pool.begin().await.unwrap();
        sqlx::query("SELECT 1 FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2 FOR UPDATE")
            .bind(enrollment.tenant_id).bind(workspace).fetch_one(&mut *publisher).await.unwrap();
        assert!(tokio::time::timeout(std::time::Duration::from_millis(150),
            context(&store, &enrollment.auth, &native, workspace, run_id, true)).await.is_err());
        publisher.rollback().await.unwrap();
        assert_eq!(context(&store, &enrollment.auth, &native, workspace, run_id, true).await.unwrap().unwrap(), lazy);
        let after: i64 = sqlx::query_scalar("SELECT count(*) FROM pipeline_delivery_receipts WHERE workspace_id=$1 AND run_id=$2")
            .bind(workspace).bind(run_id).fetch_one(&measured_pool).await.unwrap();
        assert_eq!(before, after);

        // The counter INSERT is only for measured read-write context calls;
        // restore before the publisher's genuinely read-only knowledge reads.
        for original in measured_originals {
            sqlx::raw_sql(&original).execute(&measured_pool).await.unwrap();
        }

        // An actual independently bound publication waits for the context's
        // workspace fence. After release its new generation is observed fresh.
        let mut held = store.begin(TransactionMode::ReadWrite).await.unwrap();
        let identity = held.authenticate(&enrollment.auth).await.unwrap();
        held.set_tenant(identity.tenant_id).await.unwrap();
        held.lock_native_session(enrollment.auth.host_id, &native).await.unwrap();
        let session = held.session(enrollment.auth.host_id, &native).await.unwrap().unwrap();
        let held_context = held.pipeline_run_ordinary_context(workspace, identity.principal_id, session.id, run_id).await.unwrap().unwrap();
        assert_eq!(held_context, lazy);
        let mut publication = tokio::spawn(async move {
            commit_create(&mut publisher_client, runbook("procedure", "concurrent-publication")).await
        });
        assert!(tokio::time::timeout(std::time::Duration::from_millis(150), &mut publication).await.is_err());
        tokio::time::timeout(std::time::Duration::from_secs(5), async {
            loop {
                let blocked: bool = sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_stat_activity a CROSS JOIN LATERAL unnest(pg_blocking_pids(a.pid)) blocker JOIN pg_stat_activity b ON b.pid=blocker WHERE b.application_name=$1)")
                    .bind(&reader_tag).fetch_one(&measured_pool).await.unwrap();
                if blocked { break; }
                tokio::time::sleep(std::time::Duration::from_millis(20)).await;
            }
        }).await.expect("actual publisher must wait on the context reader fence");
        held.commit().await.unwrap();
        publication.await.unwrap();
        let refreshed = context(&store, &enrollment.auth, &native, workspace, run_id, true).await.unwrap().unwrap();
        assert_ne!(refreshed.knowledge_resource_status, lazy.knowledge_resource_status);
        assert_eq!(Some(refreshed), context(&store, &enrollment.auth, &native, workspace, run_id, false).await.unwrap());
    }).await;
    let mut recovery = pool.begin().await.unwrap();
    for original in &originals {
        sqlx::raw_sql(original)
            .execute(&mut *recovery)
            .await
            .unwrap();
    }
    sqlx::query(&format!("DROP TABLE public.{table}"))
        .execute(&mut *recovery)
        .await
        .unwrap();
    recovery.commit().await.unwrap();
    for (signature, original) in [
        "public.tect_dk2_native_read(uuid,uuid,uuid,bigint,uuid,boolean)",
        "public.tect_dk2_native_read_batch(uuid,uuid,jsonb)",
    ]
    .into_iter()
    .zip(originals)
    {
        let restored: String = sqlx::query_scalar("SELECT pg_get_functiondef($1::regprocedure)")
            .bind(signature)
            .fetch_one(&pool)
            .await
            .unwrap();
        assert_eq!(
            restored, original,
            "canonical function definition restored after probe"
        );
    }
    // Restore owned fault injection even when an assertion panics, so a later
    // qualification run cannot inherit this fixture's deliberately corrupt row.
    for (unit, digest, iri) in revisions {
        sqlx::query("UPDATE knowledge_revisions SET rdf_digest=$3,unit_iri=$4 WHERE workspace_id=$1 AND unit_id=$2")
            .bind(workspace).bind(unit).bind(digest).bind(iri).execute(&pool).await.unwrap();
    }
    for (event, payload, digest) in events {
        sqlx::query("UPDATE knowledge_publication_events SET payload_erased=false,event_payload=$2,rdf_digest=$3 WHERE id=$1")
            .bind(event).bind(payload).bind(digest).execute(&pool).await.unwrap();
    }
    sqlx::query("UPDATE workspace_knowledge_state SET capability_ready=true WHERE workspace_id=$1")
        .bind(workspace)
        .execute(&pool)
        .await
        .unwrap();
    sqlx::query("SELECT pg_advisory_unlock(hashtextextended('ordinary-context-test-probe',0))")
        .execute(&mut *probe_lease)
        .await
        .unwrap();
    measured.unwrap();
}
