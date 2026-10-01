//! Additive S05 DEV harness. Real auth/PG/wire; business/Matrix scaffold is controlled.
//! No S02 technical approval. Never launches the candidate model.
use super::*;
use crate::model_route_live_tests::positive_input as input;
use crate::technical_decision_comparison_pg_tests::isolated_pg;
use crate::{ApprovedMatrixEvidenceArtifact, admin};
use ring::signature::{Ed25519KeyPair, KeyPair};
use std::os::unix::fs::MetadataExt;
const CASE_ID: &str = "jev-s05-ordinary-grouping-v1";
const ROUTE: &str = "dev-luna6";
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}
mod seed;
use seed::BaseFixture;
struct Catalogue;
impl ModelRouteCatalogueProvider for Catalogue {
    fn catalogue(&self) -> Result<Option<ModelRouteCatalogue>> {
        Ok(Some(ModelRouteCatalogue {
            schema: MODEL_ROUTE_CATALOGUE_SCHEMA.into(),
            version: 1,
            routes: vec![ModelRoute {
                id: "dev-luna6".into(),
                provider: "openai".into(),
                model: "gpt-6-luna".into(),
                effort: "xhigh".into(),
                enabled: true,
                allowed_matrix_choice_ids: vec!["reuse".into()],
                allowed_roles: vec!["implementation".into()],
                allowed_tools: vec!["code".into()],
                allowed_data_classes: vec!["internal".into()],
                required_host_capabilities: vec!["owned-stdio".into()],
                minimum_budget_units: 1,
                minimum_latency_ms: 1,
            }],
        }))
    }
}
struct Capabilities;
impl ModelRouteHostCapabilitiesProvider for Capabilities {
    fn host_capabilities(&self) -> Result<ModelRouteFact<Vec<String>>> {
        ModelRouteHostCapabilities {
            schema: MODEL_ROUTE_HOST_CAPABILITIES_SCHEMA.into(),
            version: 1,
            capabilities: vec!["owned-stdio".into()],
        }
        .fact()
    }
}
struct SyntheticRanker;

#[async_trait]
impl ModelRouteRankingProvider for SyntheticRanker {
    fn required_profile(&self) -> Option<&str> {
        Some("dev-s05-route")
    }
    fn prepare(
        &self,
        saved: &PreparedModelRouteRecommendation,
    ) -> Result<ModelRoutePreparedAttempt> {
        ModelRoutePreparedAttempt::new(ModelRouteRankingWireRequest::new(
            saved.workspace_id,
            &saved.request_key,
            &saved.work,
            saved.catalogue.as_ref().unwrap(),
            saved.eligible.as_ref().unwrap(),
            "SYNTHETIC CONTROL RANKER",
        )?)
    }
    async fn attempt_prepared(
        &self,
        attempted: ModelRoutePreparedAttempt,
        _: ModelRouteSendPermit,
    ) -> Result<Vec<u8>> {
        Ok(json!({"schema":MODEL_ROUTE_RANKING_WIRE_SCHEMA,"binding_digest":attempted.request.binding_digest,"adviser_model":"SYNTHETIC CONTROL RANKER","outcome":{"kind":"ranked","route_ids":["dev-luna6"]}}).to_string().into_bytes())
    }
    async fn attempt_prepared_observed(
        &self,
        attempted: ModelRoutePreparedAttempt,
        permit: ModelRouteSendPermit,
    ) -> Result<ModelRouteProviderObservation> {
        Ok(ModelRouteProviderObservation {
            raw: self.attempt_prepared(attempted, permit).await?,
            response_complete: None,
            original_transport_context: None,
            http_status: None,
            input_tokens: Some(4),
            output_tokens: Some(3),
            elapsed_monotonic_ms: Some(1),
        })
    }
}

