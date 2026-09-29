use super::*;
use sqlx::postgres::PgConnectOptions;
use std::str::FromStr;
use tect_domain::{
    DeclarationRecorder, DeclaredRequirementValue, EngineeringIntent, EngineeringMatrixInput,
    EngineeringMode, MATRIX_REQUIREMENTS_SCHEMA, MatrixRequirementsConfirmation,
    MatrixRequirementsProposal, MatrixRequirementsRevision, OperatingEnvelope,
    RequirementDeclarationPatch, RequirementsAnchor, required_matrix_operating_facts,
    resolve_matrix_requirements,
};

async fn isolated_pools() -> (PgPool, PgPool, String) {
    assert_eq!(std::env::var("TECT_TEST_DISPOSABLE_PG").as_deref(), Ok("1"));
    let name = std::env::var("TECT_TEST_EXPECTED_DB_NAME").unwrap();
    assert!(name.starts_with("tect_matrix_evidence_"));
    let port: u16 = std::env::var("TECT_TEST_EXPECTED_PG_PORT")
        .unwrap()
        .parse()
        .unwrap();
    assert_ne!(port, 64775);
    let expected_system = std::env::var("TECT_TEST_EXPECTED_PG_SYSTEM_ID").unwrap();
    let expected_oid: i64 = std::env::var("TECT_TEST_EXPECTED_DB_OID")
        .unwrap()
        .parse()
        .unwrap();
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    assert_eq!(role, "tect_ci");
    let admin_options =
        PgConnectOptions::from_str(&std::env::var("TECT_TEST_ADMIN_URL").unwrap()).unwrap();
    let runtime_options =
        PgConnectOptions::from_str(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap()).unwrap();
    for (options, user) in [(&admin_options, "postgres"), (&runtime_options, "tect_ci")] {
        assert_eq!(options.get_username(), user);
        assert_eq!(options.get_database(), Some(name.as_str()));
        assert_eq!(options.get_host(), "127.0.0.1");
        assert_eq!(options.get_port(), port);
        assert!(options.get_socket().is_none());
    }
    let admin_pool = PgPool::connect_with(admin_options).await.unwrap();
    let identity: (i32, String, String, i64, String) = sqlx::query_as(
        "SELECT current_setting('server_version_num')::integer,current_database(),current_user, \
         (SELECT oid::bigint FROM pg_database WHERE datname=current_database()), \
         (SELECT system_identifier::text FROM pg_control_system())",
    )
    .fetch_one(&admin_pool)
    .await
    .unwrap();
    assert!((180000..190000).contains(&identity.0));
    assert_eq!(identity.1, name);
    assert_eq!(identity.2, "postgres");
    assert_eq!(identity.3, expected_oid);
    assert_eq!(identity.4, expected_system);
    let runtime_pool = PgPool::connect_with(runtime_options).await.unwrap();
    let runtime_user: String = sqlx::query_scalar("SELECT current_user")
        .fetch_one(&runtime_pool)
        .await
        .unwrap();
    assert_eq!(runtime_user, role);
    (admin_pool, runtime_pool, role)
}

fn known<T>(value: T) -> MatrixFact<T> {
    MatrixFact::Known {
        value,
        provenance: FactProvenance("source-record:42".into()),
    }
}

fn sourced<T>(fact: T) -> Sourced<T> {
    Sourced {
        fact,
        source_ref: "system-observation:42".into(),
        observed_at: 900,
        expires_at: 1100,
    }
}

fn sample() -> (ApprovedMatrixEvidenceArtifact, String) {
    let approval = ApprovedMatrixEvidenceArtifact {
        tenant_id: Uuid::new_v4(),
        workspace_id: Uuid::new_v4(),
        task_id: Uuid::new_v4(),
        task_revision: 3,
        artifact_id: Uuid::new_v4(),
        artifact_revision: 1,
        sha256: String::new(),
        policy_version: "matrix-snapshot/1".into(),
        max_age_seconds: 100,
    };
    let snapshot = OperatingSnapshot {
        schema: SCHEMA.into(),
        workspace_id: approval.workspace_id,
        task_id: approval.task_id,
        task_revision: approval.task_revision,
        scale: sourced(known("12 workers".into())),
        criticality: sourced(known("low".into())),
        affected_guarantees: sourced(MatrixFact::KnownEmpty {
            provenance: FactProvenance("source-record:42".into()),
        }),
        actual_exposure: sourced(known(false)),
        urgent_repair: sourced(known(false)),
        operational_facts: sourced(OperationalFacts::KnownEmpty {
            provenance: FactProvenance("source-record:42".into()),
        }),
        demand_commitment: None,
        latency_commitment: None,
    };
    (approval, serde_json::to_string(&snapshot).unwrap())
}

