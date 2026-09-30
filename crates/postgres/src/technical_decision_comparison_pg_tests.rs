//! Real PostgreSQL/service comparison. Approval here is a synthetic admin
//! CONTROL FIXTURE, never evidence of genuine owner acceptance or deployment.
use crate::technical_decision_evidence::*;
use crate::{ApprovedMatrixEvidenceArtifact, PgMatrixEvidenceValidator, PgStore, admin};
use async_trait::async_trait;
use serde_json::{Value, json};
use sha2::{Digest, Sha256};
use sqlx::PgPool;
use std::{
    sync::Arc,
    time::{Duration, SystemTime, UNIX_EPOCH},
};
use tect_application::*;
use tect_domain::*;
use uuid::Uuid;

#[path = "model_route_live_tests/positive_input.rs"]
mod input;

pub(super) struct NoExternal;
#[async_trait]
impl SourceInspector for NoExternal {
    async fn inspect(&self, _: &str, _: &[String]) -> Result<SourceLocation> {
        panic!("no source inspection")
    }
}
impl SetupFiles for NoExternal {
    fn resolve_directory(&self, _: &str, _: &[String]) -> Result<SetupDirectory> {
        panic!("no setup resolution")
    }
    fn inspect(&self, _: &SetupDirectory, _: usize) -> Result<FileObservation> {
        panic!("no setup inspection")
    }
    fn publish(&self, _: &SetupDirectory, _: &str) -> Result<FilePublication> {
        panic!("no publication")
    }
}
fn sha(bytes: &[u8]) -> String {
    format!("{:x}", Sha256::digest(bytes))
}
fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap()
        .as_secs() as i64
}

