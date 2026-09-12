//! Explicit local performance acceptance. Fixture identities are synthetic UUIDs.

#[path = "performance/fixture.rs"]
mod fixture;
#[path = "performance/protocol.rs"]
mod protocol;
#[path = "performance/report.rs"]
mod report;

use fixture::{SeedFixture, cardinalities, seed_fixture};
use protocol::{Bridge, OwnedDaemon, write_private_config};
use report::{Failure, Metric, PerformanceReport, RuntimeVersions, hardware, write_report};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::Instant;
use tect_postgres::admin;
use tokio::sync::Barrier;
use uuid::Uuid;

pub(crate) type TestResult<T> = Result<T, Box<dyn std::error::Error + Send + Sync>>;

const WARM_CONCURRENCY: usize = 10;
const WARM_CALLS_PER_BRIDGE: usize = 100;
const OPEN_CONCURRENCY: usize = 10;
const OPEN_SAMPLES: usize = 100;

#[tokio::test(flavor = "multi_thread", worker_threads = 12)]
#[ignore = "explicit local MCP performance acceptance"]
async fn local_mcp_performance_acceptance() {
    let result = run_acceptance().await;
    if let Err(error) = result {
        panic!("performance acceptance setup failed: {error}");
    }
}

async fn run_acceptance() -> TestResult<()> {
    let environment = Environment::read()?;
    let admin_pool = PgPool::connect(&environment.admin_url).await?;
    admin::migrate(&admin_pool, &environment.runtime_role).await?;
    let enrollment = admin::enroll_host(&admin_pool, None, Vec::new()).await?;
    let run_id = Uuid::new_v4();
    let fixture = seed_fixture(&admin_pool, &enrollment, run_id, WARM_CONCURRENCY).await?;
    let seeded_tenant = cardinalities(&admin_pool, Some(enrollment.tenant_id)).await?;

    let temporary = tempfile::tempdir()?;
    let private_root = temporary.path().canonicalize()?;
    std::fs::set_permissions(
        &private_root,
        std::os::unix::fs::PermissionsExt::from_mode(0o700),
    )?;
    let socket = private_root.join("tectd.sock");
    let config = private_root.join("host.json");
    write_private_config(&config, &enrollment.auth)?;
    let daemon = OwnedDaemon::start(&socket, &environment.runtime_url).await?;

    let mut bridges = start_warm_bridges(&socket, &config, &fixture).await?;
    select_full_fixture(&mut bridges, &fixture.worktree_ids).await?;
    seed_program_population(&mut bridges).await?;
    admin::grant_setup_root(
        &admin_pool,
        enrollment.auth.host_id,
        private_root.to_string_lossy().into_owned(),
    )
    .await?;
    seed_setup_population(&mut bridges, &private_root).await?;
    let after_selection = cardinalities(&admin_pool, Some(enrollment.tenant_id)).await?;
    assert_eq!(after_selection.programs, fixture::SEEDED_PROGRAMS);
    assert_eq!(after_selection.program_inputs, fixture::SEEDED_PROGRAMS);
    assert_eq!(after_selection.workspace_setups, fixture::SEEDED_SETUPS);
    assert_eq!(
        after_selection.setup_session_directories,
        fixture::SEEDED_SETUPS
    );
    assert_eq!(
        after_selection.workspace_setup_inputs,
        fixture::SEEDED_SETUPS
    );
    let warm = measure_warm_reads(bridges).await;
    let bootstrap = measure_bootstraps(&socket, &config, run_id).await;
    let final_tenant = cardinalities(&admin_pool, Some(enrollment.tenant_id)).await?;
    let final_database = cardinalities(&admin_pool, None).await?;

    daemon.stop().await?;
    let postgres_version: String = sqlx::query_scalar("SELECT pg_catalog.version()")
        .fetch_one(&admin_pool)
        .await?;
    let warm_metric = warm.metric(WARM_CONCURRENCY, WARM_CONCURRENCY * WARM_CALLS_PER_BRIDGE);
    let bootstrap_metric = bootstrap.metric(OPEN_CONCURRENCY, OPEN_SAMPLES);
    let budgets = report::Budgets::new(&warm_metric, &bootstrap_metric);
    let report = PerformanceReport {
        schema_version: "tect.local-mcp-performance.v3",
        generated_unix_ms: report::unix_millis(),
        identity_note: "all IDs in this test are disposable synthetic UUID fixtures",
        measurement_scope: "local stdio MCP -> owned tectd Unix daemon -> PostgreSQL",
        hardware: hardware(),
        versions: RuntimeVersions::capture(postgres_version),
        fixture: report::FixtureReport::new(
            run_id,
            &fixture,
            seeded_tenant,
            after_selection,
            final_tenant,
            final_database,
        ),
        warm_get_state: warm_metric,
        open_workspace: bootstrap_metric,
        budgets,
        notes: vec![
            "fixture setup, Program/setup creation, worktree selection, and MCP initialization are excluded from measurements",
            "warm operation is exactly get_state; bootstrap operation is exactly open_workspace",
            "synthetic source paths are DB fixtures and are not filesystem Git worktrees",
            "each measured session has its own bound physical task directory and saved waiting_input setup; get_state does no file inspection",
        ],
    };
    write_report(&environment.report_path, &report)?;

    assert_eq!(report.warm_get_state.attempted_samples, 1_000);
    assert_eq!(report.warm_get_state.latency_sample_count, 1_000);
    assert_eq!(report.open_workspace.attempted_samples, 100);
    assert_eq!(report.open_workspace.latency_sample_count, 100);
    assert!(
        report.budgets.warm_get_state_pass,
        "warm get_state budget failed"
    );
    assert!(
        report.budgets.open_workspace_pass,
        "open_workspace budget failed"
    );
    assert!(
        report.budgets.no_measurement_failures,
        "one or more measured samples failed"
    );
    Ok(())
}

