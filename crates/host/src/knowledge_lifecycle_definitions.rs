use serde::Deserialize;
use sha2::{Digest, Sha256};
use tect_application::KnowledgeLifecycleDefinitionProvider;
use tect_domain::*;

const REGISTRY_VERSION: &str = "0.2.0-dk2.1";
const DEFINITION_VERSION: &str = "0.3.0-dk3.1";
const ROOT: &str = "crates/host/knowledge-methods/";

pub(crate) struct StaticKnowledgeLifecycleDefinitions;

impl KnowledgeLifecycleDefinitionProvider for StaticKnowledgeLifecycleDefinitions {
    fn definition(&self) -> Result<KnowledgeChangeDefinition> {
        let registry = self.registry()?;
        let overview = snapshot_version(
            "tect:knowledge-change:overview",
            DEFINITION_VERSION,
            "overview-dk3.md",
            include_str!("../knowledge-methods/overview-dk3.md"),
        );
        let profile_methods: Vec<_> = registry
            .profiles
            .iter()
            .flat_map(|p| p.methods.clone())
            .collect();
        let mut phases = Vec::new();
        for (index, id) in KnowledgeChangePhaseId::ALL.into_iter().enumerate() {
            let phase_method = phase_method(id);
            let instructions = phase_method.clone().into_iter().collect();
            let methods = if id.agent_authored() {
                phase_method
                    .into_iter()
                    .chain(
                        matches!(id.ordinal(), 4..=8)
                            .then_some(profile_methods.clone())
                            .into_iter()
                            .flatten(),
                    )
                    .collect()
            } else {
                Vec::new()
            };
            let previous = index
                .checked_sub(1)
                .map(|i| vec![KnowledgeChangePhaseId::ALL[i]])
                .unwrap_or_default();
            let backward = if (3..=10).contains(&id.ordinal())
                || id == KnowledgeChangePhaseId::KcResultHandoff
            {
                KnowledgeChangePhaseId::ALL
                    .into_iter()
                    .filter(|target| {
                        (2..=7).contains(&target.ordinal()) && target.ordinal() < id.ordinal()
                    })
                    .collect()
            } else {
                Vec::new()
            };
            let contract = source_contract_ref(
                &format!("tect:knowledge-change:{}:output", id.as_str()),
                "crates/domain/src/knowledge_lifecycle_execution.rs",
                include_str!("../../domain/src/knowledge_lifecycle_execution.rs"),
            );
            phases.push(KnowledgeChangePhaseDefinition {
                id,
                ordinal: id.ordinal(),
                title: phase_title(id).into(),
                executor: executor(id),
                output_kind: output_kind(id),
                depends_on: previous,
                allowed_backward_to: backward,
                instructions,
                methods,
                required_input_refs: if index == 0 {
                    vec!["begin_request".into()]
                } else {
                    vec![KnowledgeChangePhaseId::ALL[index - 1].as_str().into()]
                },
                required_output_refs: vec![id.as_str().into()],
                required_obligation_ids: Vec::new(),
                output_contract_ref: contract,
            });
        }
        let mut value = KnowledgeChangeDefinition {
            version: DEFINITION_VERSION.into(),
            digest: String::new(),
            registry_version: registry.version.clone(),
            registry_digest: registry.digest.clone(),
            overview,
            default_mode: PipelineDeliveryMode::Whole,
            allowed_modes: vec![PipelineDeliveryMode::Whole, PipelineDeliveryMode::Phasewise],
            phases,
            completion_contract_ref: contract_ref_version(
                "tect:knowledge-change:completion",
                DEFINITION_VERSION,
                "overview-dk3.md",
                include_str!("../knowledge-methods/overview-dk3.md"),
            ),
            escalation_contract_ref: contract_ref_version(
                "tect:knowledge-change:escalation",
                DEFINITION_VERSION,
                "overview-dk3.md",
                include_str!("../knowledge-methods/overview-dk3.md"),
            ),
        };
        value.digest = material_digest(&value)?;
        Ok(value)
    }

