#[path = "knowledge_search_native/corpus.rs"]
mod corpus;
#[path = "knowledge_search_native/daemon.rs"]
mod daemon;
#[path = "pipeline_execution/knowledge_lifecycle_support.rs"]
#[allow(dead_code)]
mod knowledge_lifecycle_support;
#[path = "knowledge_search_native/metrics.rs"]
mod metrics;
#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;
#[path = "knowledge_search_native/target.rs"]
mod target;

use corpus::{assert_current_results, document, result_units};
use daemon::{EmbeddingDaemon, write_worker_probe};
use knowledge_lifecycle_support::commit_create;
use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::{
    collections::{BTreeMap, BTreeSet},
    fs,
    io::Write,
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    time::{Duration, Instant},
};
use tect_domain::RequestContext;
use tect_postgres::admin;
use uuid::Uuid;

fn required_path(name: &str) -> PathBuf {
    PathBuf::from(std::env::var_os(name).unwrap_or_else(|| panic!("{name} is required")))
}

fn private_json(path: &Path, value: &Value) {
    let mut file = fs::OpenOptions::new()
        .create_new(true)
        .write(true)
        .mode(0o600)
        .open(path)
        .unwrap();
    file.write_all(&serde_json::to_vec(value).unwrap()).unwrap();
}

async fn search(client: &mut Mcp, params: Value) -> (Value, u128) {
    let started = Instant::now();
    let response = support::route(client, "query", "knowledge.search", params).await;
    (response, started.elapsed().as_millis())
}

async fn wait_for_passages(counter: &Path, count: u64) -> Vec<u128> {
    let started = Instant::now();
    let mut observed = 0;
    let mut milestones = Vec::new();
    tokio::time::timeout(Duration::from_secs(90), async {
        loop {
            if let Ok(bytes) = fs::read(counter) {
                let value: Value = serde_json::from_slice(&bytes).unwrap();
                let current = value["passage"].as_u64().unwrap();
                while observed < current {
                    observed += 1;
                    milestones.push(started.elapsed().as_millis());
                }
                if current >= count {
                    break;
                }
            }
            tokio::time::sleep(Duration::from_millis(5)).await;
        }
    })
    .await
    .unwrap();
    milestones
}

async fn counts(pool: &PgPool, tenant: Uuid, workspace: Uuid) -> Value {
    let (resources, jobs, vectors): (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT count(*) FROM knowledge_search_resources WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM knowledge_search_embedding_jobs WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM knowledge_search_vectors WHERE tenant_id=$1 AND workspace_id=$2)",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(pool)
    .await
    .unwrap();
    json!({"resources":resources,"jobs":jobs,"vectors":vectors})
}

