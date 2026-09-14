use serde::Deserialize;
use sha2::{Digest, Sha256};
use tect_application::KnowledgeLifecycleDefinitionProvider;
use tect_domain::*;

const VERSION: &str = "0.2.0-dk2.1";
const ROOT: &str = "crates/host/knowledge-methods/";

pub(crate) struct StaticKnowledgeLifecycleDefinitions;

impl KnowledgeLifecycleDefinitionProvider for StaticKnowledgeLifecycleDefinitions {
    fn definition(&self) -> Result<KnowledgeChangeDefinition> {
        let registry = self.registry()?;
        let overview = snapshot(
            "tect:knowledge-change:overview",
            "overview.md",
            include_str!("../knowledge-methods/overview.md"),
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
            version: VERSION.into(),
            digest: String::new(),
            registry_version: registry.version.clone(),
            registry_digest: registry.digest.clone(),
            overview,
            default_mode: PipelineDeliveryMode::Whole,
            allowed_modes: vec![PipelineDeliveryMode::Whole, PipelineDeliveryMode::Phasewise],
            phases,
            completion_contract_ref: contract_ref(
                "tect:knowledge-change:completion",
                "overview.md",
                include_str!("../knowledge-methods/overview.md"),
            ),
            escalation_contract_ref: contract_ref(
                "tect:knowledge-change:escalation",
                "overview.md",
                include_str!("../knowledge-methods/overview.md"),
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
        if source.version != VERSION {
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
                version: VERSION.into(),
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
            version: VERSION.into(),
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
    PipelineInstructionSnapshot {
        id: id.into(),
        version: VERSION.into(),
        digest: digest(body.as_bytes()),
        body: body.into(),
        origin_refs: vec![format!("{ROOT}{file}")],
    }
}
fn contract_ref(id: &str, file: &str, body: &str) -> KnowledgeContractRef {
    KnowledgeContractRef {
        id: id.into(),
        version: VERSION.into(),
        digest: digest(body.as_bytes()),
        source_ref: format!("{ROOT}{file}"),
    }
}
fn source_contract_ref(id: &str, source_ref: &str, body: &str) -> KnowledgeContractRef {
    KnowledgeContractRef {
        id: id.into(),
        version: VERSION.into(),
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
        snapshot(
            mid,
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
mod tests {
    use super::*;

    #[test]
    fn registry_pins_seven_profiles_and_49_operation_obligations() {
        let registry = StaticKnowledgeLifecycleDefinitions.registry().unwrap();
        registry.validate().unwrap();
        assert_eq!(registry.version, VERSION);
        assert_eq!(registry.profiles.len(), 7);
        assert_eq!(
            registry
                .profiles
                .iter()
                .map(|p| p.obligations.len())
                .sum::<usize>(),
            49
        );
        for profile in &registry.profiles {
            assert_eq!(profile.operations, all_operations());
            assert!(profile.lifecycle_complete);
            let method = &profile.methods[0];
            assert_eq!(method.id, profile.profile_id.method_id());
            assert_eq!(method.digest, digest(method.body.as_bytes()));
            assert!(method.origin_refs[0].starts_with(ROOT));
            assert!(
                profile
                    .obligations
                    .iter()
                    .all(|obligation| obligation.required
                        && obligation.phase_id == KnowledgeChangePhaseId::KcDomainChecks)
            );
        }
        for profile in [
            KnowledgeProfileId::Runbook,
            KnowledgeProfileId::Devops,
            KnowledgeProfileId::Security,
        ] {
            let contract = registry
                .profiles
                .iter()
                .find(|item| item.profile_id == profile)
                .unwrap();
            assert!(
                contract
                    .applicable_kinds
                    .contains(&KnowledgeKind::Procedure)
            );
        }
    }

    #[test]
    fn definition_pins_all_twelve_phases_and_composed_methods() {
        let definition = StaticKnowledgeLifecycleDefinitions.definition().unwrap();
        definition.validate().unwrap();
        assert_eq!(definition.phases.len(), 12);
        for (index, phase) in definition.phases.iter().enumerate() {
            assert_eq!(phase.id, KnowledgeChangePhaseId::ALL[index]);
            assert_eq!(
                phase.depends_on,
                index
                    .checked_sub(1)
                    .map(|i| vec![KnowledgeChangePhaseId::ALL[i]])
                    .unwrap_or_default()
            );
            if phase.id.agent_authored() {
                assert!(
                    phase
                        .methods
                        .iter()
                        .any(|method| Some(method.id.as_str()) == phase.id.method_id())
                );
            } else {
                assert!(phase.methods.is_empty());
            }
            if matches!(phase.ordinal, 4..=8) {
                assert_eq!(phase.methods.len(), 8);
            }
        }
        assert!(
            definition
                .phases
                .iter()
                .all(|phase| phase.required_obligation_ids.is_empty())
        );
        assert!(
            definition
                .phases
                .iter()
                .all(|phase| phase.output_contract_ref.source_ref
                    == "crates/domain/src/knowledge_lifecycle_execution.rs")
        );
        assert_eq!(
            definition.phases[8].executor,
            KnowledgePhaseExecutor::Backend
        );
        assert_eq!(
            definition.phases[9].executor,
            KnowledgePhaseExecutor::Publisher
        );
        assert_eq!(
            definition.phases[10].executor,
            KnowledgePhaseExecutor::Backend
        );
    }
}