    fn registry(&self) -> Result<KnowledgeProfileRegistry> {
        let source: ObligationFile = serde_json::from_str(include_str!(
            "../knowledge-methods/profile-obligations.json"
        ))
        .map_err(|_| Error::InvalidConfiguration)?;
        if source.version != REGISTRY_VERSION {
            return Err(Error::InvalidConfiguration);
        }
        let mut profiles = Vec::new();
        for profile in source.profiles {
            let id = profile_id(&profile.id)?;
            let body = profile_body(&profile.method_file)?;
            let method = snapshot(
                &format!("tect:knowledge-profile:{}", profile.id),
                &profile.method_file,
                body,
            );
            let method_ref = contract_ref(&method.id, &profile.method_file, body);
            let mut obligations: Vec<_> = profile
                .obligations
                .into_iter()
                .map(|o| KnowledgeProfileObligationDefinition {
                    id: o.id,
                    requirement: o.requirement,
                    phase_id: KnowledgeChangePhaseId::KcDomainChecks,
                    applicability: applicability(&o.operations),
                    required: true,
                    depends_on: Vec::new(),
                    method_refs: vec![method_ref.clone()],
                    shape_refs: Vec::new(),
                    required_outputs: vec!["obligation_receipt".into()],
                    reuse_rule_ref: method_ref.clone(),
                    terminal_rule_refs: vec![method_ref.clone()],
                })
                .collect();
            obligations.sort_by(|a, b| a.id.cmp(&b.id));
            let mut value = KnowledgeProfileContract {
                profile_id: id,
                version: REGISTRY_VERSION.into(),
                digest: String::new(),
                applicable_kinds: applicable_kinds(id),
                operations: all_operations(),
                inherits: if id == KnowledgeProfileId::General {
                    Vec::new()
                } else {
                    vec![KnowledgeProfileId::General]
                },
                compatible_profiles: all_profiles().into_iter().filter(|p| *p != id).collect(),
                shape_refs: Vec::new(),
                methods: vec![method],
                obligations,
                evidence_rule_refs: vec![method_ref.clone()],
                freshness_rule_refs: vec![method_ref.clone()],
                authority_rule_refs: vec![method_ref.clone()],
                impact_rule_refs: vec![method_ref.clone()],
                retention_rule_refs: vec![method_ref.clone()],
                index_rule_refs: vec![method_ref.clone()],
                terminal_rule_refs: vec![method_ref],
                lifecycle_complete: true,
            };
            value.digest = material_digest(&value)?;
            profiles.push(value);
        }
        let mut registry = KnowledgeProfileRegistry {
            version: REGISTRY_VERSION.into(),
            digest: String::new(),
            profiles,
        };
        registry.digest = material_digest(&registry)?;
        Ok(registry)
    }
}

#[derive(Deserialize)]
struct ObligationFile {
    version: String,
    profiles: Vec<ObligationProfile>,
}
#[derive(Deserialize)]
struct ObligationProfile {
    id: String,
    method_file: String,
    obligations: Vec<Obligation>,
}
#[derive(Deserialize)]
struct Obligation {
    id: String,
    operations: Vec<KnowledgeLifecycleOperation>,
    requirement: String,
}