async fn artifact(
    pool: &PgPool,
    tenant: Uuid,
    workspace: Uuid,
    id: Uuid,
    body: &str,
    format: &str,
) {
    sqlx::query("INSERT INTO pipeline_evidence_artifacts(tenant_id,workspace_id,artifact_id,revision,digest,size,format,provenance,target,readiness,body,request_id) VALUES($1,$2,$3,1,$4,$5,$6,'SYNTHETIC ADMIN CONTROL FIXTURE','test only','ready',$7,$8)")
        .bind(tenant).bind(workspace).bind(id).bind(sha(body.as_bytes())).bind(body.len() as i64)
        .bind(format).bind(body).bind(Uuid::new_v4()).execute(pool).await.unwrap();
}
pub(super) fn service(
    pool: &PgPool,
    operating: &ApprovedMatrixEvidenceArtifact,
    approvals: Vec<ApprovedTechnicalDecisionEvidence>,
) -> WorkspaceService {
    let guards = Arc::new(NoExternal);
    WorkspaceService::new(
        Arc::new(PgStore::from_pool(pool.clone())),
        guards.clone(),
        guards,
    )
    .with_matrix_evidence_validator(Arc::new(PgMatrixEvidenceValidator::new(
        pool.clone(),
        operating.clone(),
    )))
    .with_technical_decision_evidence_resolver(Arc::new(
        PgTechnicalDecisionEvidenceResolver::new(pool.clone(), approvals),
    ))
}
fn control_approval(
    binding: ServerTechnicalDecisionTaskBinding,
    delegated: Uuid,
    observed: i64,
) -> (ApprovedTechnicalDecisionEvidence, String) {
    let mapping: Vec<_> = binding
        .choice_set
        .candidates
        .iter()
        .enumerate()
        .map(|(i, c)| TechnicalDecisionCandidateMapping {
            frozen_candidate: c.clone(),
            technical_approach: DeliveryApproach {
                id: c.candidate_id.clone(),
                title: c.title.clone(),
                mechanism: c.approach.clone(),
                kind: if i == 0 {
                    DeliveryApproachKind::ReuseExistingPath
                } else {
                    DeliveryApproachKind::SeparateMechanism
                },
                operational_consequences: vec!["Synthetic control consequence".into()],
            },
        })
        .collect();
    let kinds = [
        TechnicalFactKind::ReuseSourceSupport,
        TechnicalFactKind::SeparateSourceSupport,
        TechnicalFactKind::ReuseMeetsOutcome,
        TechnicalFactKind::ReuseOperationsAcceptable,
        TechnicalFactKind::SeparateMeetsOutcome,
        TechnicalFactKind::SeparateOperationsAcceptable,
        TechnicalFactKind::SeparateRequiredByConstraint,
    ];
    let artifact = TechnicalDecisionEvidenceArtifact {
        schema: TECHNICAL_EVIDENCE_SCHEMA.into(),
        tenant_id: binding.tenant_id,
        workspace_id: binding.workspace_id,
        task_id: binding.task_id,
        task_revision: binding.task_revision,
        operating_verification_digest: binding.operating_verification_digest.clone(),
        operating_policy_version: binding.operating_policy_version.clone(),
        requirements: TechnicalEvidenceRequirements {
            locator: (&binding.requirements_binding.locator).into(),
            snapshot_id: binding.requirements_binding.snapshot_id,
            semantic_digest: binding.requirements_binding.semantic_digest.clone(),
            authority_schema: binding.requirements_binding.authority_schema.clone(),
        },
        choice_set_digest: binding.choice_set_digest.clone(),
        decision_question: binding.choice_set.decision_question.clone(),
        required_outcome: "Synthetic control outcome".into(),
        candidate_mapping: mapping
            .iter()
            .map(|m| TechnicalEvidenceCandidateMapping {
                frozen_candidate: m.frozen_candidate.clone(),
                technical_approach: m.technical_approach.clone(),
            })
            .collect(),
        facts: kinds
            .into_iter()
            .enumerate()
            .map(|(i, kind)| TechnicalEvidenceObservation {
                kind,
                value: if i < 2 {
                    TechnicalFactValue::SourceSupport(TechnicalSourceSupport::Supported)
                } else {
                    TechnicalFactValue::Determination(i != 6)
                },
                source_ref: format!("synthetic-control-observation:{i}"),
                observed_at: observed,
                expires_at: observed + 3600,
            })
            .collect(),
    };
    let body = serde_json::to_string(&artifact).unwrap();
    let reference = TechnicalDecisionEvidenceReference {
        artifact_id: Uuid::new_v4(),
        artifact_version: 1,
        content_sha256: sha(body.as_bytes()),
    };
    let facts = artifact
        .facts
        .iter()
        .map(|o| TechnicalDecisionFact {
            kind: o.kind,
            observation: TechnicalFactObservation::Verified {
                value: o.value,
                binding: TechnicalEvidenceBinding {
                    task_id: binding.task_id.to_string(),
                    task_revision: binding.task_revision.to_string(),
                    matrix_verification_digest: binding.operating_verification_digest.clone(),
                    evidence_ref: format!("pipeline-evidence:{}@1", reference.artifact_id),
                    content_digest: reference.content_sha256.clone(),
                    validator_policy_version: TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION.into(),
                    observed_at: o.observed_at,
                    expires_at: o.expires_at,
                    validation_outcome: EvidenceValidationOutcome::Accepted,
                },
            },
        })
        .collect();
    let mut card = DeliveryMechanismDecisionCard {
        schema: TECHNICAL_DECISION_SCHEMA.into(),
        card_id: DELIVERY_MECHANISM_CARD.into(),
        task_id: binding.task_id.to_string(),
        task_revision: binding.task_revision.to_string(),
        matrix_verification_digest: binding.operating_verification_digest.clone(),
        decision_question: artifact.decision_question,
        required_outcome: artifact.required_outcome,
        approaches: mapping
            .iter()
            .map(|m| m.technical_approach.clone())
            .collect(),
        facts,
        owner_approval: TechnicalOwnerApprovalClaim {
            approval_ref: "SYNTHETIC ADMIN CONTROL APPROVAL ONLY".into(),
            approving_principal: delegated.to_string(),
            authority: TechnicalApprovalAuthority::OwnerDelegated,
            task_id: binding.task_id.to_string(),
            task_revision: binding.task_revision.to_string(),
            candidate_digest: String::new(),
            approved_at: observed,
        },
    };
    card.owner_approval.candidate_digest = card.candidate_digest().unwrap();
    let approval = TechnicalDecisionApprovalRecord {
        claim: card.owner_approval.clone(),
        card_digest: card.canonical_digest().unwrap(),
        candidate_digest: card.candidate_digest().unwrap(),
        choice_set_digest: binding.choice_set_digest.clone(),
        recorded_by_principal_id: binding.recorded_by_principal_id,
        owner_author_principal_id: binding.recorded_by_principal_id,
        owner_authorship_ref: "SYNTHETIC OWNER-AUTHORED CANDIDATES".into(),
    };
    (
        ApprovedTechnicalDecisionEvidence {
            binding,
            reference,
            approval,
            candidate_mapping: mapping,
            validator_policy_version: TECHNICAL_DECISION_VALIDATOR_POLICY_VERSION.into(),
            max_age_seconds: 3600,
        },
        body,
    )
}

