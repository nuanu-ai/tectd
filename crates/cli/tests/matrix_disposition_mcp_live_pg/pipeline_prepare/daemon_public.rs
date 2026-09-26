use super::*;
use std::{
    fs,
    os::unix::fs::{FileTypeExt, PermissionsExt},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    net::TcpListener,
    process::{Child, Command},
};

type DispatchAudit = (
    String,
    String,
    String,
    Vec<u8>,
    Vec<u8>,
    String,
    Option<i64>,
    Option<i64>,
);

pub(super) async fn exercise(fixture: &NoCallFixture<'_>) {
    let listener = TcpListener::bind("127.0.0.1:0").await.unwrap();
    let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let socket = fixture.root.join("pipeline-daemon.sock");
    let mut daemon = start_daemon(fixture, &socket, None).await;
    let mut owner = Mcp::start(
        &socket,
        fixture.owner_config,
        &Uuid::new_v4().to_string(),
        fixture.workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    let off = prepare(&mut owner, fixture, "pipeline-daemon-off").await;
    assert_eq!(off["state"], "no_call", "{off}");
    let off_id = Uuid::parse_str(off["opportunity_id"].as_str().unwrap()).unwrap();
    let result = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        json!({"opportunity_id":off_id}),
    )
    .await;
    assert_eq!(result["status"], "no_call");
    assert_eq!(dispatch_count(fixture, off_id).await, 0);
    super::http_public::assert_no_http(&listener).await;
    owner.finish().await;
    stop_daemon(&mut daemon, &socket).await;

    let policy = explicit_fixture_policy(fixture.task);
    let keypair = Ed25519KeyPair::from_seed_unchecked(&[7_u8; 32]).unwrap();
    let public_key_hex: String = keypair
        .public_key()
        .as_ref()
        .iter()
        .map(|byte| format!("{byte:02x}"))
        .collect();
    let owner_keys = json!([{
        "workspace_id":fixture.workspace,
        "owner_id":fixture.owner_id,
        "public_key_hex":public_key_hex
    }]);
    let mut daemon = start_daemon(fixture, &socket, Some((&endpoint, &policy, &owner_keys))).await;
    let mut owner = Mcp::start(
        &socket,
        fixture.owner_config,
        &Uuid::new_v4().to_string(),
        fixture.workspace_key,
    )
    .await;
    owner.call("open_workspace", json!({})).await;
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":3,"mode":"optional",
            "provider_profile_ref":{"id":"fixture-systemone"},
            "model_configuration":{"model":"jev-1.13.0"}
        }),
    )
    .await;
    let prepared = prepare(&mut owner, fixture, "pipeline-daemon-on").await;
    assert_eq!(prepared["state"], "prepared", "{prepared}");
    let opportunity = Uuid::parse_str(prepared["opportunity_id"].as_str().unwrap()).unwrap();
    let manifest_value: Value = sqlx::query_scalar(
        "SELECT manifest_payload FROM pipeline_advice_contexts WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(fixture.workspace)
    .bind(opportunity)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    let manifest: PipelineRecommendationManifest = serde_json::from_value(manifest_value).unwrap();
    assert_eq!(manifest.matrix_task_id, fixture.task.to_string());
    assert_eq!(
        manifest.compatibility_policy_digest,
        policy.digest().unwrap()
    );
    let frozen = tect_host::jev_pipeline_recommendation::prepare_native_request(
        "jev-1.13.0",
        &manifest,
        tect_host::jev_pipeline_recommendation::MAX_REQUEST_BYTES,
    )
    .unwrap();
    let raw_response = super::http_public::native_response(&frozen.body, &frozen.eligible_ids);
    assert_eq!(dispatch_count(fixture, opportunity).await, 0);
    super::http_public::assert_no_http(&listener).await;

    let response_for_stub = raw_response.clone();
    let audit_pool = fixture.pool.clone();
    let workspace = fixture.workspace;
    let stub = tokio::spawn(async move {
        let (mut connection, _) = listener.accept().await.unwrap();
        let committed: (String, bool) = sqlx::query_as(
            "SELECT state,send_started_at IS NOT NULL FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
        )
        .bind(workspace)
        .bind(opportunity)
        .fetch_one(&audit_pool)
        .await
        .unwrap();
        assert_eq!(committed, ("sending".into(), true));
        let mut request = Vec::new();
        let mut buffer = [0_u8; 4096];
        let head_end = loop {
            let read = connection.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
            if let Some(index) = request.windows(4).position(|bytes| bytes == b"\r\n\r\n") {
                break index + 4;
            }
        };
        let headers = String::from_utf8_lossy(&request[..head_end]);
        let length: usize = headers
            .lines()
            .find_map(|line| {
                let (name, value) = line.split_once(':')?;
                name.eq_ignore_ascii_case("content-length")
                    .then(|| value.trim().parse().unwrap())
            })
            .unwrap();
        while request.len() < head_end + length {
            let read = connection.read(&mut buffer).await.unwrap();
            assert!(read > 0);
            request.extend_from_slice(&buffer[..read]);
        }
        let response_headers = format!(
            "HTTP/1.1 200 OK\r\nContent-Type: application/json\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
            response_for_stub.len()
        );
        connection
            .write_all(response_headers.as_bytes())
            .await
            .unwrap();
        connection.write_all(&response_for_stub).await.unwrap();
        (listener, request, head_end)
    });
    let run = json!({"opportunity_id":opportunity});
    let ranked = route(
        &mut owner,
        "command",
        "pipeline.recommendation.run",
        run.clone(),
    )
    .await;
    assert_eq!(ranked["status"], "ranked", "{ranked}");
    assert_eq!(ranked["ranked_ids"], prepared["eligible_option_ids"]);
    let (listener, request, head_end) = stub.await.unwrap();
    let headers = String::from_utf8_lossy(&request[..head_end]);
    assert!(headers.starts_with("POST /v1/systemone HTTP/1.1\r\n"));
    assert!(headers.lines().any(|line| {
        line.split_once(':').is_some_and(|(name, value)| {
            name.eq_ignore_ascii_case("authorization") && value.trim() == "Bearer fixture-only-key"
        })
    }));
    assert_eq!(&request[head_end..], frozen.body);
    let dispatch_id = Uuid::parse_str(ranked["dispatch_id"].as_str().unwrap()).unwrap();
    let audit: DispatchAudit = sqlx::query_as(
            "SELECT state,send_certainty,outcome,request_payload,response_payload,pipeline_response_sha256,input_tokens,output_tokens FROM advisory_dispatch WHERE workspace_id=$1 AND id=$2",
        )
        .bind(fixture.workspace)
        .bind(dispatch_id)
        .fetch_one(fixture.pool)
        .await
        .unwrap();
    assert_eq!(
        (&audit.0[..], &audit.1[..], &audit.2[..]),
        ("sealed", "sent", "provider_response")
    );
    assert_eq!(audit.3, frozen.body);
    assert_eq!(audit.4, raw_response);
    assert_eq!(audit.5, format!("{:x}", Sha256::digest(&audit.4)));
    assert_eq!((audit.6, audit.7), (Some(20), Some(30)));
    let replay = route(&mut owner, "command", "pipeline.recommendation.run", run).await;
    assert_eq!(replay, ranked);
    super::http_public::assert_no_http(&listener).await;
    assert_eq!(dispatch_count(fixture, opportunity).await, 1);
    route(
        &mut owner,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":4,"mode":"optional",
            "provider_profile_ref":{"id":PROFILE},
            "model_configuration":{"model":MODEL}
        }),
    )
    .await;
    owner.finish().await;
    stop_daemon(&mut daemon, &socket).await;
}

