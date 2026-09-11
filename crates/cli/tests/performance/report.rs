use crate::TestResult;
use crate::fixture::{
    Cardinalities, SEEDED_PROGRAMS, SEEDED_SESSIONS, SEEDED_WORKSPACES, SEEDED_WORKTREES,
    SeedFixture,
};
use serde::Serialize;
use std::fs;
use std::io::Write;
use std::os::unix::fs::OpenOptionsExt;
use std::path::Path;
use std::process::Command;
use std::time::{SystemTime, UNIX_EPOCH};
use uuid::Uuid;

#[derive(Clone, Debug, Serialize)]
pub(crate) struct Failure {
    pub sample: usize,
    pub lane: usize,
    pub elapsed_ms: f64,
    pub error: String,
}

impl Failure {
    pub(crate) fn new(sample: usize, lane: usize, elapsed_ms: f64, error: String) -> Self {
        Self {
            sample,
            lane,
            elapsed_ms,
            error,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct Metric {
    pub concurrency: usize,
    pub attempted_samples: usize,
    pub latency_sample_count: usize,
    pub success_count: usize,
    pub failure_count: usize,
    pub p50_ms: Option<f64>,
    pub p95_ms: Option<f64>,
    pub p99_ms: Option<f64>,
    pub max_ms: Option<f64>,
    pub failures: Vec<Failure>,
}

impl Metric {
    pub(crate) fn new(
        concurrency: usize,
        attempted_samples: usize,
        durations_ms: &[f64],
        failures: Vec<Failure>,
    ) -> Self {
        let mut sorted = durations_ms.to_vec();
        sorted.sort_by(f64::total_cmp);
        let failure_count = failures.len();
        Self {
            concurrency,
            attempted_samples,
            latency_sample_count: sorted.len(),
            success_count: attempted_samples.saturating_sub(failure_count),
            failure_count,
            p50_ms: percentile(&sorted, 50),
            p95_ms: percentile(&sorted, 95),
            p99_ms: percentile(&sorted, 99),
            max_ms: sorted.last().copied(),
            failures,
        }
    }
}

fn percentile(sorted: &[f64], percentile: usize) -> Option<f64> {
    if sorted.is_empty() {
        return None;
    }
    let rank = (percentile * sorted.len()).div_ceil(100);
    sorted.get(rank.saturating_sub(1)).copied()
}

#[derive(Debug, Serialize)]
pub(crate) struct PerformanceReport {
    pub schema_version: &'static str,
    pub generated_unix_ms: u128,
    pub identity_note: &'static str,
    pub measurement_scope: &'static str,
    pub hardware: Hardware,
    pub versions: RuntimeVersions,
    pub fixture: FixtureReport,
    pub warm_get_state: Metric,
    pub open_workspace: Metric,
    pub budgets: Budgets,
    pub notes: Vec<&'static str>,
}

#[derive(Debug, Serialize)]
pub(crate) struct Hardware {
    pub os: String,
    pub architecture: String,
    pub cpu_model: String,
    pub logical_cpu_count: usize,
    pub memory_bytes: Option<u64>,
}

pub(crate) fn hardware() -> Hardware {
    Hardware {
        os: command_output("uname", &["-srv"]),
        architecture: std::env::consts::ARCH.to_owned(),
        cpu_model: cpu_model(),
        logical_cpu_count: std::thread::available_parallelism()
            .map(usize::from)
            .unwrap_or(0),
        memory_bytes: memory_bytes(),
    }
}

fn cpu_model() -> String {
    let mac = command_output("sysctl", &["-n", "machdep.cpu.brand_string"]);
    if mac != "unavailable" && !mac.is_empty() {
        return mac;
    }
    fs::read_to_string("/proc/cpuinfo")
        .ok()
        .and_then(|contents| {
            contents.lines().find_map(|line| {
                line.strip_prefix("model name\t:")
                    .map(|value| value.trim().to_owned())
            })
        })
        .unwrap_or_else(|| "unavailable".to_owned())
}

fn memory_bytes() -> Option<u64> {
    let mac = command_output("sysctl", &["-n", "hw.memsize"]);
    if let Ok(bytes) = mac.parse() {
        return Some(bytes);
    }
    fs::read_to_string("/proc/meminfo")
        .ok()?
        .lines()
        .find_map(|line| line.strip_prefix("MemTotal:"))?
        .split_whitespace()
        .next()?
        .parse::<u64>()
        .ok()
        .and_then(|kilobytes| kilobytes.checked_mul(1024))
}

fn command_output(program: &str, arguments: &[&str]) -> String {
    Command::new(program)
        .args(arguments)
        .output()
        .ok()
        .filter(|output| output.status.success())
        .map(|output| String::from_utf8_lossy(&output.stdout).trim().to_owned())
        .filter(|value| !value.is_empty())
        .unwrap_or_else(|| "unavailable".to_owned())
}

#[derive(Debug, Serialize)]
pub(crate) struct RuntimeVersions {
    pub tect: &'static str,
    pub rustc: String,
    pub sqlx: &'static str,
    pub postgres: String,
}

impl RuntimeVersions {
    pub(crate) fn capture(postgres: String) -> Self {
        Self {
            tect: env!("CARGO_PKG_VERSION"),
            rustc: command_output("rustc", &["--version"]),
            sqlx: "0.8.6",
            postgres,
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct FixtureReport {
    pub run_id: Uuid,
    pub declared_seed: DeclaredSeed,
    pub seeded_tenant: Cardinalities,
    pub tenant_after_selection: Cardinalities,
    pub final_tenant: Cardinalities,
    pub final_database_totals: Cardinalities,
    pub measured_session_count: usize,
    pub selected_worktrees_per_measured_session: usize,
    pub source_fixture: &'static str,
    pub program_profile: &'static str,
}

impl FixtureReport {
    pub(crate) fn new(
        run_id: Uuid,
        fixture: &SeedFixture,
        seeded_tenant: Cardinalities,
        tenant_after_selection: Cardinalities,
        final_tenant: Cardinalities,
        final_database_totals: Cardinalities,
    ) -> Self {
        Self {
            run_id,
            declared_seed: DeclaredSeed {
                workspaces: SEEDED_WORKSPACES,
                memberships: SEEDED_WORKSPACES,
                sessions: SEEDED_SESSIONS,
                source_repositories: 1,
                source_worktrees: SEEDED_WORKTREES,
                programs: SEEDED_PROGRAMS,
            },
            seeded_tenant,
            tenant_after_selection,
            final_tenant,
            final_database_totals,
            measured_session_count: fixture.measured_native_ids.len(),
            selected_worktrees_per_measured_session: fixture.worktree_ids.len(),
            source_fixture: "synthetic database rows; no filesystem worktree discovery",
            program_profile: "10 draft/compose Programs in the measured workspace",
        }
    }
}

#[derive(Debug, Serialize)]
pub(crate) struct DeclaredSeed {
    pub workspaces: i64,
    pub memberships: i64,
    pub sessions: i64,
    pub source_repositories: i64,
    pub source_worktrees: i64,
    pub programs: i64,
}

#[derive(Debug, Serialize)]
pub(crate) struct Budgets {
    pub warm_get_state_p95_limit_ms: f64,
    pub open_workspace_p95_limit_ms: f64,
    pub warm_get_state_pass: bool,
    pub open_workspace_pass: bool,
    pub no_measurement_failures: bool,
}

impl Budgets {
    pub(crate) fn new(warm: &Metric, bootstrap: &Metric) -> Self {
        let complete = |metric: &Metric| {
            metric.failure_count == 0
                && metric.latency_sample_count == metric.attempted_samples
                && metric.success_count == metric.attempted_samples
        };
        Self {
            warm_get_state_p95_limit_ms: 250.0,
            open_workspace_p95_limit_ms: 1_000.0,
            warm_get_state_pass: complete(warm)
                && warm.p95_ms.is_some_and(|latency| latency < 250.0),
            open_workspace_pass: complete(bootstrap)
                && bootstrap.p95_ms.is_some_and(|latency| latency < 1_000.0),
            no_measurement_failures: complete(warm) && complete(bootstrap),
        }
    }
}

pub(crate) fn unix_millis() -> u128 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default()
        .as_millis()
}

pub(crate) fn write_report(path: &Path, report: &PerformanceReport) -> TestResult<()> {
    if let Some(parent) = path.parent() {
        fs::create_dir_all(parent)?;
    }
    let mut file = fs::OpenOptions::new()
        .create(true)
        .truncate(true)
        .write(true)
        .mode(0o600)
        .open(path)?;
    serde_json::to_writer_pretty(&mut file, report)?;
    file.write_all(b"\n")?;
    file.sync_all()?;
    Ok(())
}
