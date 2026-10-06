//! Strict private approval configuration decoding and binding validation.
use super::{
    ApprovedTechnicalDecisionEvidence, TechnicalEvidenceCandidateMapping, TechnicalEvidenceLocator,
    TechnicalEvidenceRequirements, sha256, strict_json, text, valid_approval,
};
use serde::{Deserialize, Serialize};
use std::collections::BTreeSet;
use tect_application::{
    MatrixRequirementsLocator, ServerTechnicalDecisionTaskBinding, TechnicalDecisionApprovalRecord,
    TechnicalDecisionCandidateMapping, TechnicalDecisionEvidenceReference,
};
use tect_domain::{Error, Result};
use uuid::Uuid;

/// Parse only an operator-owned private configuration, never artifact content
/// or tool arguments. Duplicate keys, unknown fields and invalid pins deny.
pub fn parse_technical_decision_approvals(
    body: &str,
) -> Result<Vec<ApprovedTechnicalDecisionEvidence>> {
    let original = strict_json::parse(body).map_err(|_| Error::InvalidConfiguration)?;
    let metadata: Vec<ApprovalConfiguration> =
        serde_json::from_value(original.clone()).map_err(|_| Error::InvalidConfiguration)?;
    if metadata.len() > 256
        || serde_json::to_value(&metadata).map_err(|_| Error::InvalidConfiguration)? != original
    {
        return Err(Error::InvalidConfiguration);
    }
    let mut keys = BTreeSet::new();
    metadata
        .into_iter()
        .map(|m| {
            let approval = m.into_approval();
            if !valid_approval(&approval)
                || !valid_config_binding(&approval)
                || !keys.insert((
                    approval.binding.tenant_id,
                    approval.binding.workspace_id,
                    approval.binding.task_id,
                    approval.binding.task_revision,
                    approval.reference.artifact_id,
                    approval.reference.artifact_version,
                ))
            {
                return Err(Error::InvalidConfiguration);
            }
            Ok(approval)
        })
        .collect()
}

fn valid_config_binding(a: &ApprovedTechnicalDecisionEvidence) -> bool {
    let b = &a.binding;
    let r = &b.requirements_binding;
    let valid_locator = match &r.locator {
        MatrixRequirementsLocator::Program { program_id } => !program_id.is_nil(),
        MatrixRequirementsLocator::Scope {
            program_id,
            scope_id,
        } => !program_id.is_nil() && !scope_id.is_nil(),
        MatrixRequirementsLocator::Slice {
            program_id,
            scope_id,
            candidate_set_id,
            work_candidate_id,
            expected_work_revision,
        } => {
            [program_id, scope_id, candidate_set_id, work_candidate_id]
                .iter()
                .all(|id| !id.is_nil())
                && *expected_work_revision > 0
        }
        MatrixRequirementsLocator::OpenedSlice { slice_id } => !slice_id.is_nil(),
    };
    let mut ids = BTreeSet::new();
    let mut kinds = BTreeSet::new();
    valid_locator
        && !r.snapshot_id.is_nil()
        && sha256(&r.semantic_digest)
        && text(&r.authority_schema, 256)
        && text(&b.operating_policy_version, 256)
        && b.choice_set.schema == tect_domain::MATRIX_CHOICE_SET_SCHEMA
        && b.choice_set.version > 0
        && text(&b.choice_set.choice_set_id, 256)
        && text(&b.choice_set.decision_question, 4096)
        && a.approval.claim.approved_at >= 0
        && a.candidate_mapping.iter().all(|m| {
            let c = &m.frozen_candidate;
            let t = &m.technical_approach;
            ids.insert(c.candidate_id.clone())
                && kinds.insert(t.kind)
                && b.choice_set.candidates.contains(c)
                && text(&c.candidate_id, 256)
                && text(&c.title, 4096)
                && text(&c.approach, 65536)
                && c.candidate_id == t.id
                && c.title == t.title
                && c.approach == t.mechanism
                && !t.operational_consequences.is_empty()
                && t.operational_consequences.iter().all(|s| text(s, 4096))
        })
}

