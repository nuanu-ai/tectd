//! Live Matrix advice persistence contract. All provider bytes below are synthetic.

use crate::{admin, store::PgUnitOfWork};
use sha2::{Digest, Sha256};
use sqlx::{PgPool, Row, postgres::PgConnectOptions};
use std::str::FromStr;
use tect_application::{
    GuardedMatrixAdviceOutcome, GuardedMatrixAdviceRecord, MatrixAdviceStore,
    MatrixProviderBinding, UnitOfWork, canonical_matrix_advice_digest,
    canonical_matrix_input_digest,
};
use tect_domain::{
    AdvisoryModelConfiguration, AdvisoryProviderProfileRef, EngineeringCandidate,
    EngineeringChoiceSet, EngineeringMatrixInput, Error, MATRIX_CHOICE_SET_SCHEMA,
    OwnerReportedEngineeringMatrixFacts, compose_owner_reported_engineering_matrix,
    matrix_evaluation_digest,
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
    assert!(admin_identity.4.parse::<u64>().is_ok_and(|id| id != 0));

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
    let task_id = Uuid::new_v4();
    let opportunity_id = Uuid::new_v4();
    let dispatch_id = Uuid::new_v4();
    let input: EngineeringMatrixInput = serde_json::from_value(serde_json::json!({
        "mode": {"state":"absent"},
        "envelope": {"scale":{"state":"absent"},"operational_facts":{"state":"absent"}},
        "criticality":{"state":"absent"}, "intent":{"state":"absent"},
        "urgency":{"state":"absent"}, "promised_behavior":{"state":"absent"},
        "promised_proof":{"state":"absent"}, "affected_guarantees":{"state":"absent"},
        "actual_exposure":{"state":"absent"}, "demand_commitment":{"state":"absent"},
        "latency_commitment":{"state":"absent"}, "urgent_repair":{"state":"absent"}
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
    let composition = compose_owner_reported_engineering_matrix(&reported);
    let evaluation_digest = matrix_evaluation_digest(&input, &composition, &choice)
        .unwrap()
        .unwrap();
    let binding = MatrixProviderBinding {
        task_id,
        task_revision: 1,
        input_digest: input_digest.clone(),
        choice_set_id: choice.choice_set_id.clone(),
        choice_set_version: 1,
        choice_set_digest: choice_digest.clone(),
        evaluation_digest: evaluation_digest.clone(),
    };
    let profile = AdvisoryProviderProfileRef {
        id: "synthetic-provider".into(),
    };
    let model = AdvisoryModelConfiguration {
        model: "synthetic-model".into(),
    };
    let raw = b"synthetic provider response: ranked [a,b]".to_vec();
    let request = b"synthetic provider request".to_vec();
    let request_sha = sha(&request);
    let snapshot = serde_json::json!({
        "provider_profile_ref": profile, "model_configuration": model,
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
    sqlx::query("INSERT INTO agent_sessions (id,tenant_id,host_id,workspace_id,native_session_id) VALUES ($1,$2,$3,$4,$5)")
        .bind(session_id).bind(tenant_id).bind(owner.auth.host_id).bind(workspace_id)
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
    sqlx::query("INSERT INTO advisory_workspace_config_history (tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,changed_by_principal_id,changed_by_session_id) VALUES ($1,$2,0,'optional',$3,$4,$5,$6)")
        .bind(tenant_id).bind(workspace_id).bind(&profile.id).bind(serde_json::json!(model))
        .bind(owner.principal_id).bind(session_id).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_workspace_config (tenant_id,workspace_id,revision,mode,provider_profile_ref,model_configuration,updated_by_principal_id,updated_by_session_id) VALUES ($1,$2,0,'optional',$3,$4,$5,$6)")
        .bind(tenant_id).bind(workspace_id).bind(&profile.id).bind(serde_json::json!(model))
        .bind(owner.principal_id).bind(session_id).execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_opportunity (id,tenant_id,workspace_id,work_item_kind,work_item_id,session_id,authorized_actor_id,source_revision,capability,decision_point,matrix_task_revision,matrix_choice_set_digest,config_revision,session_preference,request_preference,policy_version,request_key,material_digest,state,primary_reason) VALUES ($1,$2,$3,'matrix_task',$4,$5,$6,'1','engineering_profile','engineering.profile.before_selection',1,$7,0,'use_workspace','use_workspace','test-policy',$8,$9,'prepared','dispatch_authorized')")
        .bind(opportunity_id).bind(tenant_id).bind(workspace_id).bind(task_id)
        .bind(session_id).bind(owner.principal_id).bind(&choice_digest)
        .bind(format!("matrix-advice-{}", Uuid::new_v4())).bind(&evaluation_digest)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("INSERT INTO advisory_dispatch (id,tenant_id,workspace_id,opportunity_id,attempt_number,provider,model,configuration_snapshot,configuration_digest,material_digest,payload_digest,request_payload,response_payload,state,send_certainty,outcome,retry_basis,send_started_at,sealed_at) VALUES ($1,$2,$3,$4,1,$5,$6,$7,$8,$9,$10,$11,$12,'sealed','sent','provider_response','initial',clock_timestamp(),clock_timestamp())")
        .bind(dispatch_id).bind(tenant_id).bind(workspace_id).bind(opportunity_id)
        .bind(&profile.id).bind(&model.model).bind(&snapshot).bind(&configuration_digest)
        .bind(&evaluation_digest).bind(&request_sha).bind(&request).bind(&raw)
        .execute(&admin_pool).await.unwrap();
    sqlx::query("UPDATE advisory_opportunity SET state='advised',primary_reason='provider_response' WHERE id=$1")
        .bind(opportunity_id).execute(&admin_pool).await.unwrap();

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
    let dispatch_raw: Vec<u8> =
        sqlx::query("SELECT response_payload FROM advisory_dispatch WHERE id=$1")
            .bind(dispatch_id)
            .fetch_one(&admin_pool)
            .await
            .unwrap()
            .get("response_payload");
    assert_eq!(dispatch_raw, raw);
}
