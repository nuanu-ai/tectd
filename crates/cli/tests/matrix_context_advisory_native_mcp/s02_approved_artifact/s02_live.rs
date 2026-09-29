//! Isolated S02 one-shot. The options are agent-formulated under Owner delegation;
//! native Choice/Score thresholds remain provisional, not production approval.
use super::*;
use std::{
    fs,
    io::{BufRead, Write},
    os::unix::fs::OpenOptionsExt,
    path::{Path, PathBuf},
    sync::{
        Mutex,
        atomic::{AtomicBool, Ordering},
    },
    time::Duration,
};
use tect_application::{
    MatrixProviderObservation, MatrixProviderUsage, SignedMatrixBudgetPreflight,
    StoredMatrixDispatch,
};
use tect_host::jev_matrix_advice::native_provider::{
    JevNativeMatrixConfig, JevNativeMatrixProvider, MAX_NATIVE_MATRIX_RESPONSE_BYTES,
};
use tect_host::jev_matrix_advice::native_wire::NATIVE_MATRIX_WIRE_VERSION;

// -1 and -2 failed before dispatch; -3 and -4 each sent once but failed closed.
// -5 sent once and abstained under strict policy. All prior one-use markers stay spent.
const CALL_ID: &str = "tectd-jev-matrix-s02-mvp-2026-09-29-6";
const ENDPOINT: &str = "https://api.typesafe.ai/v1/systemone";
const PROFILE_ENV: &str = "JEV_MATRIX_PROFILE_ID";
const MODEL: &str = "jev-1.13.0";
const OWNER_SOURCE: &str = "owner-approval:active-jev-s02-operating-facts-2026-09-29";
const OBSERVED_AT: i64 = 1_790_682_517; // 2026-09-29 11:48:37 UTC; never refreshed by a run.
const EXPIRES_AT: i64 = OBSERVED_AT + 86_400;

#[path = "s02_live/continuation.rs"]
mod continuation;
#[path = "s02_live/pipeline.rs"]
mod pipeline;
#[path = "s02_live/source.rs"]
mod source;
pub(super) use continuation::selection_confirmation;

fn native_identity(profile: &str) -> MatrixProviderIdentity {
    let ranking_policy = match std::env::var("JEV_MATRIX_ONE_SHOT_RANKING_POLICY") {
        Ok(value) if value == "robust-trial-v1" => {
            tect_application::MatrixRankingPolicy::RobustTrialV1
        }
        Err(std::env::VarError::NotPresent) => tect_application::MatrixRankingPolicy::StrictV1,
        _ => panic!("only explicit robust-trial-v1 or absent strict policy is allowed"),
    };
    MatrixProviderIdentity {
        provider_profile_ref: AdvisoryProviderProfileRef { id: profile.into() },
        model_configuration: AdvisoryModelConfiguration {
            model: MODEL.into(),
        },
        destination: ENDPOINT.into(),
        wire_version: NATIVE_MATRIX_WIRE_VERSION.into(),
        ranking_policy,
    }
}

fn native_provider(profile: &str, credential: String) -> JevNativeMatrixProvider {
    JevNativeMatrixProvider::new(
        JevNativeMatrixConfig {
            provider_identity: native_identity(profile),
            endpoint: Url::parse(ENDPOINT).unwrap(),
            timeout: Duration::from_secs(15),
            maximum_request_bytes: tect_application::MAX_PREPARED_MATRIX_BODY_BYTES,
            maximum_response_bytes: MAX_NATIVE_MATRIX_RESPONSE_BYTES,
        },
        credential,
    )
    .unwrap()
}

/// Captures the exact application-minted context-v2 request before a deny-budget
/// no-call. Its transport is unreachable and never receives a dispatch permit.
struct CaptureProvider {
    inner: JevNativeMatrixProvider,
    body: Arc<Mutex<Option<Vec<u8>>>>,
    live: Arc<Mutex<Option<Arc<dyn MatrixAdviceProvider>>>>,
}
#[async_trait]
impl MatrixAdviceProvider for CaptureProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        self.inner.identity()
    }
    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        if let Some(live) = self.live.lock().unwrap().clone() {
            return live.prepare(request);
        }
        let prepared = self.inner.prepare(request)?;
        *self.body.lock().unwrap() = Some(prepared.body().to_vec());
        Ok(prepared)
    }
    fn parse_sealed_response(
        &self,
        request: &MatrixProviderRequest,
        saved: &StoredMatrixDispatch,
    ) -> Result<MatrixProviderResponse> {
        self.live
            .lock()
            .unwrap()
            .clone()
            .ok_or(tect_domain::Error::Forbidden)?
            .parse_sealed_response(request, saved)
    }
    fn sealed_response_usage(&self, saved: &StoredMatrixDispatch) -> MatrixProviderUsage {
        self.inner.sealed_response_usage(saved)
    }
    async fn observe_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderObservation> {
        let live = self
            .live
            .lock()
            .unwrap()
            .clone()
            .ok_or(tect_domain::Error::Forbidden)?;
        live.observe_prepared(prepared, permit).await
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        let live = self
            .live
            .lock()
            .unwrap()
            .clone()
            .ok_or(tect_domain::Error::Forbidden)?;
        live.attempt_prepared(prepared, permit).await
    }
}

