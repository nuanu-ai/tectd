use super::*;
use std::{
    io::Write,
    os::unix::fs::{FileTypeExt, OpenOptionsExt},
    process::Stdio,
    time::Duration,
};
use tokio::{
    io::{AsyncReadExt, AsyncWriteExt},
    process::{Child, Command},
};

fn private_json(path: &std::path::Path, value: &Value) {
    let mut file = std::fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
}
pub(super) async fn daemon(
    runtime: &str,
    socket: &std::path::Path,
    endpoint: &str,
    root: &std::path::Path,
    keys: &Value,
) -> Child {
    assert!(endpoint.starts_with("http://127.0.0.1:"));
    let catalogue = catalogue();
    let catalogue_path = root.join("catalogue.json");
    private_json(
        &catalogue_path,
        &json!({"schema":catalogue.schema,"version":catalogue.version,"digest":catalogue.digest().unwrap(),"routes":catalogue.routes}),
    );
    let capabilities = ModelRouteHostCapabilities {
        schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
        version: 1,
        capabilities: vec!["model-api".into()],
    };
    let capabilities_path = root.join("capabilities.json");
    private_json(
        &capabilities_path,
        &json!({"schema":capabilities.schema,"version":capabilities.version,"digest":capabilities.digest().unwrap(),"capabilities":capabilities.capabilities}),
    );
    // Explicit test-owned daemon: no real key, dotenv inputs, other JEV tuple,
    // or background knowledge worker can be inherited from the parent.
    let mut child = Command::new(env!("CARGO_BIN_EXE_tectd"))
        .env_clear()
        .env("TECT_DATABASE_URL", runtime)
        .env("TECT_SOCKET", socket)
        .env("TECT_JEV_MODEL_ROUTE_ENDPOINT", endpoint)
        .env(
            "TECT_JEV_MODEL_ROUTE_PROVIDER_PROFILE_ID",
            "fixture-model-route",
        )
        .env("TECT_JEV_MODEL_ROUTE_MODEL", "fixture-choice-adviser")
        .env("TYPESAFE_API_KEY", "model-route-loopback-fixture-only")
        .env("TECT_JEV_BUDGET_OWNER_KEYS_JSON", keys.to_string())
        .env("TECT_MODEL_ROUTE_CATALOGUE", catalogue_path)
        .env("TECT_MODEL_ROUTE_HOST_CAPABILITIES", capabilities_path)
        .stdin(Stdio::null())
        .stdout(Stdio::null())
        .stderr(Stdio::null())
        .kill_on_drop(true)
        .spawn()
        .unwrap();
    tokio::time::timeout(Duration::from_secs(5), async {
        loop {
            assert!(child.try_wait().unwrap().is_none(), "owned daemon exited");
            if std::fs::symlink_metadata(socket)
                .is_ok_and(|m| m.file_type().is_socket() && m.permissions().mode() & 0o777 == 0o600)
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(10)).await;
        }
    })
    .await
    .unwrap();
    child
}