fn ready(body: &str) -> (String, i64, String, String, String) {
    (
        format!("{:x}", Sha256::digest(body.as_bytes())),
        body.len() as i64,
        FORMAT.into(),
        "ready".into(),
        body.into(),
    )
}

fn declared_context() -> tect_domain::EffectiveMatrixRequirements {
    let anchor = RequirementsAnchor::Program {
        program_id: Uuid::new_v4(),
    };
    let recorder = DeclarationRecorder {
        principal: "agent".into(),
        session: "session".into(),
    };
    let values = vec![
        DeclaredRequirementValue::Mode(EngineeringMode::Mvp),
        DeclaredRequirementValue::Intent(EngineeringIntent::Other("booking".into())),
        DeclaredRequirementValue::Urgency("normal".into()),
        DeclaredRequirementValue::PromisedBehavior("books".into()),
        DeclaredRequirementValue::PromisedProof("acceptance".into()),
        DeclaredRequirementValue::NoDemandCommitment,
        DeclaredRequirementValue::NoLatencyCommitment,
    ];
    let proposal = MatrixRequirementsProposal::new(
        anchor,
        1,
        values
            .into_iter()
            .map(|value| RequirementDeclarationPatch::Set { value })
            .collect(),
        recorder.clone(),
    )
    .unwrap();
    let confirmation = MatrixRequirementsConfirmation::new(
        &proposal,
        1,
        proposal.digest().into(),
        "owner".into(),
        "approved".into(),
        recorder,
    )
    .unwrap();
    resolve_matrix_requirements(
        &[anchor],
        &[MatrixRequirementsRevision {
            proposal,
            confirmation: Some(confirmation),
        }],
        MATRIX_REQUIREMENTS_SCHEMA,
    )
    .unwrap()
}

#[test]
fn all_six_digests_match_domain_required_operating_facts() {
    let (mut approval, body) = sample();
    let mut snapshot: OperatingSnapshot = serde_json::from_str(&body).unwrap();
    snapshot.affected_guarantees.fact =
        known(vec![ProtectedGuarantee::Data, ProtectedGuarantee::Payment]);
    let input = EngineeringMatrixInput {
        mode: MatrixFact::Absent,
        envelope: OperatingEnvelope {
            scale: snapshot.scale.fact.clone(),
            operational_facts: snapshot.operational_facts.fact.clone(),
        },
        criticality: snapshot.criticality.fact.clone(),
        intent: MatrixFact::Absent,
        urgency: MatrixFact::Absent,
        promised_behavior: MatrixFact::Absent,
        promised_proof: MatrixFact::Absent,
        affected_guarantees: snapshot.affected_guarantees.fact.clone(),
        actual_exposure: snapshot.actual_exposure.fact.clone(),
        demand_commitment: MatrixFact::Absent,
        latency_commitment: MatrixFact::Absent,
        urgent_repair: snapshot.urgent_repair.fact.clone(),
    };
    let required = required_matrix_operating_facts(&declared_context(), &input).unwrap();
    assert_eq!(required.len(), 6);
    let body = serde_json::to_string(&snapshot).unwrap();
    approval.sha256 = ready(&body).0;
    let parsed = checked_snapshot(Some(ready(&body)), &approval).unwrap();
    for fact in required {
        parsed
            .binding(&approval, &fact, &approval.evidence_ref(), 1000)
            .unwrap();
    }
}

#[test]
fn approved_typed_snapshot_binds_exact_fact_and_freshness() {
    let (mut approval, body) = sample();
    approval.sha256 = ready(&body).0;
    let snapshot = checked_snapshot(Some(ready(&body)), &approval).unwrap();
    let observed = snapshot.all_facts().unwrap();
    assert_eq!(observed.len(), 6);
    let fact = RequiredMatrixFact {
        path: "/actual_exposure".into(),
        value_digest: observed["/actual_exposure"].value_digest.clone(),
    };
    let binding = snapshot
        .binding(&approval, &fact, &approval.evidence_ref(), 1000)
        .unwrap();
    assert_eq!(binding.content_digest, approval.sha256);
    assert_eq!(binding.source, "system-observation:42");
    assert_eq!(binding.subject, format!("{}@3", approval.task_id));
    assert_eq!(
        snapshot.binding(&approval, &fact, &approval.evidence_ref(), 1001),
        Err(Error::Forbidden)
    );
    assert_eq!(
        snapshot.binding(&approval, &fact, &approval.evidence_ref(), 1100),
        Err(Error::Forbidden)
    );
}