struct SwitchedBudget(Arc<AtomicBool>);
#[async_trait]
impl MatrixBudgetPolicy for SwitchedBudget {
    async fn authorize(
        &self,
        request: &MatrixBudgetRequest,
        policy: &AdvisoryBudgetPolicy,
    ) -> Result<Option<MatrixBudgetAuthorization>> {
        if !self.0.load(Ordering::SeqCst) {
            return Ok(None);
        }
        SignedMatrixBudgetPreflight.authorize(request, policy).await
    }
}

/// Rechecks the frozen request at the last transport boundary after confirmation.
struct ReviewedProvider {
    inner: JevNativeMatrixProvider,
    reviewed: Arc<Vec<u8>>,
}
#[async_trait]
impl MatrixAdviceProvider for ReviewedProvider {
    fn identity(&self) -> Option<MatrixProviderIdentity> {
        self.inner.identity()
    }
    fn prepare(&self, request: &MatrixProviderRequest) -> Result<PreparedMatrixAdviceAttempt> {
        let prepared = self.inner.prepare(request)?;
        if prepared.body() != self.reviewed.as_slice() {
            return Err(tect_domain::Error::InputConflict);
        }
        Ok(prepared)
    }
    fn parse_sealed_response(
        &self,
        request: &MatrixProviderRequest,
        saved: &StoredMatrixDispatch,
    ) -> Result<MatrixProviderResponse> {
        self.inner.parse_sealed_response(request, saved)
    }
    fn sealed_response_usage(&self, saved: &StoredMatrixDispatch) -> MatrixProviderUsage {
        self.inner.sealed_response_usage(saved)
    }
    async fn observe_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderObservation> {
        if prepared.body() != self.reviewed.as_slice() {
            return Err(tect_domain::Error::InputConflict);
        }
        self.inner.observe_prepared(prepared, permit).await
    }
    async fn attempt_prepared(
        &self,
        prepared: PreparedMatrixAdviceAttempt,
        permit: MatrixStartedDispatchPermit,
    ) -> Result<MatrixProviderResponse> {
        if prepared.body() != self.reviewed.as_slice() {
            return Err(tect_domain::Error::InputConflict);
        }
        self.inner.attempt_prepared(prepared, permit).await
    }
}