fn control_whitelist_json(a: &ApprovedTechnicalDecisionEvidence) -> String {
    let b = &a.binding;
    let p = &a.approval;
    let r = &a.reference;
    let requirements = &b.requirements_binding;
    serde_json::to_string(&json!([{
        "binding":{"tenant_id":b.tenant_id,"workspace_id":b.workspace_id,"task_id":b.task_id,"task_revision":b.task_revision,
            "operating_verification_digest":b.operating_verification_digest,"operating_policy_version":b.operating_policy_version,
            "requirements_binding":{"locator":TechnicalEvidenceLocator::from(&requirements.locator),"snapshot_id":requirements.snapshot_id,"semantic_digest":requirements.semantic_digest,"authority_schema":requirements.authority_schema},
            "choice_set":b.choice_set,"choice_set_digest":b.choice_set_digest,"recorded_by_principal_id":b.recorded_by_principal_id},
        "reference":{"artifact_id":r.artifact_id,"artifact_version":r.artifact_version,"content_sha256":r.content_sha256},
        "approval":{"claim":p.claim,"card_digest":p.card_digest,"candidate_digest":p.candidate_digest,"choice_set_digest":p.choice_set_digest,
            "recorded_by_principal_id":p.recorded_by_principal_id,"owner_author_principal_id":p.owner_author_principal_id,"owner_authorship_ref":p.owner_authorship_ref},
        "candidate_mapping":a.candidate_mapping.iter().map(|m|TechnicalEvidenceCandidateMapping{frozen_candidate:m.frozen_candidate.clone(),technical_approach:m.technical_approach.clone()}).collect::<Vec<_>>(),
        "validator_policy_version":a.validator_policy_version,"max_age_seconds":a.max_age_seconds
    }])).unwrap()
}

async fn read_effect_counts(pool: &PgPool, tenant: Uuid) -> (i64, i64, i64, i64, i64) {
    sqlx::query_as("SELECT (SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1),(SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1),(SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1),(SELECT count(*) FROM matrix_verifications WHERE tenant_id=$1),(SELECT count(*) FROM model_route_advisory_attempts WHERE tenant_id=$1)")
        .bind(tenant).fetch_one(pool).await.unwrap()
}

async fn declaration_lock_released(pool: &PgPool, tenant: Uuid, workspace: Uuid, program: Uuid) {
    let key = format!(
        "matrix-requirements:{tenant}:{workspace}:{}",
        serde_json::to_value(RequirementsAnchor::Program {
            program_id: program
        })
        .unwrap()
    );
    tokio::time::timeout(Duration::from_secs(2),async {
        loop {
            let held:bool=sqlx::query_scalar("SELECT EXISTS(SELECT 1 FROM pg_locks WHERE locktype='advisory' AND database=(SELECT oid FROM pg_database WHERE datname=current_database()) AND classid=((hashtextextended($1,0)>>32)&4294967295)::oid AND objid=(hashtextextended($1,0)&4294967295)::oid AND objsubid=1)")
                .bind(&key).fetch_one(pool).await.unwrap();
            if !held {break;}
            tokio::task::yield_now().await;
        }
    }).await.expect("dropped comparison UoW must release declaration lock promptly");
}

pub(super) struct ControlFixture {
    pub admin_pool: PgPool,
    pub runtime: PgPool,
    pub owner: admin::Enrollment,
    pub workspace: Uuid,
    pub program: Uuid,
    pub task: Uuid,
    pub sessions: [Uuid; 3],
    pub owner_context: RequestContext,
    pub compare_context: RequestContext,
    pub locator: MatrixRequirementsLocator,
    pub operating: ApprovedMatrixEvidenceArtifact,
    pub approved: ApprovedTechnicalDecisionEvidence,
    pub technical_body: String,
    pub request: CompareTechnicalDeliveryMechanisms,
    pub delegated: Uuid,
}

