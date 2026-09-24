//! Live application-to-PostgreSQL Matrix verification. Evidence is synthetic;
//! no provider or JEV transport is configured.
use crate::{PgStore, admin};
use async_trait::async_trait;
use sha2::{Digest, Sha256};
use sqlx::{PgPool, postgres::PgConnectOptions};
use std::{
    str::FromStr,
    sync::Arc,
    time::{SystemTime, UNIX_EPOCH},
};
use tect_application::{
    MatrixEvidenceReference, MatrixEvidenceValidator, RecordMatrixTask, SetupFiles,
    SourceInspector, Store, TransactionMode, VerifyMatrixTask, WorkspaceService,
};
use tect_domain::{
    CommitmentEvidence, EngineeringIntent, EngineeringMatrixInput, EngineeringMode, Error,
    EvidenceValidationOutcome, FactProvenance, MatrixEvidenceBinding, MatrixFact,
    OperatingEnvelope, OperationalFacts, RequestContext, RequiredMatrixFact, Result,
    required_matrix_facts,
};
use uuid::Uuid;

const SYSTEM_ID: &str = "7689109430044371904";
const DATABASE_OID: i64 = 16384;

struct UnusedHostAdapters;

#[async_trait]
impl SourceInspector for UnusedHostAdapters {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<tect_domain::SourceLocation> {
        Err(Error::InternalInvariant)
    }
}

impl SetupFiles for UnusedHostAdapters {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<tect_domain::SetupDirectory> {
        Err(Error::InternalInvariant)
    }
    fn inspect(
        &self,
        _: &tect_domain::SetupDirectory,
        _: usize,
    ) -> Result<tect_domain::FileObservation> {
        Err(Error::InternalInvariant)
    }
    fn publish(
        &self,
        _: &tect_domain::SetupDirectory,
        _: &str,
    ) -> Result<tect_domain::FilePublication> {
        Err(Error::InternalInvariant)
    }
}

struct SyntheticEvidence {
    issued_at: i64,
}

#[async_trait]
impl MatrixEvidenceValidator for SyntheticEvidence {
    fn policy_version(&self) -> &str {
        "synthetic-matrix-policy/1"
    }

    async fn validate(
        &self,
        _: Uuid,
        _: Uuid,
        _: i64,
        fact: &RequiredMatrixFact,
        evidence_ref: &str,
        _now: i64,
    ) -> Result<MatrixEvidenceBinding> {
        Ok(MatrixEvidenceBinding {
            fact_path: fact.path.clone(),
            value_digest: fact.value_digest.clone(),
            evidence_ref: evidence_ref.into(),
            content_digest: format!("{:x}", Sha256::digest(evidence_ref.as_bytes())),
            source: "synthetic-test-source".into(),
            subject: "matrix-test-task".into(),
            observed_at: self.issued_at - 10,
            expires_at: self.issued_at + 3600,
            validation_outcome: EvidenceValidationOutcome::Accepted,
        })
    }
}

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("owner-source".into()),
    }
}

fn complete_input() -> EngineeringMatrixInput {
    EngineeringMatrixInput {
        mode: known(EngineeringMode::Mvp),
        envelope: OperatingEnvelope {
            scale: known("12 workers".into()),
            operational_facts: OperationalFacts::KnownEmpty {
                provenance: FactProvenance("owner-source".into()),
            },
        },
        criticality: known("low".into()),
        intent: known(EngineeringIntent::Other("booking".into())),
        urgency: known("normal".into()),
        promised_behavior: known("books".into()),
        promised_proof: known("acceptance".into()),
        affected_guarantees: MatrixFact::KnownEmpty {
            provenance: FactProvenance("owner-source".into()),
        },
        actual_exposure: known(false),
        demand_commitment: known(CommitmentEvidence::NoCommitment),
        latency_commitment: known(CommitmentEvidence::NoCommitment),
        urgent_repair: known(false),
    }
}

async fn guarded_pools() -> (PgPool, PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let admin_url = std::env::var("TECT_TEST_ADMIN_URL").unwrap();
    let runtime_url = std::env::var("TECT_TEST_RUNTIME_URL").unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(role, "tect_ci");
    let admin_options = PgConnectOptions::from_str(&admin_url).unwrap();
    let runtime_options = PgConnectOptions::from_str(&runtime_url).unwrap();
    assert_eq!(admin_options.get_username(), "postgres");
    assert_eq!(runtime_options.get_username(), role);
    for options in [&admin_options, &runtime_options] {
        assert_eq!(options.get_database(), Some("tect_test"));
        assert_eq!(
            options.get_socket().map(|path| path.to_str().unwrap()),
            Some("/tmp/tectd-matrix-pg18.6-Ry8kWr/socket")
        );
        assert_eq!(options.get_port(), 55586);
    }
    let admin_pool = PgPool::connect_with(admin_options).await.unwrap();
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()),\
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert!((180000..190000).contains(&identity.0));
    assert_eq!(
        (
            identity.1.as_str(),
            identity.2.as_str(),
            identity.3,
            identity.4.as_str()
        ),
        ("tect_test", "postgres", DATABASE_OID, SYSTEM_ID)
    );
    let runtime_identity: (String, String, i64) = sqlx::query_as(
        "SELECT current_database(),current_user,\
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database())",
    )
    .fetch_one(&runtime_pool)
    .await
    .unwrap();
    assert_eq!(
        runtime_identity,
        ("tect_test".into(), role.clone(), DATABASE_OID)
    );
    (admin_pool, runtime_pool, role)
}