#[test]
fn wrapper_preserves_native_sealed_usage_and_unknown_boundary() {
    use tect_application::MatrixProviderBinding;
    use tect_domain::{
        AdvisoryDispatch, AdvisoryDispatchOutcome, AdvisoryDispatchState, AdvisoryRetryBasis,
        AdvisorySendCertainty,
    };

    let profile = "synthetic-no-send";
    let saved = StoredMatrixDispatch {
        dispatch: AdvisoryDispatch {
            id: Uuid::new_v4(),
            opportunity_id: Uuid::new_v4(),
            predecessor_dispatch_id: None,
            attempt_number: 1,
            provider: profile.into(),
            model: MODEL.into(),
            configuration_digest: String::new(),
            material_digest: String::new(),
            payload_digest: String::new(),
            input_tokens: None,
            output_tokens: None,
            latency_ms: None,
            state: AdvisoryDispatchState::Sealed,
            send_certainty: AdvisorySendCertainty::Sent,
            outcome: Some(AdvisoryDispatchOutcome::ProviderResponse),
            retry_basis: AdvisoryRetryBasis::Initial,
            raw_response_ref: None,
        },
        binding: MatrixProviderBinding {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            input_digest: String::new(),
            choice_set_id: String::new(),
            choice_set_version: 1,
            choice_set_digest: String::new(),
            evaluation_digest: String::new(),
            verification: MatrixVerificationAuthority::Unverified,
        },
        provider_profile_ref: AdvisoryProviderProfileRef { id: profile.into() },
        model_configuration: AdvisoryModelConfiguration {
            model: MODEL.into(),
        },
        configuration_snapshot: json!({}),
        destination: ENDPOINT.into(),
        wire_version: NATIVE_MATRIX_WIRE_VERSION.into(),
        request_payload: Vec::new(),
        request_payload_sha256: String::new(),
        response_payload: Some(br#"{"usage":{"input_tokens":3656,"output_tokens":80}}"#.to_vec()),
        response_payload_sha256: None,
        response_http_status: Some(200),
        original_input_tokens: None,
        original_output_tokens: None,
        original_elapsed_ms: None,
        raw_observation_sealed: true,
        response_complete: true,
        original_transport_context: None,
    };
    let capture = CaptureProvider {
        inner: native_provider(profile, "synthetic-credential-never-sent".into()),
        body: Arc::new(Mutex::new(None)),
        live: Arc::new(Mutex::new(None)),
    };
    let reviewed = ReviewedProvider {
        inner: native_provider(profile, "synthetic-credential-never-sent".into()),
        reviewed: Arc::new(Vec::new()),
    };
    let known = MatrixProviderUsage {
        input_tokens: Some(3656),
        output_tokens: Some(80),
    };
    assert_eq!(capture.sealed_response_usage(&saved), known);
    assert_eq!(reviewed.sealed_response_usage(&saved), known);
    let mut unsealed = saved.clone();
    unsealed.raw_observation_sealed = false;
    assert_eq!(
        capture.sealed_response_usage(&unsealed),
        MatrixProviderUsage::default()
    );
    assert_eq!(
        reviewed.sealed_response_usage(&unsealed),
        MatrixProviderUsage::default()
    );
    let mut missing = saved;
    missing.response_payload = None;
    assert_eq!(
        capture.sealed_response_usage(&missing),
        MatrixProviderUsage::default()
    );
    assert_eq!(
        reviewed.sealed_response_usage(&missing),
        MatrixProviderUsage::default()
    );
}

fn owner_declarations() -> Vec<Value> {
    vec![
        declaration("mode", json!("mvp")),
        declaration(
            "intent",
            json!({"kind":"other","description":"Active JEV as optional TectD V2 advisor"}),
        ),
        declaration(
            "urgency",
            json!("Finish and verify this sprint; not an emergency production repair"),
        ),
        declaration(
            "promised_behavior",
            json!(
                "JEV is optional; skip/off prevents provider send; actual JEV calls are durably auditable; advice never auto-applies."
            ),
        ),
        declaration(
            "promised_proof",
            json!(
                "PG/CI tests plus real JEV outcomes; agent disposition; separate effect verification where selected"
            ),
        ),
        json!({"operation":"set","value":{"kind":"no_demand_commitment"}}),
        json!({"operation":"set","value":{"kind":"no_latency_commitment"}}),
    ]
}

fn owner_input() -> Value {
    let provenance = OWNER_SOURCE;
    let known = |value: Value| json!({"state":"known","value":value,"provenance":provenance});
    json!({
        "mode":{"state":"absent"},"intent":{"state":"absent"},"urgency":{"state":"absent"},
        "promised_behavior":{"state":"absent"},"promised_proof":{"state":"absent"},
        "demand_commitment":{"state":"absent"},"latency_commitment":{"state":"absent"},
        "envelope":{"scale":known(json!("One workspace/repo")),
            "operational_facts":{"state":"reported","entries":[
                {"name":"jev_optional","fact":known(json!("JEV optional"))},
                {"name":"calls_audited","fact":known(json!("calls audited"))},
                {"name":"testing_isolated","fact":known(json!("testing isolated"))}
            ]}},
        "criticality":known(json!("Development, not a production incident")),
        "affected_guarantees":known(json!(["data","secret"])),
        "actual_exposure":known(json!(false)),
        "urgent_repair":known(json!(false))
    })
}

fn owner_choices(task: Uuid) -> Value {
    json!({"schema":"tect.matrix-choice-set/1","choice_set_id":"active-jev-mvp-s02-sequence","version":1,
    "task_id":task.to_string(),"task_revision":"1",
    "decision_question":"Which bounded implementation sequence should deliver optional Matrix advice for the current Active JEV MVP without weakening mandatory EM02 obligations?",
    "candidates":[
        {"candidate_id":"matrix-local-evidence-first","title":"Prove isolated artifact-bound Matrix advice first",
         "approach":"Use public task/revision evidence-artifact issuance, independent verification, a signed one-call budget and native Choice/Score in disposable PostgreSQL; calibrate and separately decide production trust/promotion afterward.",
         "assumption_fact_ids":["actual_exposure","envelope.scale","envelope.operational_facts.jev_optional"]},
        {"candidate_id":"matrix-trust-first","title":"Build production trust resolver before Matrix advice trial",
         "approach":"Defer real Matrix advice until a production evidence resolver, owner key and positive signed policy are implemented and reviewed; continue deterministic EM02 composition and independent verification meanwhile.",
         "assumption_fact_ids":["affected_guarantees","actual_exposure","envelope.operational_facts.jev_optional"]}
    ]})
}

fn owner_artifact_body(workspace: Uuid, task: Uuid) -> String {
    let input = owner_input();
    let sourced = |fact: Value| json!({"fact":fact,"source_ref":OWNER_SOURCE,"observed_at":OBSERVED_AT,"expires_at":EXPIRES_AT});
    serde_json::to_string(&json!({
        "schema":"tect.matrix-operating-evidence/1","workspace_id":workspace,"task_id":task,"task_revision":1,
        "scale":sourced(input["envelope"]["scale"].clone()),
        "criticality":sourced(input["criticality"].clone()),
        "affected_guarantees":sourced(input["affected_guarantees"].clone()),
        "actual_exposure":sourced(input["actual_exposure"].clone()),
        "urgent_repair":sourced(input["urgent_repair"].clone()),
        "operational_facts":sourced(input["envelope"]["operational_facts"].clone())
    })).unwrap()
}

fn artifact_paths() -> (PathBuf, PathBuf) {
    let dir = PathBuf::from(
        std::env::var("JEV_MATRIX_ONE_SHOT_ARTIFACT_DIR")
            .expect("owner-only artifact dir required"),
    );
    assert!(dir.is_absolute() && dir.is_dir());
    let dir = dir.canonicalize().unwrap();
    let checkout = Path::new(env!("CARGO_MANIFEST_DIR"))
        .ancestors()
        .nth(2)
        .unwrap()
        .canonicalize()
        .unwrap();
    assert!(!dir.starts_with(checkout));
    assert_eq!(fs::metadata(&dir).unwrap().permissions().mode() & 0o077, 0);
    (
        dir.join(format!("{CALL_ID}.request.json")),
        dir.join(format!("{CALL_ID}.used")),
    )
}

fn exclusive_write(path: &Path, bytes: &[u8]) {
    let mut file = fs::OpenOptions::new()
        .write(true)
        .create_new(true)
        .mode(0o600)
        .open(path)
        .expect("one-use artifact already exists");
    file.write_all(bytes).unwrap();
    file.sync_all().unwrap();
    fs::File::open(path.parent().unwrap())
        .unwrap()
        .sync_all()
        .unwrap();
}

fn confirmation_matches(reader: &mut impl BufRead, digest: &str) -> bool {
    let mut answer = String::new();
    if reader.read_line(&mut answer).is_err() {
        return false;
    }
    answer.strip_suffix('\n').is_some_and(|line| {
        line.strip_suffix('\r').unwrap_or(line) == format!("SEND JEV MATRIX {digest}")
    })
}

fn now_seconds() -> i64 {
    i64::try_from(
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap()
            .as_secs(),
    )
    .unwrap()
}

async fn start_server(
    path: &Path,
    service: Arc<WorkspaceService>,
) -> tokio::task::JoinHandle<Result<()>> {
    let listener = UnixListener::bind(path).unwrap();
    fs::set_permissions(path, fs::Permissions::from_mode(0o600)).unwrap();
    tokio::spawn(tect_host::serve(listener, service))
}

async fn service(
    runtime_url: &str,
    keys: BudgetOwnerKeys,
    approval: ApprovedMatrixEvidenceArtifact,
    provider: Arc<dyn MatrixAdviceProvider>,
    budget: Arc<dyn MatrixBudgetPolicy>,
) -> Arc<WorkspaceService> {
    let pool = PgPool::connect_with(PgConnectOptions::from_str(runtime_url).unwrap())
        .await
        .unwrap();
    Arc::new(
        WorkspaceService::new(
            Arc::new(
                PgStore::connect(runtime_url, 4)
                    .await
                    .unwrap()
                    .with_budget_owner_keys(keys),
            ),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(pool, approval)))
        .with_matrix_advisory_adapters(provider, budget),
    )
}

#[path = "s02_live/run.rs"]
mod run;