#[tokio::test(flavor = "multi_thread", worker_threads = 6)]
async fn native_search_uses_verified_rdf_graph_lexical_and_local_vectors() {
    if std::env::var("TECT_TEST_DK3").as_deref() != Ok("1") {
        return;
    }
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let corpus_path = required_path("TECT_TEST_DK3_CORPUS");
    let python = required_path("TECT_TEST_DK3_PYTHON");
    let model = required_path("TECT_TEST_DK3_MODEL");
    let evidence = required_path("TECT_TEST_DK3_EVIDENCE");
    let fixed = corpus::load(&corpus_path);
    assert_eq!((fixed.records.len(), fixed.queries.len()), (16, 10));

    let pool = PgPool::connect(&admin_url).await.unwrap();
    admin::migrate(&pool, &role).await.unwrap();
    tect_postgres::enable_durable_knowledge(&pool, &role)
        .await
        .unwrap();
    tect_postgres::enable_knowledge_vector_search(&pool, &role)
        .await
        .unwrap();
    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    support::repository(&repo);
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config = root.join("host.json");
    host_file(&config, &enrollment.auth);
    let workspace_key = format!("dk3-native-search-{}", Uuid::new_v4());
    let native = Uuid::new_v4().to_string();

    let query_counter = root.join("query-counts.json");
    let query_probe = root.join("query-worker.py");
    write_worker_probe(&query_probe, &python, &query_counter);
    let first_socket = root.join("search-cold.sock");
    let first_runtime = tagged_url(&runtime_url, &format!("dk3-search-cold-{}", Uuid::new_v4()));
    let first_daemon = EmbeddingDaemon::start(
        &first_runtime,
        first_socket.clone(),
        &query_probe,
        &model,
        None,
    )
    .await;
    let mut first = Mcp::start(&first_socket, &config, &native, &workspace_key).await;
    let target_a = target::open(&mut first, &repo, &pool, "program-a").await;
    let target_b = target::open(&mut first, &repo, &pool, "program-b").await;
    let tenant = enrollment.tenant_id;
    let workspace: Uuid =
        sqlx::query_scalar("SELECT id FROM workspaces WHERE tenant_id=$1 AND key=$2")
            .bind(tenant)
            .bind(&workspace_key)
            .fetch_one(&pool)
            .await
            .unwrap();
    let program_a = Uuid::parse_str(target_a["program_id"].as_str().unwrap()).unwrap();
    let program_b = Uuid::parse_str(target_b["program_id"].as_str().unwrap()).unwrap();

    let listed = first.exchange("tools/list", json!({})).await;
    let tool_names = listed["result"]["tools"]
        .as_array()
        .unwrap()
        .iter()
        .map(|tool| tool["name"].as_str().unwrap())
        .collect::<BTreeSet<_>>();
    assert_eq!(
        tool_names,
        BTreeSet::from(["command", "execute", "get_state", "help", "query"])
    );
    let help = first
        .call(
            "help",
            json!({"mode":"describe","tool":"query","route":"knowledge.search"}),
        )
        .await;
    assert_eq!(help["route"], "knowledge.search");

    let mut units = BTreeMap::<String, Uuid>::new();
    for record in &fixed.records {
        let committed = commit_create(&mut first, document(record, program_a)).await;
        units.insert(
            record.id.clone(),
            Uuid::parse_str(
                committed.receipt["applied_operations"][0]["unit_id"]
                    .as_str()
                    .unwrap(),
            )
            .unwrap(),
        );
    }
    let unit_to_id = units
        .iter()
        .map(|(id, unit)| (*unit, id.as_str()))
        .collect::<BTreeMap<_, _>>();
    assert_eq!(
        counts(&pool, tenant, workspace).await,
        json!({"resources":16,"jobs":16,"vectors":0})
    );
    let database_before: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2)",
    )
    .bind(tenant)
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    let exact = fixed
        .queries
        .iter()
        .find(|query| query.id == "exact-title")
        .unwrap();
    let binding_a = json!({"kind":"program","program_id":program_a});
    let (lexical, lexical_ms) = search(
        &mut first,
        corpus::search_params(exact, "lexical", &binding_a),
    )
    .await;
    assert_current_results(&lexical);
    assert_eq!(lexical["vector_status"], "not_requested");
    assert_eq!(
        result_units(&lexical)[0],
        *units.get("resource-vector").unwrap()
    );
    assert!(!query_counter.exists());
    let (graph, graph_ms) = search(
        &mut first,
        json!({"mode":"graph_search",
        "seeds":[corpus::source_iri("resource-vector")],"relations":["derived_from"],
        "direction":"incoming","max_depth":1,"binding":binding_a,"limit":16,
        "corpus_limit":32,"purpose":"Verify native source provenance traversal."}),
    )
    .await;
    assert_current_results(&graph);
    assert_eq!(graph["vector_status"], "not_requested");
    assert!(result_units(&graph).contains(units.get("resource-vector").unwrap()));
    assert!(!query_counter.exists());
    let (wrong_program, _) = search(
        &mut first,
        corpus::search_params(
            exact,
            "lexical",
            &json!({"kind":"program","program_id":program_b}),
        ),
    )
    .await;
    assert_eq!(wrong_program["results"], json!([]));
    let (cold_vector, cold_query_ms) = search(
        &mut first,
        corpus::search_params(
            fixed
                .queries
                .iter()
                .find(|query| query.id == "ru-recovery")
                .unwrap(),
            "super_wide",
            &binding_a,
        ),
    )
    .await;
    assert_eq!(cold_vector["vector_status"], "partial");
    assert_eq!(cold_vector["metrics"]["embedding_cache_hit"], false);
    let cold_process = metrics::process_sample(&query_counter);
    let database_after: (i64, i64, i64) = sqlx::query_as(
        "SELECT (SELECT generation FROM workspace_knowledge_state WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM pipeline_knowledge_manifests WHERE tenant_id=$1 AND workspace_id=$2),\
         (SELECT count(*) FROM knowledge_change_runs WHERE tenant_id=$1 AND workspace_id=$2)",
    )
    .bind(tenant).bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(database_after, database_before);
    first.finish().await;
    first_daemon.stop().await;

    let contexts = root.join("contexts.json");
    private_json(
        &contexts,
        &json!({"contexts":[RequestContext{
        auth:enrollment.auth.clone(),native_session_id:native.clone(),workspace_key:workspace_key.clone()
    }],"batch_limit":64,"interval_seconds":3600}),
    );
    let passage_counter = root.join("passage-counts.json");
    let passage_probe = root.join("passage-worker.py");
    write_worker_probe(&passage_probe, &python, &passage_counter);
    let drain_socket = root.join("search-drain.sock");
    let drain_runtime = tagged_url(
        &runtime_url,
        &format!("dk3-search-drain-{}", Uuid::new_v4()),
    );
    let drain_started = Instant::now();
    let drain_daemon = EmbeddingDaemon::start(
        &drain_runtime,
        drain_socket.clone(),
        &passage_probe,
        &model,
        Some(&contexts),
    )
    .await;
    let passage_milestones = wait_for_passages(&passage_counter, 16).await;
    tokio::time::timeout(Duration::from_secs(45), async {
        loop {
            if counts(&pool, tenant, workspace).await
                == json!({"resources":16,"jobs":0,"vectors":16})
            {
                break;
            }
            tokio::time::sleep(Duration::from_millis(20)).await;
        }
    })
    .await
    .unwrap();
    let passage_drain_ms = drain_started.elapsed().as_millis();
    let passage_process = metrics::process_sample(&passage_counter);
    let mut second = Mcp::start(&drain_socket, &config, &native, &workspace_key).await;
    let mut rankings = Vec::new();
    let mut incremental = Vec::new();
    let mut warm_query_ms = None;
    for query in &fixed.queries {
        assert_eq!(
            query.mode,
            if query.id == "exact-title" {
                "lexical"
            } else {
                "super_wide"
            }
        );
        let (lexical_result, lexical_elapsed) = search(
            &mut second,
            corpus::search_params(query, "lexical", &binding_a),
        )
        .await;
        let (wide, wide_elapsed) = search(
            &mut second,
            corpus::search_params(query, "super_wide", &binding_a),
        )
        .await;
        assert_current_results(&wide);
        assert_eq!(wide["vector_status"], "ready");
        let lexical_units = result_units(&lexical_result)
            .into_iter()
            .collect::<BTreeSet<_>>();
        let wide_units = result_units(&wide).into_iter().collect::<BTreeSet<_>>();
        assert!(lexical_units.is_subset(&wide_units));
        for relevant in &query.relevant {
            assert!(
                wide_units.contains(units.get(relevant).unwrap()),
                "query {} missed {relevant}",
                query.id
            );
        }
        incremental.extend(corpus::incremental_top_five(
            query,
            &lexical_result,
            &wide,
            &unit_to_id,
        ));
        rankings.push(json!({"query_id":query.id,"relevant":query.relevant,
            "lexical":corpus::ranking(&lexical_result,&unit_to_id,&query.relevant),
            "super_wide":corpus::ranking(&wide,&unit_to_id,&query.relevant),
            "lexical_e2e_ms":lexical_elapsed,"super_wide_e2e_ms":wide_elapsed,
            "embedding":wide["metrics"]}));
        if query.id == "ru-recovery" {
            let count_before = fs::read(&passage_counter).unwrap();
            let (warm, elapsed) = search(
                &mut second,
                corpus::search_params(query, "super_wide", &binding_a),
            )
            .await;
            assert_eq!(warm["metrics"]["embedding_cache_hit"], true);
            assert_eq!(fs::read(&passage_counter).unwrap(), count_before);
            warm_query_ms = Some(elapsed);
        }
    }
    assert!(
        incremental
            .iter()
            .map(|value| value["query_id"].as_str().unwrap())
            .collect::<BTreeSet<_>>()
            .len()
            >= 2
    );
    let warm_query_ms = warm_query_ms.expect("immediate repeated query was measured");
    let worker_before_cache = fs::read(&passage_counter).unwrap();
    let (loaded_graph, _) = search(
        &mut second,
        json!({"mode":"graph_search",
        "seeds":[corpus::source_iri("resource-vector")],"relations":["derived_from"],
        "direction":"incoming","max_depth":1,"binding":binding_a,"limit":16,
        "corpus_limit":32,"purpose":"Prove loaded provider remains unused by graph search."}),
    )
    .await;
    assert!(result_units(&loaded_graph).contains(units.get("resource-vector").unwrap()));
    let (loaded_lexical, _) = search(
        &mut second,
        corpus::search_params(exact, "lexical", &binding_a),
    )
    .await;
    assert_eq!(
        result_units(&loaded_lexical)[0],
        *units.get("resource-vector").unwrap()
    );
    assert_eq!(fs::read(&passage_counter).unwrap(), worker_before_cache);

    let mut unrelated = fixed.records[0].clone();
    unrelated.id = "unrelated-apple-pie".into();
    unrelated.title = "Cinnamon apple pie recipe".into();
    unrelated.canonical_text = "Bake sliced apples with cinnamon in a pastry shell.".into();
    unrelated.source.path = "isolated-dk3-negative-fixture".into();
    unrelated.source.sha256 = "0".repeat(64);
    unrelated.source.revision = "fixture-v1".into();
    unrelated.source.line = 1;
    unrelated.source.text = unrelated.canonical_text.clone();
    let unrelated_commit = commit_create(&mut second, document(&unrelated, program_a)).await;
    let unrelated_unit = Uuid::parse_str(
        unrelated_commit.receipt["applied_operations"][0]["unit_id"]
            .as_str()
            .unwrap(),
    )
    .unwrap();
    assert_eq!(
        counts(&pool, tenant, workspace).await,
        json!({"resources":17,"jobs":1,"vectors":16})
    );
    let (missing_vector, _) = search(
        &mut second,
        json!({"mode":"super_wide",
        "query":unrelated.title,"binding":binding_a,"limit":17,"corpus_limit":32,
        "purpose":"Preserve exact lexical result when one title vector is pending."}),
    )
    .await;
    assert_eq!(missing_vector["vector_status"], "partial");
    assert!(result_units(&missing_vector).contains(&unrelated_unit));
    assert_eq!(passage_process["counts"]["passage"], 16);
    second.finish().await;
    drain_daemon.stop().await;

    let disabled_socket = root.join("search-disabled.sock");
    let disabled_runtime = tagged_url(
        &runtime_url,
        &format!("dk3-search-disabled-{}", Uuid::new_v4()),
    );
    let mut disabled_daemon = Daemon::start(&disabled_runtime, disabled_socket.clone()).await;
    let mut disabled = Mcp::start(&disabled_socket, &config, &native, &workspace_key).await;
    let (fallback_lexical, _) = search(
        &mut disabled,
        json!({"mode":"lexical",
        "query":unrelated.title,"binding":binding_a,"limit":17,"corpus_limit":32,
        "purpose":"Verify lexical operation without a configured embedding worker."}),
    )
    .await;
    assert!(result_units(&fallback_lexical).contains(&unrelated_unit));
    let (fallback_wide, _) = search(
        &mut disabled,
        json!({"mode":"super_wide",
        "query":unrelated.title,"binding":binding_a,"limit":17,"corpus_limit":32,
        "purpose":"Verify bounded lexical fallback without a configured embedding worker."}),
    )
    .await;
    assert_eq!(fallback_wide["vector_status"], "vector_unavailable");
    assert!(fallback_wide["metrics"].is_null());
    assert!(result_units(&fallback_wide).contains(&unrelated_unit));
    disabled.finish().await;
    disabled_daemon.crash().await;
    disabled_daemon.remove_owned_stale_socket();

    let proof = json!({"status":"pass","corpus_sha256":metrics::sha256(&corpus_path),
        "published_root_resources":16,"unrelated_resources":1,"program_a":program_a,
        "program_b":program_b,"tools":tool_names,"route":"knowledge.search",
        "graph_without_provider":true,"lexical_without_provider":true,
        "database_read_only_checkpoint":{"before":database_before,"after":database_after},
        "latency_ms":{"lexical_exact":lexical_ms,"graph":graph_ms,"cold_query_e2e":cold_query_ms,
            "warm_query_e2e":warm_query_ms,"passage_drain":passage_drain_ms,
            "passage_request_milestones":passage_milestones},
        "cold_query_process":cold_process,"passage_process":passage_process,
        "rankings":rankings,"incremental_vector_top_five":incremental,
        "loaded_provider_graph_and_lexical_calls":0,
        "relation_sizes":metrics::relation_sizes(&pool).await,
        "asset_bytes":{"model":metrics::tree_bytes(&model),"venv":metrics::tree_bytes(python.parent().unwrap().parent().unwrap())},
        "missing_vector_lexical_survives":true,"disabled_worker_lexical_fallback":true,
        "bounds":["isolated fixture","current canonical revisions","no application restore proof"]});
    private_json(&evidence, &proof);
}