pub(super) async fn control_fixture() -> ControlFixture {
    let admin_pool = PgPool::connect(&std::env::var("TECT_TEST_ADMIN_URL").unwrap())
        .await
        .unwrap();
    let identity:(String,i32,String)=sqlx::query_as("SELECT current_setting('data_directory'),current_setting('server_version_num')::integer,current_database()")
        .fetch_one(&admin_pool).await.unwrap();
    assert_eq!(
        identity,
        (
            "/private/tmp/jev-pg-proof.BptdZTGr/data".into(),
            180006,
            "jev_contention".into()
        )
    );
    let role = std::env::var("TECT_TEST_RUNTIME_ROLE").unwrap();
    admin::migrate(&admin_pool, &role).await.unwrap();
    let runtime = PgPool::connect(&std::env::var("TECT_TEST_RUNTIME_URL").unwrap())
        .await
        .unwrap();
    crate::runtime::verify_runtime_role(&runtime).await.unwrap();
    let owner = admin::enroll_host(&admin_pool, None, vec![]).await.unwrap();
    let workspace = Uuid::new_v4();
    let program = Uuid::new_v4();
    let task = Uuid::new_v4();
    let workspace_key = format!("technical-control-{workspace}");
    sqlx::query("INSERT INTO workspaces(id,tenant_id,key) VALUES($1,$2,$3)")
        .bind(workspace)
        .bind(owner.tenant_id)
        .bind(&workspace_key)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO memberships(tenant_id,workspace_id,principal_id) VALUES($1,$2,$3)")
        .bind(owner.tenant_id)
        .bind(workspace)
        .bind(owner.principal_id)
        .execute(&admin_pool)
        .await
        .unwrap();
    sqlx::query("INSERT INTO programs(id,tenant_id,workspace_id,status,revision,current_step,input_cursor,latest_input,max_input_bytes) VALUES($1,$2,$3,'draft',1,'compose',0,1,65536)").bind(program).bind(owner.tenant_id).bind(workspace).execute(&admin_pool).await.unwrap();
    let verifier = admin::prepare_verifier_enrollment(&admin_pool, owner.tenant_id, workspace)
        .await
        .unwrap()
        .try_commit()
        .await
        .unwrap();
    let sessions = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    let native_ids = [Uuid::new_v4(), Uuid::new_v4(), Uuid::new_v4()];
    for ((session, host), native) in [
        (sessions[0], owner.auth.host_id),
        (sessions[1], owner.auth.host_id),
        (sessions[2], verifier.auth.host_id),
    ]
    .into_iter()
    .zip(native_ids)
    {
        sqlx::query("INSERT INTO agent_sessions(id,tenant_id,host_id,workspace_id,native_session_id) VALUES($1,$2,$3,$4,$5)")
            .bind(session).bind(owner.tenant_id).bind(host).bind(workspace).bind(native.to_string()).execute(&admin_pool).await.unwrap();
    }
    let owner_context = RequestContext {
        auth: owner.auth.clone(),
        native_session_id: native_ids[0].to_string(),
        workspace_key: workspace_key.clone(),
    };
    let compare_context = RequestContext {
        native_session_id: native_ids[1].to_string(),
        ..owner_context.clone()
    };
    let verifier_context = RequestContext {
        auth: verifier.auth.clone(),
        native_session_id: native_ids[2].to_string(),
        workspace_key: workspace_key.clone(),
    };
    let guards = Arc::new(NoExternal);
    let base = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime.clone())),
        guards.clone(),
        guards,
    );
    let locator = MatrixRequirementsLocator::Program {
        program_id: program,
    };
    let proposal = base
        .propose_matrix_requirements_context(
            &owner_context,
            &ProposeMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                expected_context_revision: 0,
                patches: vec![
                    DeclaredRequirementValue::Mode(EngineeringMode::Demo),
                    DeclaredRequirementValue::Intent(EngineeringIntent::Other("demo".into())),
                    DeclaredRequirementValue::Urgency("ordinary".into()),
                    DeclaredRequirementValue::PromisedBehavior("demo".into()),
                    DeclaredRequirementValue::PromisedProof("check".into()),
                    DeclaredRequirementValue::NoDemandCommitment,
                    DeclaredRequirementValue::NoLatencyCommitment,
                ]
                .into_iter()
                .map(|value| RequirementDeclarationPatch::Set { value })
                .collect(),
            },
        )
        .await
        .unwrap();
    base.confirm_matrix_requirements_context(
        &owner_context,
        &ConfirmMatrixRequirementsContext {
            request_id: Uuid::new_v4(),
            locator: locator.clone(),
            proposal_revision: 1,
            proposal_digest: proposal.proposal.digest().into(),
            owner_response_ref: "SYNTHETIC CONTROL DECLARATION CONFIRMATION".into(),
        },
    )
    .await
    .unwrap();
    let mut raw = serde_json::to_value(input::matrix_input()).unwrap();
    for field in [
        "mode",
        "intent",
        "urgency",
        "promised_behavior",
        "promised_proof",
        "demand_commitment",
        "latency_commitment",
    ] {
        raw[field] = json!({"state":"absent"});
    }
    let source = base
        .record_matrix_task_with_requirements(
            &owner_context,
            &RecordMatrixTask {
                task_id: task,
                revision: 1,
                expected_current_revision: 0,
                request_id: Uuid::new_v4(),
                input: serde_json::from_value(raw).unwrap(),
                choice_set: Some(EngineeringChoiceSet {
                    schema: MATRIX_CHOICE_SET_SCHEMA.into(),
                    choice_set_id: format!("control-{task}"),
                    version: 1,
                    task_id: task.to_string(),
                    task_revision: "1".into(),
                    decision_question: "Which delivery mechanism?".into(),
                    candidates: ["reuse", "separate"]
                        .into_iter()
                        .map(|id| EngineeringCandidate {
                            candidate_id: id.into(),
                            title: id.into(),
                            approach: format!("{id} mechanism"),
                            assumption_fact_ids: vec!["criticality".into()],
                        })
                        .collect(),
                }),
            },
            &locator,
        )
        .await
        .unwrap();
    let observed = now() - 1;
    let operating_id = Uuid::new_v4();
    let sourced = |fact: Value| json!({"fact":fact,"source_ref":"SYNTHETIC OPERATING CONTROL OBSERVATION","observed_at":observed,"expires_at":observed+3600});
    let saved = serde_json::to_value(&source.revision.input).unwrap();
    let body=serde_json::to_string(&json!({"schema":"tect.matrix-operating-evidence/1","workspace_id":workspace,"task_id":task,"task_revision":1,
        "scale":sourced(saved["envelope"]["scale"].clone()),"operational_facts":sourced(saved["envelope"]["operational_facts"].clone()),
        "criticality":sourced(saved["criticality"].clone()),"affected_guarantees":sourced(saved["affected_guarantees"].clone()),"actual_exposure":sourced(saved["actual_exposure"].clone()),"urgent_repair":sourced(saved["urgent_repair"].clone()),
        })).unwrap();
    artifact(
        &admin_pool,
        owner.tenant_id,
        workspace,
        operating_id,
        &body,
        "application/vnd.tect.matrix-operating-evidence+json;version=1",
    )
    .await;
    let operating = ApprovedMatrixEvidenceArtifact {
        tenant_id: owner.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        artifact_id: operating_id,
        artifact_revision: 1,
        sha256: sha(body.as_bytes()),
        policy_version: "synthetic-control-operating/1".into(),
        max_age_seconds: 3600,
    };
    let effective = base
        .get_effective_matrix_requirements_context(&owner_context, &locator)
        .await
        .unwrap();
    let evidence = required_matrix_operating_facts(&effective, &source.revision.input)
        .unwrap()
        .into_iter()
        .map(|f| MatrixEvidenceReference {
            fact_path: f.path,
            evidence_ref: format!("pipeline-evidence:{operating_id}@1"),
        })
        .collect();
    let verify = service(&runtime, &operating, vec![])
        .verify_matrix_task(
            &verifier_context,
            &VerifyMatrixTask {
                task_id: task,
                expected_revision: 1,
                input_digest: source.revision.input_digest.clone(),
                evidence,
            },
        )
        .await
        .unwrap();
    let VerifiedMatrixTask::Context(verification) = verify else {
        panic!("V2 verification required")
    };
    let binding = ServerTechnicalDecisionTaskBinding {
        tenant_id: owner.tenant_id,
        workspace_id: workspace,
        task_id: task,
        task_revision: 1,
        operating_verification_digest: verification.digest.clone(),
        operating_policy_version: verification.policy_version,
        requirements_binding: source.requirements_binding.clone().unwrap(),
        choice_set: source.revision.choice_set.clone().unwrap(),
        choice_set_digest: source.revision.choice_set_digest.clone().unwrap(),
        recorded_by_principal_id: owner.principal_id,
    };
    // Existing independent verifier principal stands in as the separately
    // delegated technical approver in this synthetic control configuration.
    let delegated = verifier.principal_id;
    assert_ne!(delegated, owner.principal_id);
    let (approved, technical_body) = control_approval(binding, delegated, observed);
    artifact(
        &admin_pool,
        owner.tenant_id,
        workspace,
        approved.reference.artifact_id,
        &technical_body,
        TECHNICAL_EVIDENCE_FORMAT,
    )
    .await;
    let request = CompareTechnicalDeliveryMechanisms {
        task_id: task,
        expected_task_revision: 1,
        operating_verification_digest: verification.digest,
        evidence_reference: approved.reference.clone(),
    };
    ControlFixture {
        admin_pool,
        runtime,
        owner,
        workspace,
        program,
        task,
        sessions,
        owner_context,
        compare_context,
        locator,
        operating,
        approved,
        technical_body,
        request,
        delegated,
    }
}

