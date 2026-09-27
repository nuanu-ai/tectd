//! Ignored, test-only S04 one-shot. Preflight never reads a provider key or opens HTTP.
#[allow(dead_code)]
mod recovery_support;
#[path = "anti_bloat_native_mcp/setup.rs"]
mod setup;
#[path = "anti_bloat_native_mcp/source.rs"]
mod source;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Mcp, host_file, private_temp};
use ring::signature::{Ed25519KeyPair, KeyPair};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::fs::{OpenOptionsExt, PermissionsExt},
    path::Path,
    process::Stdio,
    str::FromStr,
    sync::Arc,
};
use support::{id, repository};
use tect_application::{
    AntiBloatAttemptState, AntiBloatRankingMaterial, AntiBloatRankingProvider, Store,
    StoredAntiBloatReview, TransactionMode, WorkspaceService,
};
use tect_domain::{AdvisoryBudgetCeilings, AdvisoryBudgetPolicy};
use tect_postgres::{PgStore, admin};
use tokio::{
    net::UnixListener,
    process::{Child, Command},
};
use uuid::Uuid;

const CALL_ID: &str = "tectd-jev-s04-campaign-2026-09-28-1";
const ARTIFACT_DIR: &str =
    "/Users/tony/Work/Projects/nuanu-ai-lab/artifacts/jev-live-eval-20260919";
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const MODEL: &str = "jev-1.13.0";
const MAX_REQUEST: usize = 45_000;

fn artifact(extension: &str) -> std::path::PathBuf {
    Path::new(ARTIFACT_DIR).join(format!("{CALL_ID}.{extension}"))
}

fn exclusive(path: &Path, bytes: &[u8]) {
    assert!(path.is_absolute() && path.parent().unwrap().is_dir());
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-shot artifact already exists or cannot be created; no send");
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
    assert_eq!(fs::read(path).unwrap(), bytes);
    assert_eq!(
        fs::metadata(path).unwrap().permissions().mode() & 0o777,
        0o600
    );
}

fn confirmed(reader: &mut impl BufRead, digest: &str) -> bool {
    let mut line = String::new();
    reader.read_line(&mut line).is_ok()
        && line
            .strip_suffix('\n')
            .is_some_and(|s| s.strip_suffix('\r').unwrap_or(s) == format!("SEND JEV {digest}"))
}

#[test]
fn one_shot_fence_rejects_replay_and_wrong_confirmation() {
    let temp = private_temp();
    let path = temp.path().join("used");
    assert!(!confirmed(&mut std::io::Cursor::new("SEND JEV abc"), "abc"));
    assert!(!confirmed(
        &mut std::io::Cursor::new("SEND JEV wrong\n"),
        "abc"
    ));
    assert!(confirmed(
        &mut std::io::Cursor::new("SEND JEV abc\n"),
        "abc"
    ));
    exclusive(&path, b"call_id=test\nrequest_sha256=abc\n");
    assert!(std::panic::catch_unwind(|| exclusive(&path, b"replacement")).is_err());
}

async fn identity(pool: &PgPool) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let expected = (
        180006_i32,
        std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap(),
        std::env::var("TECT_TEST_EXPECTED_DB_OID")
            .unwrap()
            .parse::<i64>()
            .unwrap(),
        std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap(),
    );
    assert!(expected.1.starts_with("tect_s04_live_"));
    let actual: (i32,String,i64,String) = sqlx::query_as("SELECT current_setting('server_version_num')::integer,current_database(),(SELECT oid::bigint FROM pg_database WHERE datname=current_database()),(SELECT system_identifier::text FROM pg_control_system())")
        .fetch_one(pool).await.unwrap();
    assert_eq!(actual, expected);
}

async fn call(pool: &PgPool, client: &mut Mcp, kind: &str, route: &str, params: Value) -> Value {
    identity(pool).await;
    support::route(client, kind, route, params).await
}

fn config(key: String) -> tect_host::JevAntiBloatProvider {
    tect_host::JevAntiBloatProvider::new(
        tect_host::JevAntiBloatConfig {
            profile: CALL_ID.into(),
            endpoint: ENDPOINT.parse().unwrap(),
            model: MODEL.into(),
            timeout: std::time::Duration::from_secs(10),
            maximum_request_bytes: MAX_REQUEST,
            maximum_response_bytes: 65_536,
        },
        key,
    )
    .unwrap()
}