pub(super) async fn response(
    listener: TcpListener,
    pool: PgPool,
    workspace: Uuid,
    key: String,
    case: Case,
) -> (Vec<u8>, Vec<u8>, TcpListener) {
    let (mut stream, _) = tokio::time::timeout(Duration::from_secs(5), listener.accept())
        .await
        .unwrap()
        .unwrap();
    let mut bytes = Vec::new();
    let mut chunk = [0u8; 4096];
    let (start, length) = loop {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&chunk[..read]);
        assert!(bytes.len() < 524288);
        if let Some(end) = bytes.windows(4).position(|w| w == b"\r\n\r\n") {
            let headers = std::str::from_utf8(&bytes[..end]).unwrap();
            assert!(headers.starts_with("POST /v1/systemone HTTP/1.1"));
            assert!(
                headers
                    .to_ascii_lowercase()
                    .contains("authorization: bearer model-route-loopback-fixture-only")
            );
            let length = headers
                .lines()
                .find_map(|line| {
                    line.to_ascii_lowercase()
                        .strip_prefix("content-length:")
                        .map(|v| v.trim().parse::<usize>().unwrap())
                })
                .unwrap();
            break (end + 4, length);
        }
    };
    while bytes.len() < start + length {
        let read = stream.read(&mut chunk).await.unwrap();
        assert!(read > 0);
        bytes.extend_from_slice(&chunk[..read]);
    }
    let request = bytes[start..start + length].to_vec();
    let frozen:(String,Vec<u8>,String)=sqlx::query_as("SELECT state,request_payload,request_sha256 FROM model_route_advisory_attempts WHERE workspace_id=$1 AND preparation_request_key=$2").bind(workspace).bind(key).fetch_one(&pool).await.unwrap();
    assert_eq!(frozen.0, "send_unknown");
    assert_eq!(frozen.1, request);
    assert_eq!(frozen.2, format!("{:x}", Sha256::digest(&request)));
    let wire: Value = serde_json::from_slice(&request).unwrap();
    assert_eq!(wire["model"], "fixture-choice-adviser");
    let endpoint = format!("http://{}/v1/systemone", listener.local_addr().unwrap());
    let binding = serde_json::to_vec(&(
        endpoint.as_str(),
        "fixture-model-route",
        "fixture-choice-adviser",
        "tect.model-route-typesafe-choice/1",
    ))
    .unwrap();
    assert_eq!(
        wire["state"]["provider_binding_digest"],
        format!("{:x}", Sha256::digest(binding))
    );
    assert_eq!(
        wire["state"]["contract"],
        "tect.model-route-typesafe-choice/1"
    );
    let typed = &wire["state"]["request"];
    let routes = typed["eligible_routes"].as_array().unwrap();
    assert_eq!(routes.len(), 2);
    assert_eq!(routes[0]["id"], "route-a");
    assert_eq!(routes[1]["id"], "route-b");
    for candidate in routes {
        assert_ne!(candidate["model"], wire["model"]);
    }
    let questions = wire["questions"].as_object().unwrap();
    assert_eq!(questions.len(), 1);
    let question = &questions["model_route_order_v1"];
    assert_eq!(question["type"], "choice");
    assert_eq!(question["criteria"].as_object().unwrap().len(), 3);
    let mut raw=serde_json::to_vec(&json!({"model":"fixture-choice-adviser","answers":{"model_route_order_v1":{"type":"choice","choice":"R1","probabilities":{"R0":0.2,"R1":0.7,"ABSTAIN":0.1},"confidence":0.01}},"usage":{"input_tokens":7,"output_tokens":3}})).unwrap();
    if matches!(case, Case::Http500) {
        let mut body: Value = serde_json::from_slice(&raw).unwrap();
        body["usage"] = json!({"input_tokens":20,"output_tokens":30});
        raw = serde_json::to_vec(&body).unwrap();
    }
    if matches!(case, Case::Duplicate) {
        // Escaped spelling is the same key after JSON decoding, not a second
        // independent counter. Preserve these exact ambiguous bytes.
        raw = String::from_utf8(raw)
            .unwrap()
            .replace(
                "\"input_tokens\":7",
                "\"input_tokens\":999999,\"input_\\u0074okens\":0",
            )
            .into_bytes();
    }
    if matches!(case, Case::Oversize) {
        raw.resize(64 * 1024, b' ');
    }
    stream
        .write_all(
            format!(
                "HTTP/1.1 {} Fixture\r\nContent-Length: {}\r\nConnection: close\r\n\r\n",
                case.status(),
                raw.len() + usize::from(case.partial())
            )
            .as_bytes(),
        )
        .await
        .unwrap();
    stream.write_all(&raw).await.unwrap();
    stream.flush().await.unwrap();
    if matches!(case, Case::Oversize) {
        stream.write_all(b"x").await.unwrap();
    }
    if matches!(case, Case::Truncated) {
        tokio::time::sleep(Duration::from_millis(50)).await;
    }
    stream.shutdown().await.unwrap();
    (request, raw, listener)
}