#[test]
fn missing_changed_unapproved_and_wrong_binding_deny() {
    let (mut approval, body) = sample();
    approval.sha256 = ready(&body).0;
    assert!(matches!(
        checked_snapshot(None, &approval),
        Err(Error::Forbidden)
    ));
    let mut changed = ready(&body);
    changed.4 = changed.4.replace("12 workers", "13 workers");
    assert!(matches!(
        checked_snapshot(Some(changed), &approval),
        Err(Error::Forbidden)
    ));
    let mut forged = ready(&body);
    forged.4 = forged
        .4
        .replace("system-observation:42", "forged-source:42");
    forged.0 = format!("{:x}", Sha256::digest(forged.4.as_bytes()));
    forged.1 = forged.4.len() as i64;
    assert!(matches!(
        checked_snapshot(Some(forged), &approval),
        Err(Error::Forbidden)
    ));
    let mut rejected = ready(&body);
    rejected.3 = "rejected".into();
    assert!(matches!(
        checked_snapshot(Some(rejected), &approval),
        Err(Error::Forbidden)
    ));
    let mut mismatched = approval.clone();
    mismatched.task_revision += 1;
    assert!(matches!(
        checked_snapshot(Some(ready(&body)), &mismatched),
        Err(Error::Forbidden)
    ));
    mismatched = approval.clone();
    mismatched.workspace_id = Uuid::new_v4();
    assert!(matches!(
        checked_snapshot(Some(ready(&body)), &mismatched),
        Err(Error::Forbidden)
    ));
}

#[test]
fn wrong_fact_digest_and_unsupported_payload_deny() {
    let (mut approval, body) = sample();
    approval.sha256 = ready(&body).0;
    let snapshot = checked_snapshot(Some(ready(&body)), &approval).unwrap();
    let wrong = RequiredMatrixFact {
        path: "/actual_exposure".into(),
        value_digest: "0".repeat(64),
    };
    assert!(matches!(snapshot.fact(&wrong), Err(Error::Forbidden)));
    let extra = body.replacen("\"schema\":", "\"forged\":true,\"schema\":", 1);
    let mut extra_approval = approval.clone();
    extra_approval.sha256 = ready(&extra).0;
    assert!(matches!(
        checked_snapshot(Some(ready(&extra)), &extra_approval),
        Err(Error::Forbidden)
    ));
}