#[tokio::test]
#[ignore = "requires explicit opt-in and exact disposable PostgreSQL 18 cluster"]
async fn owner_recorded_matrix_verification_round_trips_and_rejects_conflicts() {
    let (admin_pool, runtime_pool, role) = guarded_pools().await;
    admin::migrate(&admin_pool, &role).await.unwrap();
    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let workspace_id = Uuid::new_v4();
    let workspace_key = format!("matrix-verify-{}", workspace_id.simple());
    sqlx::query("INSERT INTO workspaces (id,tenant_id,key) VALUES ($1,$2,$3)")
        .bind(workspace_id)
        .bind(owner.tenant_id)
        .bind(&workspace_key)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships (tenant_id,workspace_id,principal_id) VALUES ($1,$2,$3)")
        .bind(owner.tenant_id)
        .bind(workspace_id)
        .bind(owner.principal_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    let verifier = admin::prepare_verifier_enrollment(&admin_pool, owner.tenant_id, workspace_id)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let owner_session = Uuid::new_v4();
    let verifier_session = Uuid::new_v4();
    for (session, host) in [
        (owner_session, owner.auth.host_id),
        (verifier_session, verifier.auth.host_id),
    ] {
        sqlx::query("INSERT INTO agent_sessions (id,tenant_id,host_id,workspace_id,native_session_id) VALUES ($1,$2,$3,$4,$5)")
            .bind(Uuid::new_v4()).bind(owner.tenant_id).bind(host)
            .bind(workspace_id).bind(session.to_string())
            .execute(&admin_pool).await.unwrap();
    }
    let adapters = Arc::new(UnusedHostAdapters);
    let store = Arc::new(PgStore::from_pool(runtime_pool.clone()));
    let issued_at = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64;
    let service = WorkspaceService::new(store.clone(), adapters.clone(), adapters)
        .with_matrix_evidence_validator(Arc::new(SyntheticEvidence { issued_at }));
    let owner_context = RequestContext {
        auth: owner.auth.clone(),
        native_session_id: owner_session.to_string(),
        workspace_key: workspace_key.clone(),
    };
    let verifier_context = RequestContext {
        auth: verifier.auth.clone(),
        native_session_id: verifier_session.to_string(),
        workspace_key,
    };
    let task_id = Uuid::new_v4();
    let revision = service
        .record_matrix_task(
            &owner_context,
            &RecordMatrixTask {
                task_id,
                revision: 1,
                expected_current_revision: 0,
                request_id: Uuid::new_v4(),
                input: complete_input(),
                choice_set: None,
            },
        )
        .await
        .unwrap();
    assert_eq!(revision.recorded_by_principal_id, owner.principal_id);
    assert_eq!(
        service
            .get_matrix_task(&verifier_context, task_id)
            .await
            .unwrap()
            .input_digest,
        revision.input_digest
    );
    let request = VerifyMatrixTask {
        task_id,
        expected_revision: 1,
        input_digest: revision.input_digest.clone(),
        evidence: required_matrix_facts(&revision.input)
            .unwrap()
            .into_iter()
            .map(|fact| MatrixEvidenceReference {
                evidence_ref: format!("immutable:synthetic:{}@v1", fact.path),
                fact_path: fact.path,
            })
            .collect(),
    };
    assert_eq!(
        service.verify_matrix_task(&owner_context, &request).await,
        Err(Error::Forbidden)
    );
    let wrong_digest = VerifyMatrixTask {
        input_digest: "a".repeat(64),
        ..request.clone()
    };
    assert_eq!(
        service
            .verify_matrix_task(&verifier_context, &wrong_digest)
            .await,
        Err(Error::InputConflict)
    );
    let stale = VerifyMatrixTask {
        expected_revision: 2,
        ..request.clone()
    };
    assert_eq!(
        service.verify_matrix_task(&verifier_context, &stale).await,
        Err(Error::StaleRevision)
    );
    let record = service
        .verify_matrix_task(&verifier_context, &request)
        .await
        .unwrap();
    assert_eq!(record.owner_principal, owner.principal_id.to_string());
    assert_eq!(record.verifier_principal, verifier.principal_id.to_string());
    assert_eq!(record.input_digest, revision.input_digest);
    assert_eq!(record.task_revision, "1");
    assert_eq!(record.bindings.len(), request.evidence.len());
    for (binding, evidence) in record.bindings.iter().zip(&request.evidence) {
        assert_eq!(binding.fact_path, evidence.fact_path);
        assert_eq!(binding.evidence_ref, evidence.evidence_ref);
        assert_eq!(
            binding.validation_outcome,
            EvidenceValidationOutcome::Accepted
        );
    }
    let replay = service
        .verify_matrix_task(&verifier_context, &request)
        .await
        .unwrap();
    assert_eq!(replay, record);
    let count: i64 = sqlx::query_scalar(
        "SELECT count(*) FROM matrix_verifications WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3")
        .bind(owner.tenant_id).bind(workspace_id).bind(task_id)
        .fetch_one(&admin_pool).await.unwrap();
    assert_eq!(count, 1);
    let mut tx = store.begin(TransactionMode::ReadOnly).await.unwrap();
    tx.authenticate(&verifier.auth).await.unwrap();
    tx.set_tenant(owner.tenant_id).await.unwrap();
    let persisted = tx
        .matrix_verification_store()
        .unwrap()
        .matrix_verification_for_revision(workspace_id, task_id, 1, &revision.input_digest)
        .await
        .unwrap();
    assert_eq!(persisted, Some(record));
    tx.commit().await.unwrap();
    let denied = sqlx::query("UPDATE matrix_verifications SET policy_version='changed' WHERE tenant_id=$1 AND workspace_id=$2 AND task_id=$3")
        .bind(owner.tenant_id).bind(workspace_id).bind(task_id)
        .execute(&runtime_pool).await.unwrap_err();
    assert_eq!(
        denied.as_database_error().unwrap().code().as_deref(),
        Some("42501")
    );
}