async fn start_warm_bridges(
    socket: &Path,
    config: &Path,
    fixture: &SeedFixture,
) -> TestResult<Vec<Bridge>> {
    let mut bridges = Vec::with_capacity(fixture.measured_native_ids.len());
    for native_id in &fixture.measured_native_ids {
        bridges.push(Bridge::start(socket, config, native_id, &fixture.first_workspace_key).await?);
    }
    Ok(bridges)
}

async fn select_full_fixture(bridges: &mut [Bridge], worktrees: &[Uuid]) -> TestResult<()> {
    for bridge in bridges {
        let response = bridge
            .tool_call("select_worktrees", json!({"worktree_ids": worktrees}))
            .await?;
        validate_selected(&response, worktrees.len()).map_err(std::io::Error::other)?;
    }
    Ok(())
}

async fn seed_program_population(bridges: &mut [Bridge]) -> TestResult<()> {
    for (index, bridge) in bridges.iter_mut().enumerate() {
        let response = bridge
            .tool_call(
                "begin_program",
                json!({
                    "request_id": Uuid::new_v4(),
                    "input": format!("Measured nonempty Program fixture {index}")
                }),
            )
            .await?;
        let payload = decode_payload(&response).map_err(std::io::Error::other)?;
        if payload["program"]["status"] != "draft"
            || payload["program"]["current_step"] != "compose"
        {
            return Err(std::io::Error::other("program_fixture_not_draft_compose").into());
        }
    }
    Ok(())
}