async fn fresh_database() -> (PgPool, String) {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(role, "tect_ci");
    let expected_port: u16 = std::env::var("TECT_TEST_EXPECTED_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    let expected_db = std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap();
    for (url, user) in [(&admin_url, "postgres"), (&runtime_url, "tect_ci")] {
        let parsed = PgConnectOptions::from_str(url).unwrap();
        assert_eq!(parsed.get_username(), user);
        assert_eq!(parsed.get_database(), Some(expected_db.as_str()));
        assert_eq!(parsed.get_host(), "127.0.0.1");
        assert_eq!(parsed.get_port(), expected_port);
        assert!(parsed.get_socket().is_none());
    }
    let pool = PgPool::connect(&admin_url).await.unwrap();
    identity(&pool).await;
    let residue: i64 = sqlx::query_scalar("SELECT (SELECT count(*) FROM pg_namespace WHERE nspname NOT IN ('pg_catalog','pg_toast','public','information_schema')) + (SELECT count(*) FROM pg_class WHERE relnamespace='public'::regnamespace) + (SELECT count(*) FROM pg_proc WHERE pronamespace='public'::regnamespace) + (SELECT count(*) FROM pg_extension WHERE extname <> 'plpgsql') + (SELECT count(*) FROM pg_event_trigger)")
        .fetch_one(&pool).await.unwrap();
    assert_eq!(residue, 0, "S04 database must be new and empty");
    let runtime = PgPool::connect(&runtime_url).await.unwrap();
    let runtime_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime)
        .await
        .unwrap();
    assert_eq!(runtime_user, role);
    runtime.close().await;
    admin::migrate(&pool, &role).await.unwrap();
    identity(&pool).await;
    (pool, runtime_url)
}

async fn signed_budget(
    pool: &PgPool,
    store: &PgStore,
    auth: &tect_domain::HostAuth,
    tenant: Uuid,
    workspace: Uuid,
    actor: Uuid,
) -> Value {
    let key = Ed25519KeyPair::from_seed_unchecked(&[94u8; 32]).unwrap();
    let hex = |bytes: &[u8]| bytes.iter().map(|b| format!("{b:02x}")).collect::<String>();
    let keys = json!([{"workspace_id":workspace,"owner_id":actor,"public_key_hex":hex(key.public_key().as_ref())}]);
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let ceiling = AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 24_000,
        output_tokens: 2_000,
        request_utf8_bytes: MAX_REQUEST as i64,
        elapsed_monotonic_ms: 60_000,
        retry_dispatches: 1,
    };
    let id = Uuid::new_v4();
    let from = now - 60_000;
    let until = now + 600_000;
    let unsigned = AdvisoryBudgetPolicy::new(
        id,
        1,
        AdvisoryBudgetPolicy::digest_for(id, 1, from, until, ceiling),
        from,
        until,
        ceiling,
        actor,
        "0".repeat(128),
    )
    .unwrap();
    let signed = AdvisoryBudgetPolicy::new(
        id,
        1,
        unsigned.digest().into(),
        from,
        until,
        ceiling,
        actor,
        hex(key
            .sign(&unsigned.approval_signing_message(workspace).unwrap())
            .as_ref()),
    )
    .unwrap();
    identity(pool).await;
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(auth).await.unwrap();
    tx.set_tenant(tenant).await.unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(workspace, &signed)
        .await
        .unwrap();
    tx.commit().await.unwrap();
    keys
}

