use super::*;
use std::fs;
use std::time::Instant;
use tempfile::TempDir;

fn fake_worker(body: &str) -> (TempDir, LocalEmbeddingConfig) {
    let temp = tempfile::tempdir().unwrap();
    let model = temp.path().join("model");
    fs::create_dir(&model).unwrap();
    let worker = temp.path().join("worker.py");
    fs::write(&worker, body).unwrap();
    let python = PathBuf::from("/usr/bin/python3");
    let config = LocalEmbeddingConfig::for_test(python, model, worker).unwrap();
    (temp, config)
}

fn vector_json() -> String {
    let value = 1.0f64 / (EMBEDDING_DIMENSIONS as f64).sqrt();
    serde_json::to_string(&vec![value; EMBEDDING_DIMENSIONS]).unwrap()
}

#[tokio::test]
async fn configured_venv_symlink_is_preserved_after_target_validation() {
    let Ok(python) = std::env::var("TECT_DK3_TEST_SYMLINK_PYTHON") else {
        return;
    };
    let temp = tempfile::tempdir().unwrap();
    let model = temp.path().join("model");
    fs::create_dir(&model).unwrap();
    let python = PathBuf::from(python);
    assert!(
        fs::symlink_metadata(&python)
            .unwrap()
            .file_type()
            .is_symlink()
    );
    let venv = python.parent().unwrap().parent().unwrap();
    let worker = temp.path().join("worker.py");
    let expected_prefix = fs::canonicalize(venv).unwrap();
    let expected_prefix = serde_json::to_string(expected_prefix.to_str().unwrap()).unwrap();
    fs::write(
        &worker,
        format!(
            r#"import json,sys
ok=sys.prefix=={expected_prefix}
print(json.dumps({{"protocol":"{PROTOCOL}","status":"ready","model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS}}}) if ok else "{{}}",flush=True)
"#
        ),
    )
    .unwrap();
    let config = LocalEmbeddingConfig::for_test(python.clone(), model, worker).unwrap();
    assert_eq!(config.python_executable, python);
    let mut process = WorkerProcess::start(&config).await.unwrap();
    process.stop().await;
}

#[tokio::test]
async fn persistent_worker_validates_identity_and_prefix_kind_without_inherited_secrets() {
    let body = format!(
        r#"import json,os,sys
print(json.dumps({{"protocol":"{PROTOCOL}","status":"ready","model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS}}}),flush=True)
for line in sys.stdin:
 r=json.loads(line)
 prefix=r["kind"]+": "
 ok=("PATH" not in os.environ and r["kind"] in ("query","passage") and r["text"].startswith(prefix) and not r["text"].startswith(prefix+prefix))
 print(json.dumps({{"protocol":"{PROTOCOL}","status":"ok","request_id":r["request_id"],"model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS},"embedding":{vector}}}) if ok else "{{}}",flush=True)
"#,
        vector = vector_json()
    );
    let (_temp, config) = fake_worker(&body);
    let worker = LocalKnowledgeEmbeddingWorker::new(config);
    assert_eq!(
        worker
            .embed_query("query: русский запрос")
            .await
            .unwrap()
            .len(),
        384
    );
    assert_eq!(
        worker
            .embed_title("passage: English title")
            .await
            .unwrap()
            .len(),
        384
    );
}

#[tokio::test]
async fn failed_protocol_response_kills_process_and_restarts_on_next_request() {
    let marker = tempfile::tempdir().unwrap();
    let count = marker.path().join("count");
    let body = format!(
        r#"import json,os,sys
p={path:?}; n=int(open(p).read())+1 if os.path.exists(p) else 1; open(p,"w").write(str(n))
print(json.dumps({{"protocol":"{PROTOCOL}","status":"ready","model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS}}}),flush=True)
for line in sys.stdin:
 r=json.loads(line)
 if n==1: print(json.dumps({{"protocol":"wrong","status":"error","code":"bad"}}),flush=True)
 else: print(json.dumps({{"protocol":"{PROTOCOL}","status":"ok","request_id":r["request_id"],"model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS},"embedding":{vector}}}),flush=True)
"#,
        path = count,
        vector = vector_json()
    );
    let (_temp, config) = fake_worker(&body);
    let worker = LocalKnowledgeEmbeddingWorker::new(config);
    assert!(worker.embed_query("query: first").await.is_err());
    assert!(worker.embed_query("query: second").await.is_ok());
    assert_eq!(fs::read_to_string(count).unwrap(), "2");
}

#[tokio::test]
async fn queued_call_has_its_own_bounded_provider_slot_wait() {
    let body = format!(
        r#"import json,sys,time
print(json.dumps({{"protocol":"{PROTOCOL}","status":"ready","model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS}}}),flush=True)
for line in sys.stdin:
 r=json.loads(line); time.sleep(2)
 print(json.dumps({{"protocol":"{PROTOCOL}","status":"ok","request_id":r["request_id"],"model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS},"embedding":{vector}}}),flush=True)
"#,
        vector = vector_json()
    );
    let (_temp, config) = fake_worker(&body);
    let worker = Arc::new(LocalKnowledgeEmbeddingWorker::new(config));
    let first = {
        let worker = worker.clone();
        tokio::spawn(async move { worker.embed_query("query: first").await })
    };
    tokio::time::sleep(Duration::from_millis(100)).await;
    let started = Instant::now();
    assert_eq!(
        worker.embed_query("query: queued").await,
        Err(Error::TransportUnavailable)
    );
    assert!(started.elapsed() < Duration::from_millis(1500));
    assert!(first.await.unwrap().is_ok());
}

#[tokio::test]
async fn one_deadline_covers_the_complete_worker_exchange() {
    let body = format!(
        r#"import json,sys,time
print(json.dumps({{"protocol":"{PROTOCOL}","status":"ready","model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS}}}),flush=True)
for line in sys.stdin:
 time.sleep(2)
"#
    );
    let (_temp, config) = fake_worker(&body);
    let mut process = WorkerProcess::start(&config).await.unwrap();
    let started = Instant::now();
    let result = tokio::time::timeout(
        Duration::from_millis(200),
        process.embed_with_timeout(EmbeddingKind::Query, "query: bounded", Uuid::new_v4()),
    )
    .await;
    assert!(result.is_err());
    assert!(started.elapsed() < Duration::from_secs(1));
    process.stop().await;
}

#[tokio::test]
async fn provider_requires_backend_prefix_and_digest() {
    let body = format!(
        r#"import json,sys
print(json.dumps({{"protocol":"{PROTOCOL}","status":"ready","model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS}}}),flush=True)
for line in sys.stdin:
 r=json.loads(line)
 print(json.dumps({{"protocol":"{PROTOCOL}","status":"ok","request_id":r["request_id"],"model":"{EMBEDDING_MODEL}","revision":"{EMBEDDING_REVISION}","recipe":"{EMBEDDING_RECIPE}","dimensions":{EMBEDDING_DIMENSIONS},"embedding":{vector}}}),flush=True)
"#,
        vector = vector_json()
    );
    let (_temp, config) = fake_worker(&body);
    let provider = LocalKnowledgeEmbeddingWorker::new(config);
    let request = |text: &str, digest: String| KnowledgeEmbeddingRequest {
        request_id: Uuid::new_v4(),
        purpose: KnowledgeEmbeddingPurpose::Query,
        text: text.into(),
        input_digest: digest,
        model: KnowledgeEmbeddingModelIdentity::pinned(),
    };
    let text = "query: exact input";
    assert!(
        KnowledgeEmbeddingProvider::embed(
            &provider,
            &request(text, format!("{:x}", Sha256::digest(text.as_bytes())))
        )
        .await
        .is_ok()
    );
    assert!(
        KnowledgeEmbeddingProvider::embed(
            &provider,
            &request(
                "exact input",
                format!("{:x}", Sha256::digest(b"exact input"))
            )
        )
        .await
        .is_err()
    );
    assert!(
        KnowledgeEmbeddingProvider::embed(&provider, &request(text, "0".repeat(64)))
            .await
            .is_err()
    );
}

#[tokio::test]
async fn input_and_vector_validation_fail_closed() {
    let bad = vec![0.0; EMBEDDING_DIMENSIONS];
    assert!(!valid_embedding(&bad));
    assert!(validate_text("").is_err());
    assert!(validate_text(&"x".repeat(MAX_TEXT_BYTES + 1)).is_err());
    let (_temp, config) = fake_worker("print('{}', flush=True)\n");
    assert!(
        LocalKnowledgeEmbeddingWorker::new(config)
            .embed_query("query")
            .await
            .is_err()
    );
}

#[tokio::test]
async fn pinned_offline_assets_produce_a_normalized_embedding_when_enabled() {
    if std::env::var("TECT_DK3_TEST_EMBEDDING").as_deref() != Ok("1") {
        return;
    }
    let config = LocalEmbeddingConfig::from_env().unwrap().unwrap();
    let provider = LocalKnowledgeEmbeddingWorker::new(config);
    let text = "query: проверка локальной модели";
    let response = KnowledgeEmbeddingProvider::embed(
        &provider,
        &KnowledgeEmbeddingRequest {
            request_id: Uuid::new_v4(),
            purpose: KnowledgeEmbeddingPurpose::Query,
            text: text.into(),
            input_digest: format!("{:x}", Sha256::digest(text.as_bytes())),
            model: KnowledgeEmbeddingModelIdentity::pinned(),
        },
    )
    .await
    .unwrap();
    assert!(valid_embedding(&response));
}