fn ranking_provider(mode: &str) -> (Arc<dyn ModelRouteRankingProvider>, &'static str) {
    match mode {
        "synthetic" => (Arc::new(SyntheticRanker), "SYNTHETIC CONTROL RANKER"),
        "real" => {
            assert_eq!(
                std::env::var("TECT_TEST_S05_DEV_REAL_OPT_IN").as_deref(),
                Ok("one-authorized-jev-ranking"),
                "real mode requires separate explicit opt-in"
            );
            // Private child environment interface only. Never log/export the credential.
            let credential =
                std::env::var("TYPESAFE_API_KEY").expect("private child credential required");
            let provider = tect_host::JevModelRouteProvider::new(
                tect_host::JevModelRouteConfig {
                    profile: "dev-s05-route".into(),
                    endpoint: "https://api.typesafe.ai/v1/systemone".parse().unwrap(),
                    model: "jev-1.13.0".into(),
                    timeout: Duration::from_secs(10),
                    maximum_request_bytes: 45_000,
                    maximum_response_bytes: 64 * 1024,
                },
                credential,
            )
            .unwrap();
            (Arc::new(provider), "jev-1.13.0")
        }
        _ => panic!("explicit synthetic or real S05 DEV mode required; no fallback"),
    }
}
fn semantic_prompt(nonce: &str) -> String {
    format!(
        "Aggregate the provided records by group. Return ONLY one JSON object with exactly these keys: case_id, case_nonce, record_count, groups. Use the case_id and case_nonce below unchanged. record_count must be the total number of records. groups must be sorted by name ascending. Each group object must have exactly these keys: name, count, sum, min, max. count, sum, min, max and record_count must be JSON integers. Compute each group's count, sum, minimum and maximum from the records. Do not include commentary, code fences or extra fields.\ncase_id: {CASE_ID}\ncase_nonce: {nonce}\nrecords: [{{\"group\":\"jade\",\"value\":-8}},{{\"group\":\"amber\",\"value\":13}},{{\"group\":\"cobalt\",\"value\":0}},{{\"group\":\"amber\",\"value\":7}},{{\"group\":\"jade\",\"value\":-3}},{{\"group\":\"cobalt\",\"value\":11}},{{\"group\":\"amber\",\"value\":7}}]"
    )
}
fn read_prompt() -> (String, String) {
    let path = PathBuf::from(
        std::env::var("TECT_TEST_S05_DEV_PROMPT_FILE").expect("private prompt file required"),
    );
    assert!(path.is_absolute());
    assert_eq!(
        path.canonicalize().unwrap(),
        path,
        "symlink prompt prohibited"
    );
    let metadata = fs::symlink_metadata(&path).unwrap();
    assert!(metadata.is_file() && metadata.len() > 0 && metadata.len() <= 16_384);
    assert_eq!(metadata.permissions().mode() & 0o777, 0o600);
    let uid = std::process::Command::new("id").arg("-u").output().unwrap();
    assert!(uid.status.success());
    assert_eq!(
        metadata.uid(),
        std::str::from_utf8(&uid.stdout)
            .unwrap()
            .trim()
            .parse::<u32>()
            .unwrap()
    );
    let nonce = std::env::var("TECT_TEST_S05_DEV_NONCE").expect("fresh nonce required");
    assert!(
        nonce.len() == 32
            && nonce
                .bytes()
                .all(|b| b.is_ascii_digit() || (b'a'..=b'f').contains(&b))
    );
    let prompt = fs::read_to_string(&path).unwrap();
    assert_eq!(
        prompt,
        semantic_prompt(&nonce),
        "exact bounded semantic case prompt required (no trailing newline)"
    );
    (prompt, nonce)
}
async fn wire(socket: &Path, context: &RequestContext, tool: &str, args: Value) -> Value {
    tect_host::call_tool(socket, context, tool, args)
        .await
        .unwrap()
}
pub(super) async fn run() {
    let mode = std::env::var("TECT_TEST_S05_DEV_MODE").expect("explicit DEV ranking mode required");
    let hold: u64 = std::env::var("TECT_TEST_S05_DEV_HOLD_SECONDS")
        .unwrap_or_else(|_| "1".into())
        .parse()
        .unwrap();
    assert!((1..=600).contains(&hold));
    let (prompt_text, nonce) = read_prompt();
    let (provider, adviser) = ranking_provider(&mode);
    let control = seed::base_fixture().await;
    let c = &control;
    let tenant = c.owner.tenant_id;
    let workspace = c.workspace;
    let mut seed = [0u8; 32];
    getrandom::fill(&mut seed).unwrap();
    let key = Ed25519KeyPair::from_seed_unchecked(&seed).unwrap();
    seed.fill(0);
    let hex = |bytes: &[u8]| bytes.iter().map(|v| format!("{v:02x}")).collect::<String>();
    let keys=crate::BudgetOwnerKeys::from_json(&json!([{"workspace_id":workspace,"owner_id":c.owner.principal_id,"public_key_hex":hex(key.public_key().as_ref())}]).to_string()).unwrap();
    let store = PgStore::from_pool(c.runtime.clone()).with_budget_owner_keys(keys);
    let guards = Arc::new(NoExternal);
    let service = Arc::new(
        WorkspaceService::new(Arc::new(store.clone()), guards.clone(), guards)
            .with_matrix_evidence_validator(Arc::new(crate::PgMatrixEvidenceValidator::new(
                c.runtime.clone(),
                c.operating.clone(),
            )))
            .with_model_route_catalogue_provider(Arc::new(Catalogue))
            .with_model_route_host_capabilities_provider(Arc::new(Capabilities))
            .with_model_route_ranking_provider(provider),
    );
    service
        .configure_advisory(
            &c.owner_context,
            &ConfigureWorkspaceAdvisory {
                expected_revision: 0,
                mode: WorkspaceAdvisoryMode::Optional,
                provider_profile_ref: Some(AdvisoryProviderProfileRef {
                    id: "dev-s05-route".into(),
                }),
                model_configuration: Some(AdvisoryModelConfiguration {
                    model: adviser.into(),
                }),
            },
        )
        .await
        .unwrap();
    install_dev_policy(c, &store, &key).await;
    let opportunity = service
        .request_engineering_advisory(
            &c.owner_context,
            &RequestEngineeringAdvisory {
                task_id: c.task,
                expected_task_revision: 1,
                request_key: format!("control-matrix-{}", Uuid::new_v4()),
                session_preference: AdvisoryRequestPreference::UseWorkspace,
                request_preference: AdvisoryRequestPreference::Skip,
            },
        )
        .await
        .unwrap();
    assert_eq!(opportunity.state, AdvisoryOpportunityState::NoCall);
    let source = service
        .get_matrix_task_source(&c.owner_context, c.task)
        .await
        .unwrap();
    let disposition = service
        .record_matrix_disposition(
            &c.owner_context,
            &RecordMatrixDisposition {
                request_id: Uuid::new_v4(),
                task_id: c.task,
                expected_task_revision: 1,
                expected_input_digest: source.revision.input_digest.clone(),
                expected_choice_set_digest: source.revision.choice_set_digest.clone(),
                opportunity_id: opportunity.id,
                basis: MatrixDispositionBasis::NoCall,
                advice_id: None,
                advice_digest: None,
                decision: MatrixDispositionDecision::Selected {
                    selected_choice_id: "reuse".into(),
                },
            },
        )
        .await
        .unwrap();
    let (candidate_set, save, work_id, work_revision) =
        seed::save_work(c, &service, disposition.disposition_id).await;

    let directory = PathBuf::from("/private/tmp").join(format!("jev-s05-dev-{}", Uuid::new_v4()));
    fs::create_dir(&directory).unwrap();
    fs::set_permissions(&directory, fs::Permissions::from_mode(0o700)).unwrap();
    let socket = directory.join("host.sock");
    let listener = UnixListener::bind(&socket).unwrap();
    fs::set_permissions(&socket, fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service.clone()));
    let result = async {
        let request_key = format!("dev-s05-{}", Uuid::new_v4());
        let prepared_value = wire(&socket, &c.compare_context, "model_route_prepare", json!({
            "disposition_id":disposition.disposition_id,"expected_task_id":c.task,"expected_task_revision":1,
            "expected_candidate_set_id":candidate_set,"expected_caller_request_id":save.request_id,
            "expected_mapped_work_node_id":work_id,"expected_mapped_work_node_revision":work_revision,
            "request_key":request_key,"requested_route_id":ROUTE
        })).await;
        let prepared: PreparedModelRouteRecommendation = serde_json::from_value(prepared_value).unwrap();
        assert_eq!(prepared.preparation, ModelRoutePreparation::Prepared);
        let before = effects(&c.admin_pool, tenant).await;
        let run_value = wire(&socket, &c.compare_context, "model_route_run", json!({"preparation_request_key":request_key})).await;
        let replay = wire(&socket, &c.compare_context, "model_route_run", json!({"preparation_request_key":request_key})).await;
        assert_eq!(run_value, replay, "route ranking replay must reuse persisted result");
        let get = wire(&socket, &c.compare_context, "model_route_get", json!({"preparation_request_key":request_key})).await;
        assert_eq!(get, run_value);
        let decision: CapturedModelRouteDecision = serde_json::from_value(get["decision"].clone()).expect("ranked decision required; abstention denies dispatch");
        assert!(matches!(&decision.outcome, ModelRouteDecisionOutcome::Recommended { route_id } if route_id == ROUTE));
        assert_eq!(decision.routes.observed_actual, None);
        let disposition_id = Uuid::new_v4();
        let accepted = wire(&socket, &c.compare_context, "model_route_disposition", json!({
            "disposition_id":disposition_id,"decision_id":decision.id,"action":"accept",
            "rationale":"Explicit operator-issued DEV caller selection; no S02 or genuine Tony cryptographic approval"
        })).await;
        assert_eq!(accepted["id"], json!(disposition_id));
        let eligible = prepared.eligible.as_ref().unwrap();
        let selection_request = PrepareModelRouteHostSelection {
            preparation_request_key:request_key,decision_id:decision.id,disposition_id,
            expected_task_id:c.task,expected_task_revision:1,
            expected_work_context_digest:eligible.work_context_digest.clone(),
            expected_catalogue_digest:eligible.catalogue_digest.clone(),selected_route_id:ROUTE.into(),
            input_sha256:sha(prompt_text.as_bytes()),invocation_key:format!("dev-s05-invoke-{}", Uuid::new_v4()),
        };
        let current = wire(&socket, &c.compare_context, "prepare_model_route_host_selection", selection_params(&selection_request)).await;
        assert_eq!(current["material"]["selected_route"]["model"], "gpt-6-luna");
        assert_eq!(current["material"]["selected_route"]["effort"], "xhigh");
        assert_eq!(current["material_sha256"], sha(current["material_json"].as_str().unwrap().as_bytes()));
        let mut wrong = selection_request.clone(); wrong.input_sha256 = "z".repeat(64);
        assert_eq!(tect_host::call_tool(&socket,&c.compare_context,"prepare_model_route_host_selection",selection_params(&wrong)).await,Err(Error::InvalidArguments));
        let mut missing = selection_request.clone(); missing.disposition_id = Uuid::new_v4();
        assert_eq!(tect_host::call_tool(&socket,&c.compare_context,"prepare_model_route_host_selection",selection_params(&missing)).await,Err(Error::NotFound));
        let after = effects(&c.admin_pool, tenant).await;
        assert_eq!(after.3, before.3+1); assert_eq!(after.4, before.4+1);
        let counts:(i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM model_route_advisory_attempts WHERE tenant_id=$1),(SELECT count(*) FROM model_route_budget_reservations WHERE tenant_id=$1),(SELECT count(*) FROM model_route_budget_consumptions WHERE tenant_id=$1)").bind(tenant).fetch_one(&c.admin_pool).await.unwrap();
        assert_eq!(counts,(1,1,1));
        let auth=directory.join("host-auth.json"); private_file(&auth,serde_json::to_string(&c.owner.auth).unwrap().as_bytes());
        let prompt=directory.join("prompt.txt"); private_file(&prompt,prompt_text.as_bytes());
        let ledger=directory.join("ledger"); fs::create_dir(&ledger).unwrap(); fs::set_permissions(&ledger,fs::Permissions::from_mode(0o700)).unwrap();
        let mut pins=selection_params(&selection_request); pins.as_object_mut().unwrap().remove("input_sha256");
        pins["expected_workspace_id"]=json!(workspace); pins["expected_actor_id"]=json!(c.owner.principal_id); pins["expected_session_id"]=json!(c.sessions[1]);
        let pinpath=directory.join("pins.json"); private_file(&pinpath,serde_json::to_string(&pins).unwrap().as_bytes());
        private_file(&directory.join("source.json"),serde_json::to_string(&current).unwrap().as_bytes());
        private_file(&directory.join("route-readback.json"),serde_json::to_string(&wire(&socket,&c.compare_context,"model_route_get",json!({"preparation_request_key":selection_request.preparation_request_key})).await).unwrap().as_bytes());
        let control=directory.join("control.sock"); let stop=UnixListener::bind(&control).unwrap(); fs::set_permissions(&control,fs::Permissions::from_mode(0o600)).unwrap();
        let manifest=json!({"schema":"tect.s05-ordinary-dev-harness/1","ranking_mode":mode,
            "proof_boundary":"Real PG/auth/wire; controlled DEV Matrix/Scope/task/catalogue/economic facts; operator-issued ephemeral DEV budget signature, not genuine Tony crypto or S02 approval; ranking is SYNTHETIC only in synthetic mode",
            "case_id":CASE_ID,"case_nonce":nonce,"directory":directory,"socket":socket,"host_config":auth,
            "pins_path":pinpath,"prompt_path":prompt,"input_sha256":selection_request.input_sha256,
            "invocation_key":selection_request.invocation_key,"workspace_key":c.compare_context.workspace_key,
            "native_session_id":c.compare_context.native_session_id,"workspace_id":workspace,
            "actor_id":c.owner.principal_id,"session_id":c.sessions[1],"ledger_dir":ledger,"stop_socket":control,
            "hold_seconds":hold,"ranking_attempts":1,"real_jev_calls":if mode=="real"{1}else{0},
            "native_candidate_calls":0,"key_persistence":"Python launcher/observer durable ledger, not PostgreSQL",
            "selected":{"provider":"openai","model":"gpt-6-luna","effort":"xhigh"}});
        let ready=directory.join("ready.json"); private_file(&ready,serde_json::to_string_pretty(&manifest).unwrap().as_bytes());
        eprintln!("S05_ORDINARY_DEV_READY {}",ready.display());
        let stopped=tokio::time::timeout(Duration::from_secs(hold),stop.accept()).await;
        eprintln!("S05_ORDINARY_DEV_STOP {}",if stopped.is_ok(){"handshake"}else{"bounded_timeout"});
    }.await;
    server.abort();
    let _ = server.await;
    result
}
#[test]
fn semantic_case_prompt_has_exact_fresh_binding() {
    let prompt = semantic_prompt("0123456789abcdef0123456789abcdef");
    assert!(prompt.contains(
        "case_id: jev-s05-ordinary-grouping-v1\ncase_nonce: 0123456789abcdef0123456789abcdef\n"
    ));
    assert!(prompt.len() < 16_384 && !prompt.ends_with('\n'));
}
async fn install_dev_policy(c: &BaseFixture, store: &PgStore, key: &Ed25519KeyPair) {
    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_millis() as i64;
    let policy_id = Uuid::new_v4();
    let ceilings = AdvisoryBudgetCeilings {
        provider_calls: 1,
        input_tokens: 24_000,
        output_tokens: 2_000,
        request_utf8_bytes: 45_000,
        elapsed_monotonic_ms: 30_000,
        retry_dispatches: 1, // Policy schema requires positive; one-call ceiling and HTTP retry=never prohibit retries.
    };
    let digest =
        AdvisoryBudgetPolicy::digest_for(policy_id, 1, now - 60_000, now + 600_000, ceilings);
    let unsigned = AdvisoryBudgetPolicy::new(
        policy_id,
        1,
        digest.clone(),
        now - 60_000,
        now + 600_000,
        ceilings,
        c.owner.principal_id,
        "0".repeat(128),
    )
    .unwrap();
    let signature = key.sign(&unsigned.approval_signing_message(c.workspace).unwrap());
    let hex = signature
        .as_ref()
        .iter()
        .map(|v| format!("{v:02x}"))
        .collect::<String>();
    let policy = AdvisoryBudgetPolicy::new(
        policy_id,
        1,
        digest,
        now - 60_000,
        now + 600_000,
        ceilings,
        c.owner.principal_id,
        hex,
    )
    .unwrap();
    let mut tx = store.begin(TransactionMode::ReadWrite).await.unwrap();
    tx.authenticate(&c.owner.auth).await.unwrap();
    tx.set_tenant(c.owner.tenant_id).await.unwrap();
    tx.advisory_budget_policy_store()
        .unwrap()
        .install_budget_policy(c.workspace, &policy)
        .await
        .unwrap();
    tx.commit().await.unwrap();
}