async fn seed_setup_population(bridges: &mut [Bridge], root: &Path) -> TestResult<()> {
    for (index, bridge) in bridges.iter_mut().enumerate() {
        let path = root.join(format!("task-{index}"));
        std::fs::create_dir(&path)?;
        let discovery = bridge
            .tool_call("inspect_setup", json!({"task_directory":path}))
            .await?;
        let discovery = decode_ready(&discovery).map_err(std::io::Error::other)?;
        assert_eq!(discovery["file"]["status"], "missing");
        let created = bridge.tool_call("begin_setup", json!({"request_id":Uuid::new_v4(),
            "input":format!("We are measured workspace team {index}; preserve our working instructions.")})).await?;
        let created = decode_payload(&created).map_err(std::io::Error::other)?;
        let saved = bridge.tool_call("save_setup", json!({"setup_id":created["setup"]["id"],
            "revision":1,"input_cursor":1,"ready":false,
            "content":format!("# Measured team {index}\nPreserve explicit user instructions.\n"),
            "working_notes":"The narrative is incorporated. One actual choice remains in this synthetic fixture.",
            "pending_question":"Which team owns release acceptance?"})).await?;
        let saved = decode_payload(&saved).map_err(std::io::Error::other)?;
        assert_eq!(saved["setup"]["current_step"], "waiting_input");
    }
    Ok(())
}

async fn measure_warm_reads(bridges: Vec<Bridge>) -> Observations {
    let mut tasks = Vec::with_capacity(bridges.len());
    for (lane, mut bridge) in bridges.into_iter().enumerate() {
        tasks.push(tokio::spawn(async move {
            let mut observations = Observations::default();
            for sample in 0..WARM_CALLS_PER_BRIDGE {
                let started = Instant::now();
                let result = bridge.tool_call("get_state", json!({})).await;
                let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
                observations.durations_ms.push(elapsed);
                let validation = result
                    .map_err(|error| error.to_string())
                    .and_then(|value| validate_warm_profile(&value));
                if let Err(error) = validation {
                    observations.failures.push(Failure::new(
                        lane * WARM_CALLS_PER_BRIDGE + sample,
                        lane,
                        elapsed,
                        error,
                    ));
                }
            }
            bridge.stop().await;
            observations
        }));
    }
    let mut combined = Observations::default();
    for task in tasks {
        match task.await {
            Ok(observations) => combined.extend(observations),
            Err(error) => combined.failures.push(Failure::new(
                combined.durations_ms.len(),
                usize::MAX,
                0.0,
                format!("warm_task_join:{error}"),
            )),
        }
    }
    combined
}

async fn measure_bootstraps(socket: &Path, config: &Path, run_id: Uuid) -> Observations {
    let mut combined = Observations::default();
    for wave in 0..(OPEN_SAMPLES / OPEN_CONCURRENCY) {
        let barrier = Arc::new(Barrier::new(OPEN_CONCURRENCY));
        let mut tasks = Vec::with_capacity(OPEN_CONCURRENCY);
        for lane in 0..OPEN_CONCURRENCY {
            let index = wave * OPEN_CONCURRENCY + lane;
            let (socket, config, barrier) = (socket.to_owned(), config.to_owned(), barrier.clone());
            tasks.push(tokio::spawn(async move {
                let native_id = Uuid::new_v4().to_string();
                let workspace_key = format!("perf-open-{}-{index}", run_id.simple());
                let bridge = Bridge::start(&socket, &config, &native_id, &workspace_key).await;
                barrier.wait().await;
                let mut observation = Observations::default();
                let started = Instant::now();
                match bridge {
                    Ok(mut bridge) => {
                        let result = bridge.tool_call("open_workspace", json!({})).await;
                        let elapsed = started.elapsed().as_secs_f64() * 1_000.0;
                        observation.durations_ms.push(elapsed);
                        if let Err(error) = result
                            .map_err(|error| error.to_string())
                            .and_then(|value| validate_ready(&value))
                        {
                            observation
                                .failures
                                .push(Failure::new(index, lane, elapsed, error));
                        }
                        bridge.stop().await;
                    }
                    Err(error) => observation.failures.push(Failure::new(
                        index,
                        lane,
                        0.0,
                        format!("bridge_initialize:{error}"),
                    )),
                }
                observation
            }));
        }
        for task in tasks {
            match task.await {
                Ok(observation) => combined.extend(observation),
                Err(error) => combined.failures.push(Failure::new(
                    combined.durations_ms.len(),
                    usize::MAX,
                    0.0,
                    format!("bootstrap_task_join:{error}"),
                )),
            }
        }
    }
    combined
}

