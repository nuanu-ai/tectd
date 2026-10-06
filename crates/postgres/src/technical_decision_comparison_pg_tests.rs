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

#[path = "technical_decision_comparison_pg_tests/positive_input.rs"]
mod input;

#[path = "technical_decision_comparison_pg_tests/isolated_pg.rs"]
pub(super) mod isolated_pg;

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

include!("technical_decision_comparison_pg_tests/fixture.rs");
include!("technical_decision_comparison_pg_tests/cases.rs");

#[path = "technical_decision_comparison_pg_tests/host_wire.rs"]
mod host_wire;