fn snapshot(id: &str, file: &str, body: &str) -> PipelineInstructionSnapshot {
    snapshot_version(id, REGISTRY_VERSION, file, body)
}
fn snapshot_version(
    id: &str,
    version: &str,
    file: &str,
    body: &str,
) -> PipelineInstructionSnapshot {
    PipelineInstructionSnapshot {
        id: id.into(),
        version: version.into(),
        digest: digest(body.as_bytes()),
        body: body.into(),
        origin_refs: vec![format!("{ROOT}{file}")],
    }
}
fn contract_ref(id: &str, file: &str, body: &str) -> KnowledgeContractRef {
    contract_ref_version(id, REGISTRY_VERSION, file, body)
}
fn contract_ref_version(id: &str, version: &str, file: &str, body: &str) -> KnowledgeContractRef {
    KnowledgeContractRef {
        id: id.into(),
        version: version.into(),
        digest: digest(body.as_bytes()),
        source_ref: format!("{ROOT}{file}"),
    }
}
fn source_contract_ref(id: &str, source_ref: &str, body: &str) -> KnowledgeContractRef {
    KnowledgeContractRef {
        id: id.into(),
        version: REGISTRY_VERSION.into(),
        digest: digest(body.as_bytes()),
        source_ref: source_ref.into(),
    }
}
fn material_digest<T: serde::Serialize + Clone>(value: &T) -> Result<String> {
    let mut json = serde_json::to_value(value).map_err(|_| Error::InvalidConfiguration)?;
    json["digest"] = serde_json::Value::String(String::new());
    Ok(digest(
        &serde_json::to_vec(&json).map_err(|_| Error::InvalidConfiguration)?,
    ))
}
fn digest(bytes: &[u8]) -> String {
    Sha256::digest(bytes)
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect()
}
fn applicability(ops: &[KnowledgeLifecycleOperation]) -> KnowledgeObligationApplicability {
    if ops.len() == 1 {
        KnowledgeObligationApplicability::Operation { operation: ops[0] }
    } else {
        KnowledgeObligationApplicability::Operations {
            operations: ops.to_vec(),
        }
    }
}
fn all_operations() -> Vec<KnowledgeLifecycleOperation> {
    use KnowledgeLifecycleOperation::*;
    vec![Create, Revise, Revalidate, Supersede, Retract, Erase]
}
fn all_profiles() -> Vec<KnowledgeProfileId> {
    use KnowledgeProfileId::*;
    vec![
        General,
        Runbook,
        Protocol,
        Devops,
        Operations,
        ProductResearch,
        Security,
    ]
}
fn profile_id(id: &str) -> Result<KnowledgeProfileId> {
    use KnowledgeProfileId::*;
    match id {
        "general" => Ok(General),
        "runbook" => Ok(Runbook),
        "protocol" => Ok(Protocol),
        "devops" => Ok(Devops),
        "operations" => Ok(Operations),
        "product_research" => Ok(ProductResearch),
        "security" => Ok(Security),
        _ => Err(Error::InvalidConfiguration),
    }
}
fn applicable_kinds(_id: KnowledgeProfileId) -> Vec<KnowledgeKind> {
    use KnowledgeKind::*;
    vec![
        Constraint,
        Claim,
        Decision,
        Hypothesis,
        Procedure,
        Protocol,
        Infrastructure,
        OperatingModel,
        ProductResearch,
        Security,
    ]
}
fn profile_body(file: &str) -> Result<&'static str> {
    match file {
        "profile-general.md" => Ok(include_str!("../knowledge-methods/profile-general.md")),
        "profile-runbook.md" => Ok(include_str!("../knowledge-methods/profile-runbook.md")),
        "profile-protocol.md" => Ok(include_str!("../knowledge-methods/profile-protocol.md")),
        "profile-devops.md" => Ok(include_str!("../knowledge-methods/profile-devops.md")),
        "profile-operations.md" => Ok(include_str!("../knowledge-methods/profile-operations.md")),
        "profile-product-research.md" => Ok(include_str!(
            "../knowledge-methods/profile-product-research.md"
        )),
        "profile-security.md" => Ok(include_str!("../knowledge-methods/profile-security.md")),
        _ => Err(Error::InvalidConfiguration),
    }
}
fn phase_method(id: KnowledgeChangePhaseId) -> Option<PipelineInstructionSnapshot> {
    let file = format!("{}.md", id.as_str());
    id.method_id().map(|mid| {
        snapshot_version(
            mid,
            DEFINITION_VERSION,
            &file,
            match id {
                KnowledgeChangePhaseId::KcIntake => {
                    include_str!("../knowledge-methods/kc-intake.md")
                }
                KnowledgeChangePhaseId::KcResolveBaseline => {
                    include_str!("../knowledge-methods/kc-resolve-baseline.md")
                }
                KnowledgeChangePhaseId::KcQualifyPlan => {
                    include_str!("../knowledge-methods/kc-qualify-plan.md")
                }
                KnowledgeChangePhaseId::KcQualifyEvidence => {
                    include_str!("../knowledge-methods/kc-qualify-evidence.md")
                }
                KnowledgeChangePhaseId::KcPrepareChange => {
                    include_str!("../knowledge-methods/kc-prepare-change.md")
                }
                KnowledgeChangePhaseId::KcDomainChecks => {
                    include_str!("../knowledge-methods/kc-domain-checks.md")
                }
                KnowledgeChangePhaseId::KcImpactPlan => {
                    include_str!("../knowledge-methods/kc-impact-plan.md")
                }
                KnowledgeChangePhaseId::KcReviewReconcile => {
                    include_str!("../knowledge-methods/kc-review-reconcile.md")
                }
                KnowledgeChangePhaseId::KcResultHandoff => {
                    include_str!("../knowledge-methods/kc-result-handoff.md")
                }
                _ => unreachable!(),
            },
        )
    })
}
fn executor(id: KnowledgeChangePhaseId) -> KnowledgePhaseExecutor {
    match id {
        KnowledgeChangePhaseId::KcPublicationGate => KnowledgePhaseExecutor::Backend,
        KnowledgeChangePhaseId::KcCommit => KnowledgePhaseExecutor::Publisher,
        KnowledgeChangePhaseId::KcSettleEffects => KnowledgePhaseExecutor::Backend,
        _ => KnowledgePhaseExecutor::Agent,
    }
}
fn output_kind(id: KnowledgeChangePhaseId) -> KnowledgePhaseOutputKind {
    use KnowledgeChangePhaseId::*;
    match id {
        KcIntake => KnowledgePhaseOutputKind::ChangeIntent,
        KcResolveBaseline => KnowledgePhaseOutputKind::BaselineManifest,
        KcQualifyPlan => KnowledgePhaseOutputKind::BranchPlan,
        KcQualifyEvidence => KnowledgePhaseOutputKind::EvidenceManifest,
        KcPrepareChange => KnowledgePhaseOutputKind::ProposedChangeset,
        KcDomainChecks => KnowledgePhaseOutputKind::ObligationReceipts,
        KcImpactPlan => KnowledgePhaseOutputKind::ImpactPlan,
        KcReviewReconcile => KnowledgePhaseOutputKind::ReviewReceipt,
        KcPublicationGate => KnowledgePhaseOutputKind::ReadyToCommit,
        KcCommit => KnowledgePhaseOutputKind::PublisherReceipt,
        KcSettleEffects => KnowledgePhaseOutputKind::EffectsReport,
        KcResultHandoff => KnowledgePhaseOutputKind::ChangeResult,
    }
}
fn phase_title(id: KnowledgeChangePhaseId) -> &'static str {
    match id {
        KnowledgeChangePhaseId::KcIntake => "Intake",
        KnowledgeChangePhaseId::KcResolveBaseline => "Resolve baseline",
        KnowledgeChangePhaseId::KcQualifyPlan => "Qualify plan",
        KnowledgeChangePhaseId::KcQualifyEvidence => "Qualify evidence",
        KnowledgeChangePhaseId::KcPrepareChange => "Prepare change",
        KnowledgeChangePhaseId::KcDomainChecks => "Domain checks",
        KnowledgeChangePhaseId::KcImpactPlan => "Impact plan",
        KnowledgeChangePhaseId::KcReviewReconcile => "Review and reconcile",
        KnowledgeChangePhaseId::KcPublicationGate => "Publication gate",
        KnowledgeChangePhaseId::KcCommit => "Commit",
        KnowledgeChangePhaseId::KcSettleEffects => "Settle effects",
        KnowledgeChangePhaseId::KcResultHandoff => "Result handoff",
    }
}

#[cfg(test)]
#[path = "knowledge_lifecycle_definitions/tests.rs"]
mod tests;