#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalConfiguration {
    binding: BindingConfiguration,
    reference: ReferenceConfiguration,
    approval: ApprovalRecordConfiguration,
    candidate_mapping: Vec<TechnicalEvidenceCandidateMapping>,
    validator_policy_version: String,
    max_age_seconds: i64,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct BindingConfiguration {
    tenant_id: Uuid,
    workspace_id: Uuid,
    task_id: Uuid,
    task_revision: i64,
    operating_verification_digest: String,
    operating_policy_version: String,
    requirements_binding: TechnicalEvidenceRequirements,
    choice_set: tect_domain::EngineeringChoiceSet,
    choice_set_digest: String,
    recorded_by_principal_id: Uuid,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ReferenceConfiguration {
    artifact_id: Uuid,
    artifact_version: i64,
    content_sha256: String,
}
#[derive(Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
struct ApprovalRecordConfiguration {
    claim: tect_domain::TechnicalOwnerApprovalClaim,
    card_digest: String,
    candidate_digest: String,
    choice_set_digest: String,
    recorded_by_principal_id: Uuid,
    owner_author_principal_id: Uuid,
    owner_authorship_ref: String,
}
impl ApprovalConfiguration {
    fn into_approval(self) -> ApprovedTechnicalDecisionEvidence {
        let b = self.binding;
        let p = self.approval;
        let r = b.requirements_binding;
        let locator = match r.locator {
            TechnicalEvidenceLocator::Program { program_id } => {
                MatrixRequirementsLocator::Program { program_id }
            }
            TechnicalEvidenceLocator::Scope {
                program_id,
                scope_id,
            } => MatrixRequirementsLocator::Scope {
                program_id,
                scope_id,
            },
            TechnicalEvidenceLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            } => MatrixRequirementsLocator::Slice {
                program_id,
                scope_id,
                candidate_set_id,
                work_candidate_id,
                expected_work_revision,
            },
            TechnicalEvidenceLocator::OpenedSlice { slice_id } => {
                MatrixRequirementsLocator::OpenedSlice { slice_id }
            }
        };
        ApprovedTechnicalDecisionEvidence {
            binding: ServerTechnicalDecisionTaskBinding {
                tenant_id: b.tenant_id,
                workspace_id: b.workspace_id,
                task_id: b.task_id,
                task_revision: b.task_revision,
                operating_verification_digest: b.operating_verification_digest,
                operating_policy_version: b.operating_policy_version,
                requirements_binding: tect_application::MatrixTaskRequirementsBinding {
                    locator,
                    snapshot_id: r.snapshot_id,
                    semantic_digest: r.semantic_digest,
                    authority_schema: r.authority_schema,
                },
                choice_set: b.choice_set,
                choice_set_digest: b.choice_set_digest,
                recorded_by_principal_id: b.recorded_by_principal_id,
            },
            reference: TechnicalDecisionEvidenceReference {
                artifact_id: self.reference.artifact_id,
                artifact_version: self.reference.artifact_version,
                content_sha256: self.reference.content_sha256,
            },
            approval: TechnicalDecisionApprovalRecord {
                claim: p.claim,
                card_digest: p.card_digest,
                candidate_digest: p.candidate_digest,
                choice_set_digest: p.choice_set_digest,
                recorded_by_principal_id: p.recorded_by_principal_id,
                owner_author_principal_id: p.owner_author_principal_id,
                owner_authorship_ref: p.owner_authorship_ref,
            },
            candidate_mapping: self
                .candidate_mapping
                .into_iter()
                .map(|m| TechnicalDecisionCandidateMapping {
                    frozen_candidate: m.frozen_candidate,
                    technical_approach: m.technical_approach,
                })
                .collect(),
            validator_policy_version: self.validator_policy_version,
            max_age_seconds: self.max_age_seconds,
        }
    }
}
