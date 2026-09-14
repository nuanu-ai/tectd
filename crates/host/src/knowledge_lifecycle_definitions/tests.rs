use super::*;

#[test]
fn registry_pins_seven_profiles_and_49_operation_obligations() {
    let registry = StaticKnowledgeLifecycleDefinitions.registry().unwrap();
    registry.validate().unwrap();
    assert_eq!(registry.version, REGISTRY_VERSION);
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
    assert_eq!(definition.version, DEFINITION_VERSION);
    assert_eq!(definition.registry_version, REGISTRY_VERSION);
    assert_eq!(definition.overview.version, DEFINITION_VERSION);
    assert_eq!(
        definition.overview.origin_refs,
        [format!("{ROOT}overview-dk4.md")]
    );
    let maintenance = maintenance_method();
    assert_eq!(maintenance.version, DEFINITION_VERSION);
    assert_eq!(maintenance.digest, digest(maintenance.body.as_bytes()));
    assert_eq!(maintenance.origin_refs, [format!("{ROOT}maintenance.md")]);
    for contract in [
        &definition.completion_contract_ref,
        &definition.escalation_contract_ref,
    ] {
        assert_eq!(contract.version, DEFINITION_VERSION);
        assert_eq!(contract.source_ref, format!("{ROOT}overview-dk4.md"));
    }
    assert_eq!(definition.phases.len(), 12);
    for (index, phase) in definition.phases.iter().enumerate() {
        assert_eq!(phase.id, KnowledgeChangePhaseId::ALL[index]);
        assert_eq!(phase.output_contract_ref.version, REGISTRY_VERSION);
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
            assert_eq!(
                phase
                    .methods
                    .iter()
                    .find(|method| Some(method.id.as_str()) == phase.id.method_id())
                    .unwrap()
                    .version,
                DEFINITION_VERSION
            );
            assert!(
                phase
                    .methods
                    .iter()
                    .filter(|method| method.id.starts_with("tect:knowledge-profile:"))
                    .all(|method| method.version == REGISTRY_VERSION)
            );
        } else {
            assert!(phase.methods.is_empty());
        }
        if matches!(phase.ordinal, 4..=8) {
            assert_eq!(
                phase.methods.len(),
                if phase.id == KnowledgeChangePhaseId::KcReviewReconcile {
                    9
                } else {
                    8
                }
            );
        }
    }
    assert!(
        definition
            .phases
            .iter()
            .all(|phase| phase.required_obligation_ids.is_empty())
    );
    for id in [
        KnowledgeChangePhaseId::KcResolveBaseline,
        KnowledgeChangePhaseId::KcReviewReconcile,
    ] {
        let phase = definition
            .phases
            .iter()
            .find(|phase| phase.id == id)
            .unwrap();
        assert!(
            phase
                .instructions
                .iter()
                .any(|method| method.id == maintenance.id)
        );
        assert!(
            phase
                .methods
                .iter()
                .any(|method| method.id == maintenance.id)
        );
    }
    assert_eq!(
        definition.phases[4].instructions[0].origin_refs,
        [format!("{ROOT}kc-prepare-change-dk4.md")]
    );
    assert_eq!(
        definition.phases[7].instructions[0].origin_refs,
        [format!("{ROOT}kc-review-reconcile-dk4.md")]
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
