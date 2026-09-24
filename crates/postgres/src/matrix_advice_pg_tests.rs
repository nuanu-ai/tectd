//! Live Matrix advice persistence contract. All provider bytes below are synthetic.

use crate::{admin, store::PgUnitOfWork};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgConnectOptions};
use std::str::FromStr;
use tect_application::{
    AdvisoryStore, GuardedMatrixAdviceOutcome, GuardedMatrixAdviceRecord, MatrixAdviceStore,
    MatrixProviderBinding, UnitOfWork, canonical_matrix_advice_digest,
    canonical_matrix_input_digest,
};
use tect_domain::{
    AdvisoryAuditQuery, AdvisoryCapability, AdvisoryModelConfiguration, AdvisoryProviderProfileRef,
    EngineeringCandidate, EngineeringChoiceSet, EngineeringMatrixInput, Error,
    EvidenceValidationOutcome, MATRIX_CHOICE_SET_SCHEMA, MATRIX_VERIFICATION_SCHEMA,
    MatrixEvidenceBinding, MatrixVerificationRecord, OwnerReportedEngineeringMatrixFacts,
    compose_independently_verified_owner_matrix, evaluate_matrix_verification,
    matrix_verified_evaluation_digest, required_matrix_facts,
};
use uuid::Uuid;

const EXPECTED_DATABASE: &str = "tect_test";

fn disposable_endpoints(
    admin_url: &str,
    runtime_url: &str,
    role: &str,
) -> (PgConnectOptions, PgConnectOptions) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(role, "tect_ci");
    let admin = PgConnectOptions::from_str(admin_url).expect("valid admin URL");
    let runtime = PgConnectOptions::from_str(runtime_url).expect("valid runtime URL");
    assert_eq!(admin.get_username(), "postgres");
    assert_eq!(runtime.get_username(), role);
    for options in [&admin, &runtime] {
        assert_eq!(options.get_database(), Some(EXPECTED_DATABASE));
        if let Some(socket) = options.get_socket() {
            assert!(
                socket.is_absolute(),
                "test socket must be an absolute local path"
            );
        } else {
            assert!(
                matches!(options.get_host(), "localhost" | "127.0.0.1" | "::1"),
                "test TCP endpoint must be loopback"
            );
        }
    }
    assert_eq!(admin.get_socket(), runtime.get_socket());
    assert_eq!(admin.get_host(), runtime.get_host());
    assert_eq!(admin.get_port(), runtime.get_port());
    (admin, runtime)
}

fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}

async fn verify_disposable_cluster(admin_pool: &PgPool, runtime_pool: &PgPool, role: &str) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    assert_eq!(role, "tect_ci");
    let expected_system_id: u64 = std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID")
        .expect("fresh disposable PostgreSQL system ID required")
        .parse()
        .expect("numeric disposable PostgreSQL system ID required");
    let expected_database_oid: i64 = std::env::var("TECT_TEST_EXPECTED_DB_OID")
        .expect("fresh disposable database OID required")
        .parse()
        .expect("numeric disposable database OID required");
    let admin_identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer, \
                current_database(), current_user, \
                (SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()), \
                (SELECT system_identifier::text FROM pg_catalog.pg_control_system())",
    )
    .fetch_one(admin_pool)
    .await
    .unwrap();
    assert!((180000..190000).contains(&admin_identity.0));
    assert_eq!(admin_identity.1, EXPECTED_DATABASE);
    assert_eq!(admin_identity.2, "postgres");
    assert_eq!(admin_identity.3, expected_database_oid);
    assert_eq!(admin_identity.4, expected_system_id.to_string());

    // Hold the runtime connection while the administrator confirms its backend
    // PID belongs to the same database on this server.
    let mut runtime = runtime_pool.acquire().await.unwrap();
    let runtime_identity: (i32, String, String, i64, i32) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer, \
                current_database(), current_user, \
                (SELECT oid::bigint FROM pg_catalog.pg_database WHERE datname=current_database()), \
                pg_backend_pid()",
    )
    .fetch_one(&mut *runtime)
    .await
    .unwrap();
    assert_eq!(runtime_identity.0, admin_identity.0);
    assert_eq!(runtime_identity.1, EXPECTED_DATABASE);
    assert_eq!(runtime_identity.2, role);
    assert_eq!(runtime_identity.3, admin_identity.3);
    let observed: (String, String) =
        sqlx::query_as("SELECT datname, usename FROM pg_stat_activity WHERE pid=$1")
            .bind(runtime_identity.4)
            .fetch_one(admin_pool)
            .await
            .unwrap();
    assert_eq!(observed, (EXPECTED_DATABASE.into(), role.into()));
}