#[tokio::test]
async fn wrong_target_or_revision_denies_before_artifact_read() {
    let (mut approval, body) = sample();
    approval.sha256 = ready(&body).0;
    let pool = PgPool::connect_lazy("postgres://unused:unused@127.0.0.1:1/unused").unwrap();
    let validator = PgMatrixEvidenceValidator::new(pool, approval.clone());
    let fact = RequiredMatrixFact {
        path: "/actual_exposure".into(),
        value_digest: "0".repeat(64),
    };
    assert!(matches!(
        validator
            .validate(
                Uuid::new_v4(),
                approval.task_id,
                3,
                &fact,
                &approval.evidence_ref(),
                1000
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        validator
            .validate(
                approval.workspace_id,
                approval.task_id,
                4,
                &fact,
                &approval.evidence_ref(),
                1000
            )
            .await,
        Err(Error::Forbidden)
    ));
    assert!(matches!(
        validator
            .validate(
                approval.workspace_id,
                approval.task_id,
                3,
                &fact,
                "pipeline-evidence:unapproved@1",
                1000
            )
            .await,
        Err(Error::Forbidden)
    ));
}

#[tokio::test]
#[ignore = "requires explicit opt-in and exact disposable PostgreSQL 18 cluster"]
async fn approved_artifact_round_trip_and_revalidation_fail_closed() {
    let (admin_pool, runtime_pool, role) = isolated_pools().await;
    crate::admin::migrate(&admin_pool, &role).await.unwrap();
    let (mut approval, body) = sample();
    approval.sha256 = ready(&body).0;
    sqlx::query(
        "INSERT INTO pipeline_evidence_artifacts \
         (tenant_id,workspace_id,artifact_id,revision,digest,size,format,provenance,target,readiness,body,request_id) \
         VALUES($1,$2,$3,$4,$5,$6,$7,'caller-forged-source','caller-forged-target','ready',$8,$9)",
    )
    .bind(approval.tenant_id)
    .bind(approval.workspace_id)
    .bind(approval.artifact_id)
    .bind(approval.artifact_revision)
    .bind(&approval.sha256)
    .bind(body.len() as i64)
    .bind(FORMAT)
    .bind(&body)
    .bind(Uuid::new_v4())
    .execute(&admin_pool)
    .await
    .unwrap();

    let validator = PgMatrixEvidenceValidator::new(runtime_pool.clone(), approval.clone());
    let snapshot = checked_snapshot(Some(ready(&body)), &approval).unwrap();
    let fact = RequiredMatrixFact {
        path: "/actual_exposure".into(),
        value_digest: snapshot.all_facts().unwrap()["/actual_exposure"]
            .value_digest
            .clone(),
    };
    let binding = validator
        .validate(
            approval.workspace_id,
            approval.task_id,
            approval.task_revision,
            &fact,
            &approval.evidence_ref(),
            1000,
        )
        .await
        .unwrap();
    assert_eq!(binding.source, "system-observation:42");
    assert_eq!(binding.content_digest, approval.sha256);
    validator
        .revalidate(
            approval.workspace_id,
            approval.task_id,
            approval.task_revision,
            &fact,
            &binding,
            1000,
        )
        .await
        .unwrap();

    let mut wrong_tenant = approval.clone();
    wrong_tenant.tenant_id = Uuid::new_v4();
    let wrong_validator = PgMatrixEvidenceValidator::new(runtime_pool.clone(), wrong_tenant);
    assert!(matches!(
        wrong_validator
            .validate(
                approval.workspace_id,
                approval.task_id,
                approval.task_revision,
                &fact,
                &approval.evidence_ref(),
                1000,
            )
            .await,
        Err(Error::Forbidden)
    ));
    let mut tx = runtime_pool.begin().await.unwrap();
    sqlx::query("SELECT pg_catalog.set_config('tect.tenant_id',$1,true)")
        .bind(Uuid::new_v4().to_string())
        .execute(&mut *tx)
        .await
        .unwrap();
    let visible: i64 =
        sqlx::query_scalar("SELECT count(*) FROM pipeline_evidence_artifacts WHERE artifact_id=$1")
            .bind(approval.artifact_id)
            .fetch_one(&mut *tx)
            .await
            .unwrap();
    assert_eq!(visible, 0);
    tx.rollback().await.unwrap();

    sqlx::query(
        "UPDATE pipeline_evidence_artifacts SET body=$1 WHERE tenant_id=$2 AND artifact_id=$3",
    )
    .bind(body.replace("12 workers", "13 workers"))
    .bind(approval.tenant_id)
    .bind(approval.artifact_id)
    .execute(&admin_pool)
    .await
    .unwrap();
    assert!(matches!(
        validator
            .revalidate(
                approval.workspace_id,
                approval.task_id,
                approval.task_revision,
                &fact,
                &binding,
                1000,
            )
            .await,
        Err(Error::Forbidden)
    ));
    sqlx::query(
        "UPDATE pipeline_evidence_artifacts SET body=$1 WHERE tenant_id=$2 AND artifact_id=$3",
    )
    .bind(&body)
    .bind(approval.tenant_id)
    .bind(approval.artifact_id)
    .execute(&admin_pool)
    .await
    .unwrap();
    assert!(matches!(
        validator
            .revalidate(
                approval.workspace_id,
                approval.task_id,
                approval.task_revision,
                &fact,
                &binding,
                1100,
            )
            .await,
        Err(Error::Forbidden)
    ));
    sqlx::query("DELETE FROM pipeline_evidence_artifacts WHERE tenant_id=$1 AND artifact_id=$2")
        .bind(approval.tenant_id)
        .bind(approval.artifact_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    assert!(matches!(
        validator
            .revalidate(
                approval.workspace_id,
                approval.task_id,
                approval.task_revision,
                &fact,
                &binding,
                1000,
            )
            .await,
        Err(Error::Forbidden)
    ));
}
