#[allow(dead_code)]
mod recovery_support;
#[path = "native_planning/support.rs"]
#[allow(dead_code)]
mod support;

use recovery_support::{Daemon, Mcp, host_file, private_temp, tagged_url};
use serde_json::{Value, json};
use sqlx::PgPool;
use std::sync::Arc;
use support::{id, repository, route, route_error};
use tect_application::Sha256ScopeDigest;
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
use uuid::Uuid;

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
    admin::migrate(&pool, &role).await.unwrap();
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
    let mut daemon = Daemon::start(&runtime, socket.clone()).await;
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
    let context = &initial["context"];
    let goal = &initial["draft"]["goals"][0];
    let prior_candidate = &initial["draft"]["candidates"][0];
    draft["goals"][0]["identity"] = json!({"id":goal["id"],"revision":goal["revision"]});
    draft["goals"][0]["resolution"]["reference"] = json!({"id":prior_candidate["id"]});
    draft["candidates"][0]["identity"] =
        json!({"id":prior_candidate["id"],"revision":prior_candidate["revision"]});
    draft["candidates"][0]["coverage_goals"] = json!([{"id":goal["id"]}]);
    draft["candidates"][0]["change_rationale"] = json!("Use the selected source-authored title");
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
            covered_source_ref_ids: refs,
        }],
    };
    let (session, actor): (Uuid, Uuid) = sqlx::query_as(
        "SELECT s.id,h.principal_id FROM agent_sessions s JOIN hosts h ON (h.tenant_id,h.id)=(s.tenant_id,s.host_id) WHERE s.tenant_id=$1 AND s.native_session_id=$2"
    ).bind(enrollment.tenant_id).bind(&native_a).fetch_one(&pool).await.unwrap();
    let store = PgStore::connect(&runtime_url, 4).await.unwrap();
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
            observation: observed,
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
    assert_eq!(detail["scope_decomposition"], projection.clone());
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
    daemon.crash().await;
    daemon.remove_owned_stale_socket();
}
