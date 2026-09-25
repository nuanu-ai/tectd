#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use async_trait::async_trait;
use recovery_support::{Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::os::unix::fs::PermissionsExt;
use std::sync::{
    Arc,
    atomic::{AtomicUsize, Ordering},
};
use support::{id, repository, route, route_error};
use tect_application::{
    AntiBloatRankingProvider, AntiBloatSendPermit, Sha256ScopeDigest, WorkspaceService,
};
use tect_application::{
    AuthoredScopeAlternative, AuthoredScopeSet, GuardedScopeAdviceRecord, ScopeAuthorityObserver,
};
use tect_application::{
    ScopeAuthoredManifestRequest, ScopeAuthorityOutcome, ScopeAuthorityRequest,
    ScopeManifestRecord, ScopeManifestSupplier, Store, TransactionMode,
};
use tect_domain::{
    ConfidenceBasisPoints, NormalizedScopeAdviceAnswer, NormalizedScopeAdviceAnswers,
    ScopeAdviceChoice, ScopeAdviceScoreBand, ScopeDecompositionKind, guard_scope_advice,
};
use tect_postgres::{PgScopeAuthoredManifestSupplier, PgScopeAuthorityObserver, PgStore, admin};
use tokio::net::UnixListener;
use uuid::Uuid;

struct CommittedFakeProvider {
    pool: PgPool,
    calls: Arc<AtomicUsize>,
}

#[async_trait]
impl AntiBloatRankingProvider for CommittedFakeProvider {
    async fn rank(
        &self,
        permit: &AntiBloatSendPermit,
    ) -> tect_domain::Result<tect_application::AntiBloatProviderObservation> {
        let observed: (String, Vec<u8>, String, bool) = sqlx::query_as(
            "SELECT state,request_bytes,request_sha256,raw_response IS NOT NULL \
             FROM scope_anti_bloat_reviews WHERE review_id=$1",
        )
        .bind(permit.review_id)
        .fetch_one(&self.pool)
        .await
        .expect("independent connection observes committed send fence");
        assert_eq!(observed.0, "sending");
        assert_eq!(observed.1, permit.request.bytes);
        assert_eq!(observed.2, permit.request.sha256);
        assert!(!observed.3);
        assert_eq!(format!("{:x}", Sha256::digest(&observed.1)), observed.2);
        self.calls.fetch_add(1, Ordering::SeqCst);
        let request: Value = serde_json::from_slice(&permit.request.bytes).unwrap();
        Ok(tect_application::AntiBloatProviderObservation {
            raw: serde_json::to_vec(&request["eligible_ids"]).unwrap(),
            input_tokens: Some(1),
            output_tokens: Some(1),
            elapsed_monotonic_ms: Some(1),
        })
    }
}

fn authored_draft(source_ref: &Value) -> Value {
    json!({
        "boundary":"ongoing",
        "goals":[{"identity":{"local":"goal"},"text":"Explain the preview deviation",
            "source_ref_id":source_ref,"resolution":{"kind":"candidate","reference":{"local":"scope"}}}],
        "evidence":[],
        "candidates":[{"identity":{"local":"scope"},"title":"Selected preview diagnosis",
            "outcome":"The cause is demonstrated","trigger":"Preview differs",
            "delivered_behavior":"Cause and bounded correction path are available",
            "proof":"Direct evidence is retained","includes":["diagnosis"],
            "excludes":["deployment"],"dependencies":[],
            "coverage_goals":[{"local":"goal"}],"evidence":[]}],
        "blockers":[],"protected_changes":[]
    })
}

#[tokio::test(flavor = "multi_thread", worker_threads = 8)]
#[ignore = "requires disposable PostgreSQL 18 and TECT_TEST_*; run with `cargo test -p tect-cli --test scope_advisory_selected_mcp -- --ignored --nocapture`"]
async fn public_selected_advisory_save_is_durable_and_session_bound() {
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let pool = PgPool::connect(&admin_url).await.unwrap();
    let (database, database_oid, system_id, migration_count, last_migration):
        (String, i64, String, i64, i64) = sqlx::query_as(
        "SELECT current_database(),d.oid::bigint,(SELECT system_identifier::text FROM pg_control_system()), \
         (SELECT count(*) FROM _sqlx_migrations),(SELECT max(version) FROM _sqlx_migrations) \
         FROM pg_database d WHERE datname=current_database()"
    ).fetch_one(&pool).await.unwrap();
    assert_eq!(
        (database.as_str(), database_oid, system_id.as_str()),
        ("tect_test", 16385, "7689349823162929726")
    );
    assert_eq!((migration_count, last_migration), (81, 81));
    let runtime_pool = PgPool::connect(&runtime_url).await.unwrap();
    let runtime_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime_pool)
        .await
        .unwrap();
    assert_eq!(runtime_user, role);
    let version: String = sqlx::query_scalar("SHOW server_version_num")
        .fetch_one(&pool)
        .await
        .unwrap();
    assert_eq!(version.parse::<i32>().unwrap() / 10_000, 18);

    let temp = private_temp();
    let root = temp.path().canonicalize().unwrap();
    let repo = root.join("source");
    repository(&repo);
    let socket = root.join("selected-advice.sock");
    let runtime = tagged_url(&runtime_url, &format!("tect-selected-{}", Uuid::new_v4()));
    let store = PgStore::connect(&runtime, 4).await.unwrap();
    let calls = Arc::new(AtomicUsize::new(0));
    let service = Arc::new(
        WorkspaceService::new(
            Arc::new(store.clone()),
            Arc::new(tect_host::GitSourceInspector),
            Arc::new(tect_host::LocalSetupFiles),
        )
        .with_anti_bloat_provider(Arc::new(CommittedFakeProvider {
            pool: pool.clone(),
            calls: calls.clone(),
        })),
    );
    let listener = UnixListener::bind(&socket).unwrap();
    std::fs::set_permissions(&socket, std::fs::Permissions::from_mode(0o600)).unwrap();
    let server = tokio::spawn(tect_host::serve(listener, service));
    let enrollment = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let config_path = root.join("host.json");
    host_file(&config_path, &enrollment.auth);
    let key = format!("selected-advice-{}", Uuid::new_v4());
    let native_a = Uuid::new_v4().to_string();
    let native_b = Uuid::new_v4().to_string();
    let mut author = Mcp::start(&socket, &config_path, &native_a, &key).await;
    let mut reader = Mcp::start(&socket, &config_path, &native_b, &key).await;
    let opened = route(&mut author, "command", "workspace.open", json!({})).await;
    route(&mut reader, "command", "workspace.open", json!({})).await;
    let workspace = id(&opened["workspace"]["id"]);
    route(
        &mut author,
        "command",
        "workspace.advisory.configure",
        json!({
            "expected_revision":0,"mode":"optional",
            "provider_profile_ref":{"id":"fixture-only"},
            "model_configuration":{"model":"fixture-only"}
        }),
    )
    .await;
    let registered = route(
        &mut author,
        "command",
        "source.register",
        json!({"path":repo}),
    )
    .await;
    route(
        &mut author,
        "command",
        "session.select_worktrees",
        json!({
            "worktree_ids":[registered["id"]]
        }),
    )
    .await;
    route(
        &mut reader,
        "command",
        "session.select_worktrees",
        json!({
            "worktree_ids":[registered["id"]]
        }),
    )
    .await;
    let begun_program = route(
        &mut author,
        "command",
        "program.begin",
        json!({
            "request_id":Uuid::new_v4(),"input":"Diagnose the incorrect preview."
        }),
    )
    .await;
    let program = route(&mut author, "command", "program.save", json!({
        "program_id":begun_program["program"]["id"],"revision":1,"input_cursor":1,
        "name":"Preview diagnosis","intent":"Correct preview behavior",
        "basis":"Preview differs from saved settings","boundaries":"Diagnosis and bounded correction",
        "constraints":"No deployment","success":"Cause and correction are verified","complete":true
    })).await;
    let begun = route(
        &mut author,
        "command",
        "scope.candidates.begin",
        json!({
            "request_id":Uuid::new_v4(),"program_id":program["program"]["id"],
            "program_revision":program["program"]["revision"],"boundary":"ongoing",
            "input":"Select the source-authored preview diagnosis."
        }),
    )
    .await;
    let context = &begun["context"];
    let candidate_set = id(&context["candidate_set"]["id"]);
    let mut refs: Vec<Uuid> = context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .map(|item| id(&item["id"]))
        .collect();
    refs.sort();
    assert!(!refs.is_empty());
    let planning_ref = context["snapshot"]["source_refs"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["kind"] == "planning_input")
        .unwrap();
    let mut draft = authored_draft(&planning_ref["id"]);
    let mut initial_draft = draft.clone();
    initial_draft["candidates"][0]["title"] = json!("Initial preview diagnosis");
    let manifest_guard = &context["planning_knowledge"]["manifest"];
    let initial = route(
        &mut author,
        "command",
        "scope.candidates.save",
        json!({
            "kind":"draft","candidate_set_id":candidate_set,"revision":1,
            "snapshot_id":context["snapshot"]["id"],"input_cursor":1,
            "request_id":Uuid::new_v4(),"draft":initial_draft,
            "consumed_knowledge":{"manifest_id":manifest_guard["id"],
                "digest":manifest_guard["digest"],
                "workspace_generation":manifest_guard["workspace_generation"]}
        }),
    )
    .await;
    let initial_context = &initial["context"];
    let reviewed = route(&mut author, "command", "scope.candidates.save", json!({
        "kind":"review","candidate_set_id":candidate_set,
        "revision":initial_context["candidate_set"]["revision"],
        "snapshot_id":initial_context["snapshot"]["id"],
        "input_cursor":initial_context["candidate_set"]["input_cursor"],
        "request_id":Uuid::new_v4(),
        "review":{"verdict":"ready","summary":"The initial diagnosis is bounded and traceable.",
            "findings":[],"candidate_decisions":[{"candidate_id":initial["draft"]["candidates"][0]["id"],
                "decision":"accept","rationale":"Bounded diagnosis"}]}
    })).await;
    assert_eq!(reviewed["context"]["candidate_set"]["status"], "ready");
    let context = &reviewed["context"];
    let goal = &initial["draft"]["goals"][0];
    let prior_candidate = &initial["draft"]["candidates"][0];
    draft["goals"][0]["identity"] = json!({"id":goal["id"],"revision":goal["revision"]});
    draft["goals"][0]["resolution"]["reference"] = json!({"id":prior_candidate["id"]});
    draft["candidates"][0]["identity"] =
        json!({"id":prior_candidate["id"],"revision":prior_candidate["revision"]});
    draft["candidates"][0]["coverage_goals"] = json!([{"id":goal["id"]}]);
    draft["candidates"][0]["change_rationale"] = json!("Use the selected source-authored title");
    draft["candidates"].as_array_mut().unwrap().push(json!({
        "identity":{"local":"exploratory"},
        "grounding":{"kind":"exploratory_unrequested","provenance":"source_authored_v2"},
        "title":"Unrequested exploratory dashboard","outcome":"Optional dashboard",
        "trigger":"Exploration","delivered_behavior":"Show a dashboard",
        "proof":"Optional visual check","coverage_goals":[]
    }));
    let revision = context["candidate_set"]["revision"].as_i64().unwrap();
    let snapshot = id(&context["snapshot"]["id"]);
    let input_cursor = context["candidate_set"]["input_cursor"].as_i64().unwrap();
    let authored = AuthoredScopeSet {
        expected_candidate_set_revision: revision,
        baseline_key: "baseline".into(),
        alternatives: vec![AuthoredScopeAlternative {
            key: "baseline".into(),
            kind: ScopeDecompositionKind::Cohesive,
            draft: serde_json::from_value(draft.clone()).unwrap(),
            covered_source_ref_ids: refs.clone(),
        }],
    };
    let (session, actor): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id) WHERE s.tenant_id=$1 AND s.native_session_id=$2"
    ).bind(enrollment.tenant_id).bind(&native_a).fetch_one(&pool).await.unwrap();
    let authority = Arc::new(PgScopeAuthorityObserver::new(
        store.clone(),
        Arc::new(tect_host::StaticCandidateGuidance),
    ));
    let observed = authority
        .observe(&ScopeAuthorityRequest {
            tenant_id: enrollment.tenant_id,
            workspace_id: workspace,
            actor_id: actor,
            session_id: session,
            candidate_set_id: candidate_set,
        })
        .await
        .unwrap();
    let ScopeAuthorityOutcome::Authorized(observed) = observed else {
        panic!("source must be current")
    };
    let supplier = PgScopeAuthoredManifestSupplier::new(store.clone(), authority);
    let manifest = supplier
        .supply_authored(&ScopeAuthoredManifestRequest {
            tenant_id: enrollment.tenant_id,
            observation: *observed,
            authored_scope_set: authored,
        })
        .await
        .unwrap();
    let opportunity = Uuid::new_v4();
    let dispatch = Uuid::new_v4();
    let digest = manifest.whole_set_digest.clone();
    sqlx::query("INSERT INTO advisory_opportunity(id,tenant_id,workspace_id,work_item_kind,work_item_id,source_revision,session_id,authorized_actor_id,capability,decision_point,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES($1,$2,$3,'scope_candidate_set',$4,$5,$6,$7,'scope_decomposition','scope.decomposition.before_selection',1,'use_workspace','use_workspace','fixture',$8,$9,'prepared','dispatch_authorized')")
        .bind(opportunity).bind(enrollment.tenant_id).bind(workspace).bind(candidate_set)
        .bind(revision.to_string()).bind(session).bind(actor).bind(opportunity.to_string()).bind(&digest)
        .execute(&pool).await.unwrap();
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(&enrollment.auth).await.unwrap();
    unit.set_tenant(enrollment.tenant_id).await.unwrap();
    unit.prepare_authored_scope_advisory_manifest(
        workspace,
        &ScopeManifestRecord {
            opportunity_id: opportunity,
            candidate_set_id: candidate_set,
            config_revision: 1,
            opportunity_material_digest: digest.clone(),
            manifest: manifest.clone(),
        },
        &"a".repeat(64),
    )
    .await
    .unwrap();
    unit.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch(id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES($1,$2,$3,$4,1,'fixture','fixture','{}',$5,$5,$5,'fixture','fixture','sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(dispatch).bind(enrollment.tenant_id).bind(workspace).bind(opportunity).bind(&digest)
        .execute(&pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity).execute(&pool).await.unwrap();
    let request =
        tect_domain::ScopeAdviceRequest::from_manifest(&Sha256ScopeDigest, &manifest).unwrap();
    let advice = guard_scope_advice(
        &Sha256ScopeDigest,
        opportunity,
        &manifest,
        &request,
        &NormalizedScopeAdviceAnswers {
            answers: vec![NormalizedScopeAdviceAnswer {
                alternative_id: manifest.baseline_id.clone(),
                choice: ScopeAdviceChoice::Preferred,
                score: ScopeAdviceScoreBand::StrongFit,
                choice_confidence: ConfidenceBasisPoints(9000),
                score_confidence: ConfidenceBasisPoints(8000),
            }],
        },
    )
    .unwrap();
    let mut unit = store.begin(TransactionMode::ReadWrite).await.unwrap();
    unit.authenticate(&enrollment.auth).await.unwrap();
    unit.set_tenant(enrollment.tenant_id).await.unwrap();
    unit.persist_guarded_scope_advice(
        workspace,
        &GuardedScopeAdviceRecord {
            opportunity_id: opportunity,
            candidate_set_id: candidate_set,
            dispatch_id: dispatch,
            dispatch_material_digest: digest,
            config_revision: 1,
            advice: advice.clone(),
        },
    )
    .await
    .unwrap();
    unit.commit().await.unwrap();
    let public_read = route(
        &mut reader,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity}),
    )
    .await;
    let projection = &public_read["scope_decomposition"];
    assert_eq!(projection["version"], 1);
    assert_eq!(
        projection["manifest"],
        serde_json::to_value(&manifest).unwrap()
    );
    assert_eq!(projection["advice"], serde_json::to_value(&advice).unwrap());
    assert_eq!(
        projection["manifest"]["baseline_id"],
        projection["advice"]["ranked_ids"][0]
    );
    let returned_advice_id = projection["advice"]["id"].clone();
    let returned_selected_id = projection["manifest"]["baseline_id"].clone();
    assert!(
        projection["manifest"]["emitted"]
            .as_array()
            .unwrap()
            .iter()
            .any(|alternative| alternative["id"] == returned_selected_id)
    );
    let selected = &manifest.emitted[0];
    let disposition = route(&mut author, "command", "scope.advisory.disposition", json!({
        "opportunity_id":opportunity,"candidate_set_id":candidate_set,"request_id":Uuid::new_v4(),
        "advice_id":returned_advice_id,"expected_revision":0,"action":"accept",
        "selected_id":returned_selected_id,"items":[{"alternative_id":returned_selected_id,"state":"selected"}],
        "rationale":"Use source-authored preview diagnosis"
    })).await;
    let save_request = Uuid::new_v4();
    let save = json!({
        "kind":"draft","candidate_set_id":candidate_set,"revision":revision,
        "snapshot_id":snapshot,"input_cursor":input_cursor,"request_id":save_request,
        "selected_advisory":{"opportunity_id":opportunity,"disposition_id":disposition["id"],
            "selected_id":selected.id,"alternative_key":"baseline"},
        "draft":draft
    });
    let mut ordinary = save.clone();
    ordinary["request_id"] = json!(Uuid::new_v4());
    ordinary
        .as_object_mut()
        .unwrap()
        .remove("selected_advisory");
    let ordinary_refused =
        route_error(&mut author, "command", "scope.candidates.save", ordinary).await;
    assert_eq!(ordinary_refused["error"]["code"], "forbidden");
    let mut changed = save.clone();
    changed["request_id"] = json!(Uuid::new_v4());
    changed["draft"]["candidates"][0]["title"] = json!("Changed material");
    let refused = route_error(&mut author, "command", "scope.candidates.save", changed).await;
    assert_eq!(refused["error"]["code"], "input_conflict");
    let wrong_session = route_error(
        &mut reader,
        "command",
        "scope.candidates.save",
        save.clone(),
    )
    .await;
    assert_eq!(wrong_session["error"]["code"], "input_conflict");
    let saved = route(&mut author, "command", "scope.candidates.save", save).await;
    assert_eq!(
        saved["context"]["candidate_set"]["status"],
        "review_required"
    );
    assert_eq!(saved["context"]["candidate_set"]["revision"], revision + 1);
    assert_eq!(
        saved["draft"]["candidates"][0]["title"],
        "Selected preview diagnosis"
    );
    let read = route(
        &mut reader,
        "query",
        "scope.candidates.context",
        json!({
            "candidate_set_id":candidate_set,"view":"candidates","limit":25
        }),
    )
    .await;
    assert!(
        read["items"]
            .as_array()
            .unwrap()
            .iter()
            .any(|item| item["candidate"] == saved["draft"]["candidates"][0])
    );
    let links: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM advisory_scope_preservation_receipt WHERE candidate_set_id=$1 AND request_id=$2 AND status='passed'),(SELECT count(*) FROM advisory_scope_caller_link WHERE candidate_set_id=$1 AND request_id=$2),(SELECT count(*) FROM scope_candidate_receipts WHERE candidate_set_id=$1 AND request_id=$2)")
        .bind(candidate_set).bind(save_request).fetch_one(&pool).await.unwrap();
    assert_eq!(links, (1, 1, 1));
    let binding: (Uuid,Uuid,Uuid,Uuid,Uuid,Uuid,i64,Uuid) = sqlx::query_as(
        "SELECT l.opportunity_id,l.disposition_id,l.actor_id,l.session_id,p.request_id,p.receipt_id,l.caller_result_revision,l.link_id FROM advisory_scope_caller_link l JOIN advisory_scope_preservation_receipt p ON (p.tenant_id,p.workspace_id,p.receipt_id)=(l.tenant_id,l.workspace_id,l.preservation_receipt_id) WHERE l.tenant_id=$1 AND l.workspace_id=$2 AND l.candidate_set_id=$3 AND l.request_id=$4"
    ).bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).bind(save_request)
        .fetch_one(&pool).await.unwrap();
    assert_eq!(binding.0, opportunity);
    assert_eq!(binding.1, id(&disposition["id"]));
    assert_eq!(binding.2, actor);
    assert_eq!(binding.3, session);
    assert_eq!(binding.4, save_request);
    assert!(!binding.5.is_nil());
    assert_eq!(
        binding.6,
        saved["context"]["candidate_set"]["revision"]
            .as_i64()
            .unwrap()
    );
    let scopes: i64 = sqlx::query_scalar("SELECT count(*) FROM native_scopes WHERE tenant_id=$1 AND workspace_id=$2 AND source_candidate_set_id=$3")
        .bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).fetch_one(&pool).await.unwrap();
    assert_eq!(scopes, 0);
    let dispatches: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(
        dispatches, 1,
        "only the fixture dispatch exists; MCP did not call a provider"
    );
    let verifier = admin::prepare_verifier_enrollment(&pool, enrollment.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    assert_ne!(verifier.principal_id, enrollment.principal_id);
    let verifier_file = root.join("verifier-host.json");
    host_file(&verifier_file, &verifier.auth);
    let mut verifier_mcp =
        Mcp::start(&socket, &verifier_file, &Uuid::new_v4().to_string(), &key).await;
    route(&mut verifier_mcp, "command", "workspace.open", json!({})).await;
    let verify_request = json!({
        "request_id":Uuid::new_v4(),"opportunity_id":opportunity,"candidate_set_id":candidate_set,
        "caller_link_id":binding.7,"caller_receipt_request_id":save_request,"target_revision":binding.6
    });
    let owner_forbidden = route_error(
        &mut author,
        "command",
        "candidate.advisory.verify",
        verify_request.clone(),
    )
    .await;
    assert_eq!(owner_forbidden["error"]["code"], "forbidden");
    let before_verify: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2)")
        .bind(enrollment.tenant_id).bind(workspace).fetch_one(&pool).await.unwrap();
    let pass = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        verify_request.clone(),
    )
    .await;
    assert_eq!(pass["observation"]["status"], "passed");
    assert_eq!(
        pass["observation"]["qualification"],
        "independently_observed"
    );
    assert_eq!(
        pass["observation"]["actor_id"],
        verifier.principal_id.to_string()
    );
    assert_eq!(pass["establishes_independent_approval"], false);
    assert_eq!(pass["establishes_current_acceptance"], false);
    assert_eq!(
        route(
            &mut verifier_mcp,
            "command",
            "candidate.advisory.verify",
            verify_request.clone()
        )
        .await,
        pass
    );
    let detail = route(
        &mut verifier_mcp,
        "query",
        "candidate.advisory.get",
        json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity}),
    )
    .await;
    assert_eq!(
        detail["opportunity"]["selected_save_observation"]["qualification"],
        "independently_observed"
    );
    assert!(detail.get("scope_decomposition").is_none());
    let audit = route(
        &mut verifier_mcp,
        "query",
        "candidate.advisory.audit",
        json!({"candidate_set_id":candidate_set,"limit":50}),
    )
    .await;
    assert!(audit.to_string().contains("independently_observed"));
    for field in [
        "actor_id",
        "session_id",
        "status",
        "evidence_digest",
        "qualification",
        "verifier_digest",
    ] {
        let mut forged = verify_request.clone();
        forged[field] = json!("forged");
        let rejection = route_error(
            &mut verifier_mcp,
            "command",
            "candidate.advisory.verify",
            forged,
        )
        .await;
        assert_eq!(rejection["error"]["code"], "invalid_arguments", "{field}");
    }
    let mut wrong_target = verify_request.clone();
    wrong_target["caller_link_id"] = json!(Uuid::new_v4());
    assert_eq!(
        route_error(
            &mut verifier_mcp,
            "command",
            "candidate.advisory.verify",
            wrong_target
        )
        .await["error"]["code"],
        "forbidden"
    );
    let another_file = root.join("another-verifier-host.json");
    host_file(&another_file, &verifier.auth);
    let mut another_session =
        Mcp::start(&socket, &another_file, &Uuid::new_v4().to_string(), &key).await;
    route(&mut another_session, "command", "workspace.open", json!({})).await;
    assert_eq!(
        route_error(
            &mut another_session,
            "command",
            "candidate.advisory.verify",
            verify_request.clone()
        )
        .await["error"]["code"],
        "input_conflict"
    );
    let foreign_owner = admin::enroll_host(&pool, None, vec![root.to_string_lossy().into_owned()])
        .await
        .unwrap();
    let foreign_file = root.join("foreign-host.json");
    host_file(&foreign_file, &foreign_owner.auth);
    let mut foreign_mcp =
        Mcp::start(&socket, &foreign_file, &Uuid::new_v4().to_string(), &key).await;
    route(&mut foreign_mcp, "command", "workspace.open", json!({})).await;
    assert_eq!(
        route_error(
            &mut foreign_mcp,
            "query",
            "candidate.advisory.get",
            json!({"candidate_set_id":candidate_set,"opportunity_id":opportunity})
        )
        .await["error"]["code"],
        "not_found"
    );
    let after_verify: (i64,i64,i64) = sqlx::query_as("SELECT (SELECT count(*) FROM scope_candidate_receipts WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_scope_caller_link WHERE tenant_id=$1 AND workspace_id=$2),(SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1 AND workspace_id=$2)")
        .bind(enrollment.tenant_id).bind(workspace).fetch_one(&pool).await.unwrap();
    assert_eq!(before_verify, after_verify);
    let selected_revision = saved["context"]["candidate_set"]["revision"]
        .as_i64()
        .unwrap();
    let partition: (Value, Value) = sqlx::query_as(
        "SELECT obligation_links,non_goal_source_obligation_ids \
         FROM scope_anti_bloat_bindings WHERE tenant_id=$1 AND workspace_id=$2 \
         AND candidate_set_id=$3 AND candidate_set_revision=$4",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(candidate_set)
    .bind(selected_revision)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(partition.0.as_array().unwrap().len(), 1);
    assert_eq!(partition.1.as_array().unwrap().len(), refs.len() - 1);
    let prepared = route(
        &mut author,
        "command",
        "scope.anti_bloat.prepare",
        json!({
            "candidate_set_id":candidate_set,"expected_revision":selected_revision
        }),
    )
    .await;
    assert_eq!(prepared["state"]["status"], "prepared", "{prepared}");
    let review_id = id(&prepared["review_id"]);
    let exploratory_id = id(&saved["draft"]["candidates"][1]["id"]);
    let exploratory_revision = saved["draft"]["candidates"][1]["revision"]
        .as_i64()
        .unwrap();
    let finding = prepared["findings"]
        .as_array()
        .unwrap()
        .iter()
        .find(|item| item["candidate_id"] == exploratory_id.to_string())
        .unwrap();
    assert_eq!(finding["rankable"], true);
    let finding_id = finding["id"].as_str().unwrap();
    let refused = route_error(
        &mut author,
        "command",
        "scope.anti_bloat.run",
        json!({"review_id":review_id}),
    )
    .await;
    assert_eq!(
        refused["error"]["code"], "budget_policy_invalid",
        "{refused}"
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    assert_eq!(
        route_error(
            &mut author,
            "command",
            "scope.anti_bloat.run",
            json!({"review_id":review_id})
        )
        .await,
        refused
    );
    assert_eq!(calls.load(Ordering::SeqCst), 0);
    type SealedReviewRow = (
        String,
        Option<Vec<u8>>,
        Option<String>,
        Option<Vec<u8>>,
        Option<String>,
        bool,
        bool,
    );
    let sealed: SealedReviewRow = sqlx::query_as(
        "SELECT state,request_bytes,request_sha256,raw_response,response_sha256, \
         send_started_at IS NOT NULL,response_sealed_at IS NOT NULL \
         FROM scope_anti_bloat_reviews WHERE tenant_id=$1 AND workspace_id=$2 AND review_id=$3",
    )
    .bind(enrollment.tenant_id)
    .bind(workspace)
    .bind(review_id)
    .fetch_one(&pool)
    .await
    .unwrap();
    assert_eq!(sealed.0, "prepared");
    assert!(sealed.1.is_none() && sealed.2.is_none());
    assert!(sealed.3.is_none() && sealed.4.is_none());
    assert!(!sealed.5 && !sealed.6);
    let apply_params = json!({"review_id":review_id,"finding_id":finding_id,
    "disposition":"narrow","delta":{
        "candidate_set_id":candidate_set,"expected_revision":selected_revision,
        "idempotency_key":format!("public-narrow-{review_id}"),
        "operations":[{"operation":"candidate.remove",
            "candidate_id":exploratory_id,"expected_revision":exploratory_revision}]
    }});
    let applied = route(
        &mut author,
        "command",
        "scope.anti_bloat.apply",
        apply_params.clone(),
    )
    .await;
    assert_eq!(applied["from_revision"], selected_revision);
    assert_eq!(applied["to_revision"], selected_revision + 1);
    assert_eq!(
        route(
            &mut author,
            "command",
            "scope.anti_bloat.apply",
            apply_params
        )
        .await,
        applied
    );
    assert_eq!(
        route_error(
            &mut author,
            "command",
            "scope.anti_bloat.prepare",
            json!({
                "candidate_set_id":candidate_set,"expected_revision":selected_revision
            })
        )
        .await["error"]["code"],
        "not_found"
    );
    let after = route(
        &mut author,
        "query",
        "scope.candidates.context",
        json!({
            "candidate_set_id":candidate_set,"view":"candidates","limit":25
        }),
    )
    .await;
    let candidates = after["items"]
        .as_array()
        .unwrap()
        .iter()
        .filter(|item| item.get("candidate").is_some())
        .collect::<Vec<_>>();
    assert_eq!(candidates.len(), 1);
    assert_eq!(
        candidates[0]["candidate"]["id"],
        saved["draft"]["candidates"][0]["id"]
    );
    assert_eq!(
        after["items"]
            .as_array()
            .unwrap()
            .iter()
            .filter(|item| item.get("goal").is_some())
            .count(),
        saved["draft"]["goals"].as_array().unwrap().len()
    );
    let preserved = route(
        &mut verifier_mcp,
        "query",
        "scope.anti_bloat.preservation.get",
        json!({"review_id":review_id}),
    )
    .await;
    assert_eq!(preserved["verdict"], "pass", "{preserved}");
    for key in [
        "review_id",
        "candidate_set_id",
        "from_revision",
        "to_revision",
        "before_material_digest",
        "after_material_digest",
        "source_digest",
        "caller_request_id",
        "idempotency_key",
    ] {
        assert_eq!(preserved["material"]["receipt"][key], applied[key], "{key}");
    }
    assert_eq!(
        preserved["material"]["after_saved"]["goals"],
        saved["draft"]["goals"]
    );
    let evidence = preserved["evidence_digest"].as_str().unwrap();
    let anti_verify = json!({"request_id":Uuid::new_v4(),"review_id":review_id,
        "expected_evidence_digest":evidence});
    let mut wrong_digest = anti_verify.clone();
    wrong_digest["request_id"] = json!(Uuid::new_v4());
    wrong_digest["expected_evidence_digest"] = json!("f".repeat(64));
    assert_eq!(
        route_error(
            &mut verifier_mcp,
            "command",
            "scope.anti_bloat.preservation.verify",
            wrong_digest
        )
        .await["error"]["code"],
        "input_conflict"
    );
    assert_eq!(
        route_error(
            &mut author,
            "command",
            "scope.anti_bloat.preservation.verify",
            anti_verify.clone()
        )
        .await["error"]["code"],
        "forbidden"
    );
    let attestation = route(
        &mut verifier_mcp,
        "command",
        "scope.anti_bloat.preservation.verify",
        anti_verify.clone(),
    )
    .await;
    assert_eq!(attestation["verdict"], "pass", "{attestation}");
    assert_eq!(
        attestation["verifier_principal_id"],
        verifier.principal_id.to_string()
    );
    assert_eq!(
        route(
            &mut verifier_mcp,
            "command",
            "scope.anti_bloat.preservation.verify",
            anti_verify
        )
        .await,
        attestation
    );
    assert_eq!(
        route_error(
            &mut foreign_mcp,
            "query",
            "scope.anti_bloat.preservation.get",
            json!({"review_id":review_id})
        )
        .await["error"]["code"],
        "forbidden"
    );
    let current_revision: i64 = sqlx::query_scalar(
        "SELECT revision FROM scope_candidate_sets WHERE tenant_id=$1 AND workspace_id=$2 AND id=$3"
    ).bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).fetch_one(&pool).await.unwrap();
    assert_eq!(current_revision, selected_revision + 1);
    sqlx::query("UPDATE scope_candidate_drafts SET payload='{}'::jsonb WHERE tenant_id=$1 AND workspace_id=$2 AND candidate_set_id=$3 AND set_revision=$4")
        .bind(enrollment.tenant_id).bind(workspace).bind(candidate_set).bind(binding.6).execute(&pool).await.unwrap();
    let mut failed_request = verify_request.clone();
    failed_request["request_id"] = json!(Uuid::new_v4());
    let failure = route(
        &mut verifier_mcp,
        "command",
        "candidate.advisory.verify",
        failed_request,
    )
    .await;
    assert_eq!(failure["observation"]["status"], "failed");
    assert_eq!(
        failure["observation"]["qualification"],
        "independently_observed"
    );
    assert!(
        failure["observation"]["reason_codes"]
            .to_string()
            .contains("saved_material_missing_or_mismatched")
    );
    foreign_mcp.finish().await;
    another_session.finish().await;
    verifier_mcp.finish().await;
    author.finish().await;
    reader.finish().await;
    server.abort();
    let _ = server.await;
}