#[tokio::test]
#[ignore = "requires explicit opt-in and the disposable PostgreSQL 18.6 test cluster"]
async fn guarded_matrix_advice_round_trip_and_raw_byte_conflict() {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    let (admin_options, runtime_options) = disposable_endpoints(&admin_url, &runtime_url, &role);
    let admin_pool = PgPool::connect_with(admin_options).await.unwrap();
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    verify_disposable_cluster(&admin_pool, &runtime_pool, &role).await;
    admin::migrate(&admin_pool, &role).await.unwrap();

    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let tenant_id = owner.tenant_id;
    let workspace_id = Uuid::new_v4();
    let session_id = Uuid::new_v4();
    let verifier_id = Uuid::new_v4();
    let verifier_host = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    let task_id = Uuid::new_v4();
    let opportunity_id = Uuid::new_v4();
    let opportunity_request_key = format!("matrix-advice-{}", Uuid::new_v4());
    let dispatch_id = Uuid::new_v4();
    let input: EngineeringMatrixInput = serde_json::from_value(serde_json::json!({
        "mode":{"state":"known","value":"demo","provenance":"synthetic owner"},
        "envelope":{"scale":{"state":"known","value":"one request","provenance":"synthetic owner"},
          "operational_facts":{"state":"known_empty","provenance":"synthetic owner"}},
        "criticality":{"state":"known","value":"low","provenance":"synthetic owner"},
        "intent":{"state":"known","value":{"kind":"other","description":"demo"},"provenance":"synthetic owner"},
        "urgency":{"state":"known","value":"ordinary","provenance":"synthetic owner"},
        "promised_behavior":{"state":"known","value":"demo","provenance":"synthetic owner"},
        "promised_proof":{"state":"known","value":"check","provenance":"synthetic owner"},
        "affected_guarantees":{"state":"known_empty","provenance":"synthetic owner"},
        "actual_exposure":{"state":"known","value":false,"provenance":"synthetic owner"},
        "demand_commitment":{"state":"known","value":"no_commitment","provenance":"synthetic owner"},
        "latency_commitment":{"state":"known","value":"no_commitment","provenance":"synthetic owner"},
        "urgent_repair":{"state":"known","value":false,"provenance":"synthetic owner"}
    }))
    .unwrap();
    let choice = EngineeringChoiceSet {
        schema: MATRIX_CHOICE_SET_SCHEMA.into(),
        choice_set_id: format!("choice-{}", Uuid::new_v4().simple()),
        version: 1,
        task_id: task_id.to_string(),
        task_revision: "1".into(),
        decision_question: "Which owner-authored approach?".into(),
        candidates: ["a", "b"]
            .into_iter()
            .map(|id| EngineeringCandidate {
                candidate_id: id.into(),
                title: format!("Alternative {id}"),
                approach: format!("Owner-authored approach {id}"),
                assumption_fact_ids: vec![],
            })
            .collect(),
    };
    let canonical_input = serde_json::to_value(&input).unwrap();
    let input_digest = canonical_matrix_input_digest(&canonical_input).unwrap();
    let choice_digest = choice.canonical_digest(&input).unwrap();
    let reported = OwnerReportedEngineeringMatrixFacts::bind_recorded_task_revision(
        task_id.to_string(),
        "1".into(),
        input.clone(),
    )
    .unwrap();
    let now = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let mut verification = MatrixVerificationRecord {
        schema: MATRIX_VERIFICATION_SCHEMA.into(),
        task_id: task_id.to_string(),
        task_revision: "1".into(),
        input_digest: input_digest.clone(),
        owner_principal: owner.principal_id.to_string(),
        verifier_principal: verifier_id.to_string(),
        policy_version: "synthetic-policy/1".into(),
        bindings: required_matrix_facts(&input)
            .unwrap()
            .into_iter()
            .map(|fact| MatrixEvidenceBinding {
                fact_path: fact.path,
                value_digest: fact.value_digest,
                evidence_ref: "synthetic-ref".into(),
                content_digest: sha(b"synthetic-ref"),
                source: "synthetic".into(),
                subject: "matrix-task".into(),
                observed_at: now - 1,
                expires_at: now + 3600,
                validation_outcome: EvidenceValidationOutcome::Accepted,
            })
            .collect(),
        digest: String::new(),
    };
    verification.digest = verification.canonical_digest().unwrap();
    let validated =
        evaluate_matrix_verification(&task_id.to_string(), "1", &input, &verification, now)
            .unwrap();
    let composition = compose_independently_verified_owner_matrix(&reported, &validated).unwrap();
    let evaluation_digest =
        matrix_verified_evaluation_digest(&input, &composition, &choice, &validated).unwrap();
    let verification_digest = verification.digest.clone();
    let binding = MatrixProviderBinding {
        task_id,
        task_revision: 1,
        input_digest: input_digest.clone(),
        choice_set_id: choice.choice_set_id.clone(),
        choice_set_version: 1,
        choice_set_digest: choice_digest.clone(),
        evaluation_digest: evaluation_digest.clone(),
        verification_digest: Some(verification_digest.clone()),
    };
    let profile = AdvisoryProviderProfileRef {
        id: "synthetic-provider".into(),
    };
    let model = AdvisoryModelConfiguration {
        model: "synthetic-model".into(),
    };
    let raw = b"synthetic provider response: ranked [a,b]".to_vec();
    let request = serde_json::to_vec(&serde_json::json!({
        "model": model.model,
        "state": {"binding": {
            "task_id": binding.task_id.to_string(),
            "task_revision": binding.task_revision.to_string(),
            "input_digest": binding.input_digest,
            "choice_set_id": binding.choice_set_id,
            "choice_set_version": binding.choice_set_version,
            "choice_set_digest": binding.choice_set_digest,
            "evaluation_digest": binding.evaluation_digest,
            "verification_digest": binding.verification_digest,
        }}
    }))
    .unwrap();
    let request_sha = sha(&request);
    let snapshot = serde_json::json!({
        "provider_profile_ref": profile, "model_configuration": model,
        "destination": "synthetic-endpoint", "wire_version": "matrix-ranking/2",
        "request_body_length": request.len(), "request_body_sha256": request_sha
    });
    let configuration_digest = sha(&serde_json::to_vec(&snapshot).unwrap());

    // The production APIs do not expose construction of a sealed dispatch.
    // This fixture inserts a prepared Matrix opportunity, then a sealed, sent
    // provider_response dispatch with raw bytes, and advances the opportunity
    // to the exact advised/provider_response post-response state.
    sqlx::query("INSERT INTO workspaces (id,tenant_id,key) VALUES ($1,$2,$3)")
        .bind(workspace_id)
        .bind(tenant_id)
        .bind(format!("matrix-advice-{}", workspace_id.simple()))
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships (tenant_id,workspace_id,principal_id) VALUES ($1,$2,$3)")
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(owner.principal_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO principals (id,tenant_id,role) VALUES ($1,$2,'verifier')")
        .bind(verifier_id)
        .bind(tenant_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO hosts (id,tenant_id,principal_id,credential_digest) VALUES ($1,$2,$3,$4)",
    )
    .bind(verifier_host)
    .bind(tenant_id)
    .bind(verifier_id)
    .bind(sha(verifier_host.to_string().as_bytes()))
    .execute(&admin_pool)
    .await
    .unwrap();
    sqlx::query("INSERT INTO memberships (tenant_id,workspace_id,principal_id) VALUES ($1,$2,$3)")
        .bind(tenant_id)
        .bind(workspace_id)
        .bind(verifier_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO agent_sessions (id,tenant_id,host_id,workspace_id,native_session_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(session_id).bind(tenant_id).bind(owner.auth.host_id).bind(workspace_id)
        .bind(Uuid::new_v4().to_string()).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO agent_sessions (id,tenant_id,host_id,workspace_id,native_session_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(verifier_session).bind(tenant_id).bind(verifier_host).bind(workspace_id)
        .bind(Uuid::new_v4().to_string()).execute(&admin_pool).await.unwrap();
    let mut fixture = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant_id.to_string())
        .execute(&mut *fixture)
        .await
        .unwrap();
    sqlx::query(
        "INSERT INTO matrix_tasks (tenant_id,workspace_id,id,current_revision) VALUES ($1,$2,$3,1)",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(task_id)
    .execute(&mut *fixture)
    .await
    .unwrap();
    sqlx::query("INSERT INTO matrix_task_revisions (tenant_id,workspace_id,task_id,revision,request_id,input_schema,canonical_input,input_digest,choice_set_schema,choice_set,choice_set_digest,recorded_by_principal_id,recorded_by_session_id) VALUES ($1,$2,$3,1,$4,'tect.engineering-matrix-input/1',$5,$6,$7,$8,$9,$10,$11)")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(Uuid::new_v4())
        .bind(&canonical_input).bind(&input_digest).bind(MATRIX_CHOICE_SET_SCHEMA)
        .bind(serde_json::to_value(&choice).unwrap()).bind(&choice_digest)
        .bind(owner.principal_id).bind(session_id).execute(&mut *fixture).await.unwrap();
    fixture.commit().await.unwrap();
    let mut fixture = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant_id.to_string())
        .execute(&mut *fixture)
        .await
        .unwrap();
    let verification_id: Uuid = sqlx::query_scalar("INSERT INTO matrix_verifications (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id,verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) VALUES ($1,$2,$3,1,$4,'tect.matrix-verification/1',$5,$6,$7,'matrix_facts_verified','synthetic-policy/1',$8) RETURNING id")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(&input_digest)
        .bind(owner.principal_id).bind(verifier_id).bind(verifier_session)
        .bind(&verification_digest).fetch_one(&mut *fixture).await.unwrap();
    for binding in &verification.bindings {
        sqlx::query("INSERT INTO matrix_verification_bindings (tenant_id,workspace_id,verification_id,fact_path,value_digest,evidence_ref,content_digest,source,subject,observed_at,expires_at,validation_outcome) VALUES ($1,$2,$3,$4,$5,$6,$7,$8,$9,$10,$11,'accepted')")
            .bind(tenant_id).bind(workspace_id).bind(verification_id)
            .bind(&binding.fact_path).bind(&binding.value_digest).bind(&binding.evidence_ref)
            .bind(&binding.content_digest).bind(&binding.source).bind(&binding.subject)
            .bind(binding.observed_at).bind(binding.expires_at)
            .execute(&mut *fixture).await.unwrap();
    }
    fixture.commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config_history (tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES ($1,$2,0,'optional',$3,$4,$5,$6)")
        .bind(tenant_id).bind(workspace_id).bind(&profile.id).bind(serde_json::json!(model))
        .bind(owner.principal_id).bind(session_id).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config (tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES ($1,$2,0,'optional',$3,$4,$5,$6)")
        .bind(tenant_id).bind(workspace_id).bind(&profile.id).bind(serde_json::json!(model))
        .bind(owner.principal_id).bind(session_id).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity (id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,matrix_task_revision,matrix_choice_set_digest,matrix_verification_digest,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES ($1,$2,$3,'matrix_task',$4,$5,$6,'1','engineering_profile','engineering.profile.before_selection',1,$7,$8,0,'use_workspace','use_workspace','test-policy',$9,$10,'prepared','dispatch_authorized')")
        .bind(opportunity_id).bind(tenant_id).bind(workspace_id).bind(task_id)
        .bind(session_id).bind(owner.principal_id).bind(&choice_digest).bind(&verification_digest)
        .bind(&opportunity_request_key).bind(&evaluation_digest)
        .execute(&admin_pool).await.unwrap();
    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    assert!(
        !unit
            .advisory_opportunity_for_dispatch(workspace_id, opportunity_id)
            .await
            .unwrap()
            .provider_called
    );
    assert!(
        !unit
            .advisory_opportunity_by_request(workspace_id, &opportunity_request_key)
            .await
            .unwrap()
            .unwrap()
            .provider_called
    );
    Box::new(unit).commit().await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch (id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES ($1,$2,$3,$4,1,$5,$6,$7,$8,$9,$10,$11,$12,'sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(dispatch_id).bind(tenant_id).bind(workspace_id).bind(opportunity_id)
        .bind(&profile.id).bind(&model.model).bind(&snapshot).bind(&configuration_digest)
        .bind(&evaluation_digest).bind(&request_sha).bind(&request).bind(&raw)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity_id).execute(&admin_pool).await.unwrap();

    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    let by_id = unit
        .advisory_opportunity_for_dispatch(workspace_id, opportunity_id)
        .await
        .unwrap();
    let by_key = unit
        .advisory_opportunity_by_request(workspace_id, &opportunity_request_key)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(by_id.id, opportunity_id);
    assert!(by_id.provider_called);
    assert_eq!(by_key.id, opportunity_id);
    assert!(by_key.provider_called);
    Box::new(unit).commit().await.unwrap();

    let mut recovery_tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant_id.to_string())
        .execute(&mut *recovery_tx)
        .await
        .unwrap();
    let recovered = crate::advisory::matrix_dispatch_for_recovery(
        &mut recovery_tx,
        tenant_id,
        workspace_id,
        owner.principal_id,
        opportunity_id,
        None,
    )
    .await
    .unwrap();
    assert_eq!(recovered.binding, binding);
    assert_eq!(recovered.request_payload, request);
    assert_eq!(recovered.request_payload_sha256, request_sha);
    assert_eq!(recovered.response_payload, Some(raw.clone()));
    assert_eq!(recovered.response_payload_sha256, Some(sha(&raw)));
    assert_eq!(
        recovered.dispatch.state,
        tect_domain::AdvisoryDispatchState::Sealed
    );
    assert_eq!(
        recovered.dispatch.send_certainty,
        tect_domain::AdvisorySendCertainty::Sent
    );
    assert_eq!(
        crate::advisory::matrix_dispatch_for_recovery(
            &mut recovery_tx,
            tenant_id,
            workspace_id,
            verifier_id,
            opportunity_id,
            Some(dispatch_id),
        )
        .await
        .err(),
        Some(Error::NotFound)
    );
    recovery_tx.commit().await.unwrap();

    let outcome = GuardedMatrixAdviceOutcome::Ranked {
        ranked_choice_ids: vec!["a".into(), "b".into()],
    };
    let record = GuardedMatrixAdviceRecord {
        opportunity_id,
        dispatch_id,
        opportunity_material_digest: evaluation_digest,
        binding: binding.clone(),
        provider_profile_ref: profile,
        model_configuration: model,
        raw_response_payload: raw.clone(),
        response_payload_sha256: sha(&raw),
        advice_digest: canonical_matrix_advice_digest(&binding, &outcome).unwrap(),
        outcome,
    };
    // A correctly self-hashed but byte-different response must fail against
    // the sealed dispatch before any advice row exists.
    let mut tampered = record.clone();
    tampered.raw_response_payload.push(b'!');
    tampered.response_payload_sha256 = sha(&tampered.raw_response_payload);
    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    assert_eq!(
        unit.persist_guarded_matrix_advice(workspace_id, &tampered)
            .await,
        Err(Error::InputConflict)
    );
    Box::new(unit).commit().await.unwrap();
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM advisory_matrix_advice WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3",
    )
    .bind(tenant_id)
    .bind(workspace_id)
    .bind(opportunity_id)
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert_eq!(count, 0);

    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    let saved = unit
        .persist_guarded_matrix_advice(workspace_id, &record)
        .await
        .unwrap();
    assert_eq!(saved.record, record);
    Box::new(unit).commit().await.unwrap();

    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    let read = unit
        .guarded_matrix_advice(workspace_id, opportunity_id)
        .await
        .unwrap()
        .unwrap();
    assert_eq!(read, saved);
    assert_eq!(read.record.raw_response_payload, raw);
    let audit = unit
        .advisory_audit(
            workspace_id,
            None,
            &AdvisoryAuditQuery {
                limit: 1,
                scope_id: None,
                after: None,
                capability: Some(AdvisoryCapability::EngineeringProfile),
                decision_point: None,
                reason: None,
                state: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(audit.opportunities.len(), 1);
    let audited = &audit.opportunities[0];
    assert_eq!(audited.id, opportunity_id);
    assert_eq!(audited.guarded_advice_id, Some(saved.advice_id));
    assert_eq!(
        audited.guarded_advice_digest,
        Some(record.advice_digest.clone())
    );
    assert_eq!(audited.disposition_id, None);
    assert_eq!(audited.caller_receipt_id, None);
    assert_eq!(audited.verifier_receipt_id, None);
    let audit_json = serde_json::to_string(&audit).unwrap();
    assert!(!audit_json.contains("synthetic provider response"));
    assert_eq!(
        unit.persist_guarded_matrix_advice(workspace_id, &record)
            .await
            .unwrap()
            .advice_id,
        saved.advice_id
    );
    assert_eq!(
        unit.persist_guarded_matrix_advice(workspace_id, &tampered)
            .await,
        Err(Error::InputConflict)
    );
    Box::new(unit).commit().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3")
        .bind(tenant_id).bind(workspace_id).bind(opportunity_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(count, 1);
    // A separate no-call Matrix opportunity can carry a blocked disposition
    // without fabricating advice, caller, or verifier evidence.
    let no_call_id = Uuid::new_v4();
    let mut no_call = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant_id.to_string())
        .execute(&mut *no_call)
        .await
        .unwrap();
    sqlx::query("INSERT INTO advisory_opportunity (id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,matrix_task_revision,matrix_choice_set_digest,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES ($1,$2,$3,'matrix_task',$4,$5,$6,'1','engineering_profile','engineering.profile.before_selection',1,$7,0,'use_workspace','skip','test-policy',$8,$9,'no_call','request_skip')")
        .bind(no_call_id).bind(tenant_id).bind(workspace_id).bind(task_id)
        .bind(session_id).bind(owner.principal_id).bind(&choice_digest)
        .bind(format!("matrix-no-call-{}", Uuid::new_v4())).bind(&record.opportunity_material_digest)
        .execute(&mut *no_call).await.unwrap();
    let blocked_disposition_id: Uuid = sqlx::query_scalar("INSERT INTO advisory_matrix_disposition (tenant_id,workspace_id,opportunity_id,task_id,matrix_task_revision,matrix_choice_set_digest,request_id,actor_id,session_id,basis,outcome,blocked_reason) VALUES ($1,$2,$3,$4,1,$5,$6,$7,$8,'no_call','blocked','synthetic owner block') RETURNING disposition_id")
        .bind(tenant_id).bind(workspace_id).bind(no_call_id).bind(task_id)
        .bind(&choice_digest).bind(Uuid::new_v4()).bind(owner.principal_id)
        .bind(session_id).fetch_one(&mut *no_call).await.unwrap();
    no_call.commit().await.unwrap();

    let mut audit_unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    let audit = audit_unit
        .advisory_audit(
            workspace_id,
            None,
            &AdvisoryAuditQuery {
                limit: 2,
                scope_id: None,
                after: None,
                capability: Some(AdvisoryCapability::EngineeringProfile),
                decision_point: None,
                reason: None,
                state: None,
            },
        )
        .await
        .unwrap();
    Box::new(audit_unit).commit().await.unwrap();
    assert_eq!(audit.opportunities.len(), 2);
    let blocked = audit
        .opportunities
        .iter()
        .find(|item| item.id == no_call_id)
        .unwrap();
    assert_eq!(blocked.guarded_advice_id, None);
    assert_eq!(blocked.guarded_advice_digest, None);
    assert_eq!(blocked.disposition_id, Some(blocked_disposition_id));
    assert_eq!(blocked.caller_receipt_id, None);
    assert_eq!(blocked.verifier_receipt_id, None);
    let dispatch_raw: Vec<u8> =
        sqlx::query("SELECT response_payload FROM advisory_dispatch WHERE id=$1")
            .bind(dispatch_id)
            .fetch_one(&admin_pool)
            .await
            .unwrap()
            .get("response_payload");
    assert_eq!(dispatch_raw, raw);

    // A newer immutable verification supersedes the bound header. Replaying
    // an otherwise exact advice record now fails closed, including when the
    // replacement's evidence has already expired.
    let replacement_digest = sha(format!("replacement-{task_id}").as_bytes());
    let mut replacement = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(tenant_id.to_string())
        .execute(&mut *replacement)
        .await
        .unwrap();
    let replacement_id: Uuid = sqlx::query_scalar("INSERT INTO matrix_verifications (tenant_id,workspace_id,task_id,task_revision,input_digest,schema,owner_principal_id,verifier_principal_id,verifier_session_id,verification_reason,policy_version,record_digest) VALUES ($1,$2,$3,1,$4,'tect.matrix-verification/1',$5,$6,$7,'matrix_facts_verified','synthetic-policy/1',$8) RETURNING id")
        .bind(tenant_id).bind(workspace_id).bind(task_id).bind(&input_digest)
        .bind(owner.principal_id).bind(verifier_id).bind(verifier_session)
        .bind(&replacement_digest).fetch_one(&mut *replacement).await.unwrap();
    sqlx::query("INSERT INTO matrix_verification_bindings (tenant_id,workspace_id,verification_id,fact_path,value_digest,evidence_ref,content_digest,source,subject,observed_at,expires_at,validation_outcome) VALUES ($1,$2,$3,'mode',$4,'expired-ref',$5,'synthetic','matrix-task',$6,$7,'accepted')")
        .bind(tenant_id).bind(workspace_id).bind(replacement_id).bind(&input_digest)
        .bind(sha(b"expired-ref")).bind(now - 20).bind(now - 10)
        .execute(&mut *replacement).await.unwrap();
    replacement.commit().await.unwrap();
    let mut unit = PgUnitOfWork::test_begin(&runtime_pool, tenant_id).await;
    assert_eq!(
        unit.persist_guarded_matrix_advice(workspace_id, &record)
            .await,
        Err(Error::StaleRevision)
    );
    Box::new(unit).commit().await.unwrap();
    let count: i64 = sqlx::query_scalar("SELECT count(*) FROM advisory_matrix_advice WHERE tenant_id=$1 AND workspace_id=$2 AND opportunity_id=$3")
        .bind(tenant_id).bind(workspace_id).bind(opportunity_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(count, 1);
}