fn validate_selected(response: &Value, expected: usize) -> Result<(), String> {
    let payload = decode_ready(response)?;
    let selected = payload["selected_worktrees"]
        .as_array()
        .ok_or_else(|| "missing_selected_worktrees".to_owned())?;
    if selected.len() != expected {
        return Err(format!("selected_count:{}", selected.len()));
    }
    Ok(())
}

fn validate_warm_profile(response: &Value) -> Result<(), String> {
    validate_selected(response, 100)?;
    let payload = decode_ready(response)?;
    if payload["programs"].as_array().map(Vec::len) != Some(10)
        || payload["setup_context"]["setup"]["current_step"] != "waiting_input"
        || payload["file"]["observed_now"] != false
        || payload["actions"][0]["tool"] != "query"
        || payload["actions"][0]["arguments"]["route"] != "setup.get"
        || payload["actions"][0]["arguments"]["params"]["after_input"] != 0
    {
        return Err("measured_program_setup_profile_missing".into());
    }
    Ok(())
}

fn validate_ready(response: &Value) -> Result<(), String> {
    decode_ready(response).map(|_| ())
}

fn decode_ready(response: &Value) -> Result<Value, String> {
    if response.get("error").is_some() {
        return Err("json_rpc_error".to_owned());
    }
    let payload = decode_payload(response)?;
    if response["result"]["isError"] == true {
        return Err(format!(
            "tool_error:{}",
            payload["error"]["code"].as_str().unwrap_or("unknown")
        ));
    }
    if payload["status"] != "ready" {
        return Err("state_not_ready".to_owned());
    }
    Ok(payload)
}

fn decode_payload(response: &Value) -> Result<Value, String> {
    let result = response
        .get("result")
        .and_then(Value::as_object)
        .ok_or_else(|| "missing_tool_result".to_owned())?;
    if result.contains_key("structuredContent") {
        return Err("unexpected_structured_content".to_owned());
    }
    let content = result
        .get("content")
        .and_then(Value::as_array)
        .ok_or_else(|| "missing_tool_content".to_owned())?;
    if content.len() != 2 || content.iter().any(|item| item["type"] != "text") {
        return Err("invalid_tool_content_shape".to_owned());
    }
    let intro = content[0]["text"]
        .as_str()
        .ok_or_else(|| "missing_tool_intro".to_owned())?;
    if intro.is_empty() || intro.len() > 2_000 {
        return Err("invalid_tool_intro".to_owned());
    }
    serde_json::from_str(
        content[1]["text"]
            .as_str()
            .ok_or_else(|| "missing_json_payload".to_owned())?,
    )
    .map_err(|error| format!("invalid_json_payload:{error}"))
}

#[derive(Default)]
pub(crate) struct Observations {
    durations_ms: Vec<f64>,
    failures: Vec<Failure>,
}

impl Observations {
    fn extend(&mut self, other: Self) {
        self.durations_ms.extend(other.durations_ms);
        self.failures.extend(other.failures);
    }

    fn metric(&self, concurrency: usize, attempted: usize) -> Metric {
        Metric::new(
            concurrency,
            attempted,
            &self.durations_ms,
            self.failures.clone(),
        )
    }
}

struct Environment {
    admin_url: String,
    runtime_url: String,
    runtime_role: String,
    report_path: PathBuf,
}

impl Environment {
    fn read() -> TestResult<Self> {
        let report_path = PathBuf::from(required_env("TECT_PERFORMANCE_REPORT")?);
        if !report_path.is_absolute() {
            return Err("TECT_PERFORMANCE_REPORT must be absolute".into());
        }
        Ok(Self {
            admin_url: required_env("TECT_TEST_ADMIN_URL")?,
            runtime_url: required_env("TECT_TEST_RUNTIME_URL")?,
            runtime_role: required_env("TECT_TEST_RUNTIME_ROLE")?,
            report_path,
        })
    }
}

fn required_env(name: &str) -> TestResult<String> {
    match std::env::var(name) {
        Ok(value) if !value.is_empty() => Ok(value),
        _ => Err(format!("{name} is required").into()),
    }
}