async fn daemon(runtime: &str, socket: &Path, keys: &Value, send_key: Option<String>) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
    command
        .env_clear()
        .env("TECT_DATABASE_URL", runtime)
        .env("TECT_SOCKET", socket)
        .env("TECT_JEV_BUDGET_OWNER_KEYS_JSON", keys.to_string())
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some(key) = send_key {
        command
            .env("TECT_JEV_ANTI_BLOAT_ENDPOINT", ENDPOINT)
            .env("TECT_JEV_ANTI_BLOAT_PROVIDER_PROFILE_ID", CALL_ID)
            .env("TECT_JEV_ANTI_BLOAT_MODEL", MODEL)
            .env("TYPESAFE_API_KEY", key);
    }
    let mut child = command.spawn().unwrap();
    tokio::time::timeout(std::time::Duration::from_secs(5), async {
        loop {
            assert!(child.try_wait().unwrap().is_none(), "test daemon exited");
            if fs::symlink_metadata(socket).is_ok_and(|m| m.permissions().mode() & 0o777 == 0o600) {
                break;
            }
            tokio::time::sleep(std::time::Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    child
}

#[tokio::test(flavor = "multi_thread", worker_threads = 4)]
#[ignore = "explicit JEV_S04_ONE_SHOT_MODE=preflight or send; fresh pinned PG18 only"]
async fn one_shot_campaign_anti_bloat() {
    let mode = std::env::var("JEV_S04_ONE_SHOT_MODE").expect("preflight or send required");
    assert!(matches!(mode.as_str(), "preflight" | "send"));
    let request_path = artifact(if mode == "send" {
        "request.json"
    } else {
        "preflight.request.json"
    });
    let manifest_path = artifact(if mode == "send" {
        "manifest.json"
    } else {
        "preflight.manifest.json"
    });
    let marker_path = artifact("used");
    assert!(!request_path.exists() && !manifest_path.exists() && !marker_path.exists());
    let (pool, runtime) = fresh_database().await;
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("seed.sock");
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let service = Arc::new(WorkspaceService::new(
        Arc::new(store.clone()),
        Arc::new(tect_host::GitSourceInspector),
        Arc::new(tect_host::LocalSetupFiles),
    ));
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let enrolled = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let host = root.join("host.json");
    host_file(&host, &enrolled.auth);
    let native = Uuid::new_v4().to_string();
    let workspace_key = format!("s04-live-{}", Uuid::new_v4());
    let mut client = Mcp::start(&socket, &host, &native, &workspace_key).await;
    let opened = call(&pool, &mut client, "command", "workspace.open", json!({})).await;
    let workspace = id(&opened["workspace"]["id"]);
    call(
        &pool,
        &mut client,
        "command",
        "workspace.advisory.configure",
        json!({
        "expected_revision":0,"mode":"optional","provider_profile_ref":{"id":CALL_ID},
        "model_configuration":{"model":MODEL}}),
    )
    .await;
    let actor: Uuid = sqlx::query_scalar("SELECT principal_id FROM hosts WHERE id=$1")
        .bind(enrolled.auth.host_id)
        .fetch_one(&pool)
        .await
        .unwrap();
    let (candidate_set, revision) = setup::selected_rankable(
        &pool,
        &store,
        &enrolled.auth,
        enrolled.tenant_id,
        actor,
        workspace,
        &native,
        &mut client,
        &repo,
    )
    .await;
    let keys = signed_budget(
        &pool,
        &store,
        &enrolled.auth,
        enrolled.tenant_id,
        workspace,
        actor,
    )
    .await;
    let prepared = call(
        &pool,
        &mut client,
        "command",
        "scope.anti_bloat.prepare",
        json!({"candidate_set_id":candidate_set,"expected_revision":revision}),
    )
    .await;
    assert_eq!(prepared["state"]["status"], "prepared");
    let review_id = id(&prepared["review_id"]);
    let findings = prepared["findings"].as_array().unwrap();
    assert_eq!(findings.len(), 3);
    assert_eq!(findings.iter().filter(|f| f["rankable"] == true).count(), 2);
    assert_eq!(
        findings.iter().filter(|f| f["rankable"] == false).count(),
        1
    );
    let selected_draft: Value = sqlx::query_scalar(
        "SELECT payload FROM scope_candidate_drafts WHERE candidate_set_id=$1 AND set_revision=$2",
    )
    .bind(candidate_set)
    .bind(revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    let candidates = selected_draft["candidates"].as_array().unwrap();
    assert_eq!(candidates.len(), 3);
    let titles = candidates
        .iter()
        .map(|c| c["title"].as_str().unwrap())
        .collect::<Vec<_>>();
    assert!(titles.contains(&"Required 600px campaign Preview"));
    assert!(titles.contains(&"Optional 375/600px comparison toggle with synchronized scrolling"));
    assert!(titles.contains(&"Optional nonblocking copy-advice checklist"));
    let (input,review,eligible): (Value,Value,Value) = sqlx::query_as(
        "SELECT input_payload,review_payload,eligible_ids FROM scope_anti_bloat_reviews WHERE review_id=$1")
        .bind(review_id).fetch_one(&pool).await.unwrap();
    let saved = StoredAntiBloatReview {
        review_id,
        workspace_id: workspace,
        actor_id: actor,
        input: serde_json::from_value(input.clone()).unwrap(),
        review: serde_json::from_value(review.clone()).unwrap(),
        state: AntiBloatAttemptState::Prepared,
    };
    let eligible_ids: Vec<String> = serde_json::from_value(eligible.clone()).unwrap();
    assert_eq!(eligible_ids.len(), 2);
    let exact = config("preflight-placeholder-never-sent".into())
        .prepare(&AntiBloatRankingMaterial {
            saved: &saved,
            eligible_ids: &eligible_ids,
        })
        .unwrap();
    assert!(!exact.is_empty() && exact.len() < MAX_REQUEST);
    let digest = format!("{:x}", Sha256::digest(&exact));
    let manifest = json!({"call_id":CALL_ID,"workspace_id":workspace,"candidate_set_id":candidate_set,
        "candidate_set_revision":revision,"review_id":review_id,"input":input,"review":review,
        "eligible_ids":eligible,"request_bytes":exact.len(),"request_sha256":digest,
        "endpoint":ENDPOINT,"model":MODEL,"budget":{"provider_calls":1,"input_tokens":24000,
        "output_tokens":2000,"request_utf8_bytes":MAX_REQUEST}});
    exclusive(&request_path, &exact);
    exclusive(
        &manifest_path,
        &serde_json::to_vec_pretty(&manifest).unwrap(),
    );
    println!(
        "S04 call_id={CALL_ID} workspace={workspace} candidate_set={candidate_set} review={review_id} request_bytes={} request_sha256={digest} manifest={}",
        exact.len(),
        manifest_path.display()
    );
    client.finish().await;
    server.abort();
    drop(server);
    let native_socket = root.join("native.sock");
    if mode == "preflight" {
        let mut child = daemon(&runtime, &native_socket, &keys, None).await;
        let mut client = Mcp::start(&native_socket, &host, &native, &workspace_key).await;
        let result = call(
            &pool,
            &mut client,
            "command",
            "scope.anti_bloat.run",
            json!({"review_id":review_id}),
        )
        .await;
        assert_eq!(result["state"]["status"], "no_call");
        assert_eq!(result["state"]["reason"], "provider_unconfigured");
        let attempts: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM scope_anti_bloat_budget_reservations WHERE review_id=$1",
        )
        .bind(review_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(attempts, 0);
        assert!(!marker_path.exists());
        println!("S04 zero-send preflight no_call review={review_id} request_sha256={digest}");
        client.finish().await;
        child.kill().await.unwrap();
        child.wait().await.unwrap();
        return;
    }
    // A send run constructs the bearer only after the exact request is frozen.
    let key = std::env::var("TYPESAFE_API_KEY").expect("send needs private process key");
    assert!(!key.trim().is_empty());
    let real = config(key.clone());
    assert_eq!(
        real.prepare(&AntiBloatRankingMaterial {
            saved: &saved,
            eligible_ids: &eligible_ids
        })
        .unwrap(),
        exact
    );
    eprintln!(
        "Review {} and {}. Enter exactly: SEND JEV {digest}",
        request_path.display(),
        manifest_path.display()
    );
    assert!(
        confirmed(&mut std::io::stdin().lock(), &digest),
        "confirmation mismatch; no send"
    );
    exclusive(
        &marker_path,
        format!("call_id={CALL_ID}\nrequest_sha256={digest}\n").as_bytes(),
    );
    assert!(marker_path.exists());
    let mut child = daemon(&runtime, &native_socket, &keys, Some(key)).await;
    let mut client = Mcp::start(&native_socket, &host, &native, &workspace_key).await;
    let result = call(
        &pool,
        &mut client,
        "command",
        "scope.anti_bloat.run",
        json!({"review_id":review_id}),
    )
    .await;
    let replay = call(
        &pool,
        &mut client,
        "command",
        "scope.anti_bloat.run",
        json!({"review_id":review_id}),
    )
    .await;
    assert_eq!(result, replay);
    let audit: (Vec<u8>,String,Option<Vec<u8>>,Option<String>,String) = sqlx::query_as(
        "SELECT request_bytes,request_sha256,raw_response,response_sha256,state FROM scope_anti_bloat_reviews WHERE review_id=$1")
        .bind(review_id).fetch_one(&pool).await.unwrap();
    assert_eq!(audit.0, exact);
    assert_eq!(audit.1, digest);
    if let Some(raw) = &audit.2 {
        assert_eq!(audit.3, Some(format!("{:x}", Sha256::digest(raw))));
    }
    println!(
        "S04 provider result={} response_bytes={} response_sha256={:?}",
        result["state"],
        audit.2.as_ref().map_or(0, Vec::len),
        audit.3
    );
    // Rank-only wire has no narrative. Caller removes only a top eligible finding.
    if audit.4 == "ranked" {
        let ranked = result["state"]["ranked_ids"]
            .as_array()
            .expect("ranked IDs");
        let top = ranked.first().and_then(Value::as_str).expect("top finding");
        assert!(eligible_ids.iter().any(|id| id == top));
        let finding = findings
            .iter()
            .find(|f| f["id"] == top && f["rankable"] == true)
            .unwrap();
        let candidate = candidates
            .iter()
            .find(|c| c["id"] == finding["candidate_id"])
            .unwrap();
        let removal = json!({"review_id":review_id,"finding_id":top,"disposition":"narrow",
            "delta":{"candidate_set_id":candidate_set,"expected_revision":revision,
                "idempotency_key":format!("s04-one-shot-{review_id}"),"operations":[
                    {"operation":"candidate.remove","candidate_id":finding["candidate_id"],
                     "expected_revision":candidate["revision"]}]}});
        let applied = call(
            &pool,
            &mut client,
            "command",
            "scope.anti_bloat.apply",
            removal,
        )
        .await;
        assert_eq!(applied["from_revision"], revision);
        assert_eq!(applied["to_revision"], revision + 1);
        let verifier = admin::prepare_verifier_enrollment(&pool, enrolled.tenant_id, workspace)
            .await
            .unwrap()
            .try_commit()
            .await
            .unwrap();
        assert_ne!(verifier.principal_id, actor);
        let verifier_path = root.join("verifier.json");
        host_file(&verifier_path, &verifier.auth);
        let mut verifier_mcp = Mcp::start(
            &native_socket,
            &verifier_path,
            &Uuid::new_v4().to_string(),
            &workspace_key,
        )
        .await;
        call(
            &pool,
            &mut verifier_mcp,
            "command",
            "workspace.open",
            json!({}),
        )
        .await;
        let preservation = call(
            &pool,
            &mut verifier_mcp,
            "query",
            "scope.anti_bloat.preservation.get",
            json!({"review_id":review_id}),
        )
        .await;
        assert_eq!(preservation["verdict"], "pass");
        let attestation = call(
            &pool,
            &mut verifier_mcp,
            "command",
            "scope.anti_bloat.preservation.verify",
            json!({"request_id":Uuid::new_v4(),"review_id":review_id,
                "expected_evidence_digest":preservation["evidence_digest"]}),
        )
        .await;
        assert_eq!(attestation["verdict"], "pass");
        println!(
            "S04 caller_receipt={} verifier_attestation={}",
            applied, attestation
        );
        verifier_mcp.finish().await;
    } else {
        let links: i64 = sqlx::query_scalar(
            "SELECT count(*) FROM scope_anti_bloat_caller_links WHERE review_id=$1",
        )
        .bind(review_id)
        .fetch_one(&pool)
        .await
        .unwrap();
        assert_eq!(
            links, 0,
            "abstain, tie or invalid response cannot mutate plan"
        );
    }
    client.finish().await;
    child.kill().await.unwrap();
    child.wait().await.unwrap();
}