async fn prepare(owner: &mut Mcp, fixture: &NoCallFixture<'_>, prefix: &str) -> Value {
    let revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM slice_candidate_sets WHERE workspace_id=$1 AND id=$2",
    )
    .bind(fixture.workspace)
    .bind(fixture.set)
    .fetch_one(fixture.pool)
    .await
    .unwrap();
    route(
        owner,
        "command",
        "pipeline.recommendation.prepare",
        json!({
            "candidate_set_id":fixture.set,
            "expected_candidate_set_revision":revision,
            "work_node_id":fixture.work["id"],
            "expected_work_node_revision":fixture.work["revision"],
            "request_key":format!("{prefix}-{}", Uuid::new_v4())
        }),
    )
    .await
}

async fn dispatch_count(fixture: &NoCallFixture<'_>, opportunity: Uuid) -> i64 {
    sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE workspace_id=$1 AND opportunity_id=$2",
    )
    .bind(fixture.workspace)
    .bind(opportunity)
    .fetch_one(fixture.pool)
    .await
    .unwrap()
}

async fn start_daemon(
    fixture: &NoCallFixture<'_>,
    socket: &std::path::Path,
    enabled: Option<(&str, &PipelineCompatibilityPolicy, &Value)>,
) -> Child {
    let mut command = Command::new(env!("CARGO_BIN_EXE_tectd"));
    command
        .env_clear()
        .env("TECT_DATABASE_URL", fixture.runtime_url)
        .env("TECT_SOCKET", socket)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true);
    if let Some((endpoint, policy, keys)) = enabled {
        command
            .env("TECT_JEV_PIPELINE_ENDPOINT", endpoint)
            .env("TECT_JEV_PIPELINE_PROVIDER_PROFILE_ID", "fixture-systemone")
            .env("TECT_JEV_PIPELINE_MODEL", "jev-1.13.0")
            .env(
                "TECT_JEV_PIPELINE_COMPATIBILITY_POLICY_JSON",
                serde_json::to_string(policy).unwrap(),
            )
            .env("TECT_JEV_BUDGET_OWNER_KEYS_JSON", keys.to_string())
            .env("TYPESAFE_API_KEY", "fixture-only-key");
    }
    let mut child = command.spawn().unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(
                child.try_wait().unwrap().is_none(),
                "owned daemon exited during startup"
            );
            if fs::symlink_metadata(socket).is_ok_and(|metadata| {
                metadata.file_type().is_socket() && metadata.permissions().mode() & 0o777 == 0o600
            }) {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    child
}

async fn stop_daemon(child: &mut Child, socket: &std::path::Path) {
    let pid = child.id().unwrap();
    let signal = Command::new("kill")
        .arg("-TERM")
        .arg(pid.to_string())
        .status()
        .await
        .unwrap();
    assert!(signal.success());
    assert!(child.wait().await.unwrap().success());
    assert!(!socket.exists());
}