#[tokio::test]
#[ignore = "requires exact disposable PG18.6 identity; synthetic approval CONTROL FIXTURE only"]
async fn technical_decision_comparison_pg_real_service_control_fixture() {
    let ControlFixture {
        admin_pool,
        runtime,
        owner,
        workspace,
        program,
        task,
        sessions,
        owner_context,
        compare_context,
        locator,
        operating,
        approved,
        technical_body,
        request,
        delegated,
    } = control_fixture().await;
    let guards = Arc::new(NoExternal);
    let base = WorkspaceService::new(
        Arc::new(PgStore::from_pool(runtime.clone())),
        guards.clone(),
        guards,
    );
    let whitelist = control_whitelist_json(&approved);
    let parsed_controls = parse_technical_decision_approvals(&whitelist).unwrap();
    assert_eq!(parsed_controls.len(), 1);
    let mut forged_config: Value = serde_json::from_str(&whitelist).unwrap();
    forged_config[0]["accepted"] = json!(true);
    assert!(matches!(
        parse_technical_decision_approvals(&serde_json::to_string(&forged_config).unwrap()),
        Err(Error::InvalidConfiguration)
    ));
    let trusted = service(&runtime, &operating, parsed_controls);
    let before = read_effect_counts(&admin_pool, owner.tenant_id).await;
    let TechnicalDeliveryMechanismRead::Compared(compared) = trusted
        .compare_technical_delivery_mechanisms(&compare_context, &request)
        .await
        .unwrap()
    else {
        panic!("current approved control must compare")
    };
    assert_eq!(compared.eligible_approach_ids, vec!["reuse"]);
    assert!(compared.needs_inspection.is_empty());
    assert_eq!(
        compared.assessments[0].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        compared.assessments[1].source_support,
        TechnicalSourceSupport::Supported
    );
    assert_eq!(
        compared.assessments[0].engineering_adequacy,
        TechnicalAdequacy::Adequate
    );
    assert_eq!(
        compared.assessments[1].engineering_adequacy,
        TechnicalAdequacy::Dominated
    );
    assert_eq!(
        read_effect_counts(&admin_pool, owner.tenant_id).await,
        before,
        "successful read creates no persisted effect"
    );
    assert_eq!(
        base.compare_technical_delivery_mechanisms(&compare_context, &request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    assert_eq!(
        service(&runtime, &operating, vec![])
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    for mutation in 0..7 {
        let mut wrong = approved.clone();
        match mutation {
            0 => wrong.approval.owner_author_principal_id = delegated,
            1 => wrong.approval.recorded_by_principal_id = delegated,
            2 => wrong.binding.tenant_id = Uuid::new_v4(),
            3 => wrong.binding.workspace_id = Uuid::new_v4(),
            4 => wrong.binding.task_revision = 2,
            5 => wrong.reference.artifact_version = 2,
            6 => wrong.candidate_mapping[0].frozen_candidate.title = "forged".into(),
            _ => unreachable!(),
        }
        assert_eq!(
            service(&runtime, &operating, vec![wrong])
                .compare_technical_delivery_mechanisms(&compare_context, &request)
                .await
                .unwrap(),
            TechnicalDeliveryMechanismRead::Unavailable,
            "wrong control approval {mutation}"
        );
    }
    // Untrusted artifact fields never supply accepted validation or approval.
    // Each body receives its own synthetic whitelist SHA so parsing/binding
    // checks, rather than an absent whitelist entry, exercise the real resolver.
    for mutation in 0..9 {
        let mut value: Value = serde_json::from_str(&technical_body).unwrap();
        match mutation {
            0 => value["accepted"] = json!(true),
            1 => value["tenant_id"] = json!(Uuid::new_v4()),
            2 => value["workspace_id"] = json!(Uuid::new_v4()),
            3 => value["task_id"] = json!(Uuid::new_v4()),
            4 => value["task_revision"] = json!(2),
            5 => value["candidate_mapping"][0]["frozen_candidate"]["title"] = json!("forged"),
            6 => value["facts"][0]["observed_at"] = json!(now() + 3600),
            7 => value["facts"][0]["expires_at"] = json!(now() - 1),
            8 => {}
            _ => unreachable!(),
        }
        let mutated = if mutation == 8 {
            format!(
                "{{\"schema\":\"{}\",{}",
                TECHNICAL_EVIDENCE_SCHEMA,
                &technical_body[1..]
            )
        } else {
            serde_json::to_string(&value).unwrap()
        };
        let mut control = approved.clone();
        control.reference.artifact_id = Uuid::new_v4();
        control.reference.content_sha256 = sha(mutated.as_bytes());
        artifact(
            &admin_pool,
            owner.tenant_id,
            workspace,
            control.reference.artifact_id,
            &mutated,
            TECHNICAL_EVIDENCE_FORMAT,
        )
        .await;
        let mut mutated_request = request.clone();
        mutated_request.evidence_reference = control.reference.clone();
        let result = service(&runtime, &operating, vec![control])
            .compare_technical_delivery_mechanisms(&compare_context, &mutated_request)
            .await;
        if matches!(mutation, 0 | 8) {
            assert_eq!(result, Err(Error::Forbidden), "artifact shape {mutation}");
        } else {
            assert_eq!(
                result,
                Ok(TechnicalDeliveryMechanismRead::Unavailable),
                "artifact binding/time {mutation}"
            );
            // SQLx Drop queues rollback for resolver errors; synchronize the
            // next independent negative test on actual lock release readiness.
            declaration_lock_released(&admin_pool, owner.tenant_id, workspace, program).await;
        }
    }
    // Ready metadata and an approved digest cannot authenticate changed bytes.
    let mut wrong_bytes = approved.clone();
    wrong_bytes.reference.artifact_id = Uuid::new_v4();
    artifact(
        &admin_pool,
        owner.tenant_id,
        workspace,
        wrong_bytes.reference.artifact_id,
        "{}",
        TECHNICAL_EVIDENCE_FORMAT,
    )
    .await;
    let mut byte_request = request.clone();
    byte_request.evidence_reference = wrong_bytes.reference.clone();
    assert_eq!(
        service(&runtime, &operating, vec![wrong_bytes])
            .compare_technical_delivery_mechanisms(&compare_context, &byte_request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    let mut wrong_operating = operating.clone();
    wrong_operating.sha256 = "f".repeat(64);
    assert_eq!(
        service(&runtime, &wrong_operating, vec![approved.clone()])
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await
            .unwrap(),
        TechnicalDeliveryMechanismRead::Unavailable
    );
    let mut stale = request.clone();
    stale.expected_task_revision = 2;
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&compare_context, &stale)
            .await,
        Err(Error::StaleRevision)
    );
    let mut missing = compare_context.clone();
    missing.native_session_id = Uuid::new_v4().to_string();
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&missing, &request)
            .await,
        Err(Error::WorkspaceNotOpen)
    );
    let mut foreign = compare_context.clone();
    foreign.workspace_key = format!("foreign-{}", Uuid::new_v4());
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&foreign, &request)
            .await,
        Err(Error::SessionWorkspaceMismatch)
    );
    sqlx::query("UPDATE agent_sessions SET revoked=true WHERE id=$1")
        .bind(sessions[1])
        .execute(&admin_pool)
        .await
        .unwrap();
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await,
        Err(Error::SessionRevoked)
    );
    sqlx::query("UPDATE agent_sessions SET revoked=false WHERE id=$1")
        .bind(sessions[1])
        .execute(&admin_pool)
        .await
        .unwrap();

    // Deterministic held locks exercise public comparison, then fresh retry.
    for held_task in [true, false] {
        let store = PgStore::from_pool(runtime.clone());
        let mut holder = store.begin(TransactionMode::ReadWrite).await.unwrap();
        holder.authenticate(&owner.auth).await.unwrap();
        holder.set_tenant(owner.tenant_id).await.unwrap();
        if held_task {
            holder
                .lock_matrix_task(workspace, task)
                .await
                .unwrap()
                .unwrap();
        } else {
            holder
                .matrix_requirements_context_store()
                .unwrap()
                .lock_matrix_requirements_head(
                    workspace,
                    RequirementsAnchor::Program {
                        program_id: program,
                    },
                )
                .await
                .unwrap();
        }
        assert_eq!(
            tokio::time::timeout(
                Duration::from_secs(2),
                trusted.compare_technical_delivery_mechanisms(&compare_context, &request)
            )
            .await
            .unwrap(),
            Err(Error::StaleRevision)
        );
        // Losing comparison must release the other lock before holder finishes.
        if held_task {
            holder
                .matrix_requirements_context_store()
                .unwrap()
                .lock_matrix_requirements_head(
                    workspace,
                    RequirementsAnchor::Program {
                        program_id: program,
                    },
                )
                .await
                .unwrap();
        } else {
            holder
                .lock_matrix_task(workspace, task)
                .await
                .unwrap()
                .unwrap();
        }
        holder.commit().await.unwrap();
        assert!(matches!(
            trusted
                .compare_technical_delivery_mechanisms(&compare_context, &request)
                .await
                .unwrap(),
            TechnicalDeliveryMechanismRead::Compared(_)
        ));
    }
    // A subsequent accepted declaration makes the old frozen source stale.
    let changed = base
        .propose_matrix_requirements_context(
            &owner_context,
            &ProposeMatrixRequirementsContext {
                request_id: Uuid::new_v4(),
                locator: locator.clone(),
                expected_context_revision: 1,
                patches: vec![RequirementDeclarationPatch::Set {
                    value: DeclaredRequirementValue::PromisedProof("changed proof".into()),
                }],
            },
        )
        .await
        .unwrap();
    base.confirm_matrix_requirements_context(
        &owner_context,
        &ConfirmMatrixRequirementsContext {
            request_id: Uuid::new_v4(),
            locator,
            proposal_revision: 2,
            proposal_digest: changed.proposal.digest().into(),
            owner_response_ref: "SYNTHETIC CHANGED DECLARATION CONTROL".into(),
        },
    )
    .await
    .unwrap();
    assert_eq!(
        trusted
            .compare_technical_delivery_mechanisms(&compare_context, &request)
            .await,
        Err(Error::StaleRevision)
    );
    let counts:(i64,i64,i64)=sqlx::query_as("SELECT (SELECT count(*) FROM advisory_dispatch WHERE tenant_id=$1),(SELECT count(*) FROM advisory_opportunity WHERE tenant_id=$1),(SELECT count(*) FROM matrix_task_revisions WHERE tenant_id=$1)")
        .bind(owner.tenant_id).fetch_one(&admin_pool).await.unwrap();
    assert_eq!(counts, (0, 0, 1));
    assert_eq!(
        sqlx::query_scalar::<_, i64>("SELECT count(*) FROM matrix_tasks WHERE id=$1")
            .bind(task)
            .fetch_one(&runtime)
            .await
            .unwrap(),
        0
    );
    eprintln!(
        "S02 real authenticated PG service/operating validator/technical resolver positive, control delegated authority, denials, public task/declaration contention and stale declaration proof passed; synthetic controls are NOT genuine owner acceptance"
    );
}
