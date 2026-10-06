use super::*;

pub(super) fn historical_lightweight() -> PipelineDefinitionSnapshot {
    load(
        include_str!("fixtures/lightweight-tdd.json"),
        PipelineKind::LightweightTddDevelopment,
    )
    .unwrap()
}

#[test]
fn retired_lightweight_is_historical_only_and_all_production_selectors_are_compact() {
    use tect_domain::{
        ensure_pipeline_definition_selectable, ensure_pipeline_run_mutable, is_retired_lightweight,
    };
    let archived = historical_lightweight();
    assert!(is_retired_lightweight(&archived));
    assert_eq!(archived.version, "0.6.0-native.engineering.2");
    assert_eq!(
        hex(&Sha256::digest(include_bytes!(
            "fixtures/lightweight-tdd.json"
        ))),
        "bc7968632aba4aefe6ec39b516dfc362d6db7c0025847a1070f2fc31300122e3"
    );
    assert!(ensure_pipeline_definition_selectable(&archived).is_err());
    for status in [
        "active",
        "waiting_input",
        "blocked",
        "completed",
        "escalated",
        "superseded",
    ] {
        assert!(ensure_pipeline_run_mutable(&archived, status).is_err());
    }
    for selector in [None, Some("0.7.0-native.k1k5"), Some("0.7.1-native.k1k5")] {
        let definition = StaticPipelineDefinitions
            .definition_for(PipelineKind::LightweightTddDevelopment, selector)
            .unwrap();
        assert_eq!(definition.phases.len(), 5);
        assert!(!is_retired_lightweight(&definition));
        assert!(ensure_pipeline_run_mutable(&definition, "active").is_ok());
        assert!(ensure_pipeline_run_mutable(&definition, "superseded").is_err());
    }
    for selector in [
        "0.6.0-native.engineering.2",
        "0.4.0-native.skills.1",
        "0.1.0-native.1",
        "unknown",
    ] {
        assert!(
            StaticPipelineDefinitions
                .definition_for(PipelineKind::LightweightTddDevelopment, Some(selector))
                .is_err()
        );
    }
    let full = StaticPipelineDefinitions
        .definition_for(
            PipelineKind::FullDesignToExecution,
            Some("0.6.0-native.engineering.2"),
        )
        .unwrap();
    assert!(!is_retired_lightweight(&full));
    let mut disguised = lightweight_v07().unwrap();
    disguised.version = "unrecognized-snapshot".into();
    disguised.phases = vec![disguised.phases[0].clone(); 15];
    assert!(is_retired_lightweight(&disguised));
}

#[test]
fn retirement_restart_allows_empty_mappings_only_for_actual_retired_current_snapshots() {
    let old = historical_lightweight();
    let current = lightweight_v07().unwrap();
    let request = tect_domain::PipelineRunMigrationRequest {
        request_id: uuid::Uuid::new_v4(),
        predecessor_run_id: uuid::Uuid::new_v4(),
        predecessor_definition_version: old.version.clone(),
        predecessor_definition_digest: old.digest.clone(),
        successor_definition_version: current.version.clone(),
        successor_definition_digest: current.digest.clone(),
        mappings: vec![],
    };
    let command = tect_domain::PipelineRunMigrationCommand {
        request_id: request.request_id,
        predecessor_run_id: request.predecessor_run_id,
        expected_revision: 1,
        idempotency_key: "retirement-shape-only".into(),
        successor_definition_version: current.version.clone(),
        mappings: vec![],
    };
    assert!(command.validate().is_ok());
    assert!(request.validate().is_err());
    assert!(
        request
            .validate_retirement_restart(&old, &current, &DefinitionDigest)
            .is_ok()
    );
    let mut mismatched = request.clone();
    mismatched.predecessor_definition_digest = "incorrect-digest".into();
    assert!(
        mismatched
            .validate_retirement_restart(&old, &current, &DefinitionDigest)
            .is_err()
    );
    assert!(
        request
            .validate_retirement_restart(&current, &current, &DefinitionDigest)
            .is_err()
    );
    assert!(
        request
            .validate_retirement_restart(&old, &lightweight_v070().unwrap(), &DefinitionDigest)
            .is_err()
    );
}

#[test]
fn retirement_successor_pin_rejects_forged_body_and_metadata() {
    use tect_domain::{
        CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST, is_current_lightweight_retirement_successor,
        pipeline_definition_digest,
    };
    let current = lightweight_v07().unwrap();
    assert_eq!(
        pipeline_definition_digest(&current, &DefinitionDigest).unwrap(),
        CURRENT_LIGHTWEIGHT_DEFINITION_DIGEST
    );
    assert!(is_current_lightweight_retirement_successor(
        &current,
        &DefinitionDigest
    ));
    let predecessor = historical_lightweight();
    let restart =
        |successor: &PipelineDefinitionSnapshot| tect_domain::PipelineRunMigrationRequest {
            request_id: uuid::Uuid::new_v4(),
            predecessor_run_id: uuid::Uuid::new_v4(),
            predecessor_definition_version: predecessor.version.clone(),
            predecessor_definition_digest: predecessor.digest.clone(),
            successor_definition_version: successor.version.clone(),
            successor_definition_digest: successor.digest.clone(),
            mappings: vec![],
        };
    assert!(
        restart(&current)
            .validate_retirement_restart(&predecessor, &current, &DefinitionDigest)
            .is_ok()
    );
    let mut arbitrary = current.clone();
    arbitrary.phases = vec![current.phases[0].clone(); 5];
    let mut altered_phase = current.clone();
    altered_phase.phases[0].instructions[0]
        .body
        .push_str(" forged obligation");
    let mut altered_contract = current.clone();
    altered_contract
        .completion_contract
        .push_str(" forged contract");
    let mut false_metadata = current.clone();
    false_metadata.digest = "f".repeat(64);
    for successor in [arbitrary, altered_phase, altered_contract, false_metadata] {
        assert!(!is_current_lightweight_retirement_successor(
            &successor,
            &DefinitionDigest
        ));
        assert!(
            restart(&successor)
                .validate_retirement_restart(&predecessor, &successor, &DefinitionDigest)
                .is_err()
        );
    }
}

#[test]
fn successor_selection_retags_retirement_provider_and_store_errors_only() {
    use tect_domain::{
        ensure_pipeline_definition_selectable, migration_successor_retirement_error,
    };
    let begin_error = StaticPipelineDefinitions
        .definition_for(
            PipelineKind::LightweightTddDevelopment,
            Some("0.6.0-native.engineering.2"),
        )
        .unwrap_err();
    let begin = begin_error.refusal().unwrap().clone();
    assert_eq!(
        begin.path.as_deref(),
        Some("arguments.params.definition_version")
    );
    assert_eq!(
        begin.next_action.as_deref(),
        Some("begin_current_lightweight_k1k5")
    );
    let provider = StaticPipelineDefinitions
        .definition_for(
            PipelineKind::LightweightTddDevelopment,
            Some("0.6.0-native.engineering.2"),
        )
        .map_err(migration_successor_retirement_error)
        .unwrap_err();
    let store = ensure_pipeline_definition_selectable(&historical_lightweight())
        .map_err(migration_successor_retirement_error)
        .unwrap_err();
    let mut expected = begin;
    expected.path = Some("arguments.params.successor_definition_version".into());
    expected.next_action = Some("get_current_context_and_use_exact_migration_action".into());
    assert_eq!(provider.refusal(), Some(expected.clone()));
    assert_eq!(store.refusal(), Some(expected));
    let unknown = StaticPipelineDefinitions
        .definition_for(PipelineKind::LightweightTddDevelopment, Some("unknown"))
        .unwrap_err();
    assert_eq!(
        migration_successor_retirement_error(unknown.clone()),
        unknown
    );
}

#[test]
fn retirement_restart_is_closed_and_preserves_generic_full_mapping() {
    use tect_domain::{
        PipelineMigrationEvidenceRef, PipelineObligationMapping, PipelineRunMigrationRequest,
    };
    let current = lightweight_v07().unwrap();
    let old = historical_lightweight();
    let mapping = PipelineObligationMapping {
        legacy_obligation_id: "private-legacy-obligation".into(),
        successor_obligation_id: "private-successor-obligation".into(),
        evidence_refs: vec![PipelineMigrationEvidenceRef {
            reference: "private-evidence-reference".into(),
            digest: "private-evidence-digest".into(),
        }],
    };
    let request = |predecessor: &PipelineDefinitionSnapshot,
                   successor: &PipelineDefinitionSnapshot| {
        PipelineRunMigrationRequest {
            request_id: uuid::Uuid::new_v4(),
            predecessor_run_id: uuid::Uuid::new_v4(),
            predecessor_definition_version: predecessor.version.clone(),
            predecessor_definition_digest: predecessor.digest.clone(),
            successor_definition_version: successor.version.clone(),
            successor_definition_digest: successor.digest.clone(),
            mappings: vec![],
        }
    };
    let assert_refusal = |error: Error, rule: &str, path: &str, actual: &str| {
        let refusal = error.refusal().unwrap();
        assert_eq!(
            refusal.code,
            tect_domain::RefusalCode::LegacyMigrationRequired
        );
        assert_eq!(refusal.rule.as_deref(), Some(rule));
        assert_eq!(refusal.path.as_deref(), Some(path));
        assert_eq!(refusal.actual.as_deref(), Some(actual));
        assert_eq!(
            refusal.expected.as_deref(),
            Some("canonical current Lightweight K1-K5 0.7.1-native.k1k5 with empty mappings")
        );
        assert_eq!(
            refusal.next_action.as_deref(),
            Some("get_current_context_and_use_exact_migration_action")
        );
        assert_eq!(
            refusal.required.as_deref(),
            Some("canonical_current_lightweight_k1k5_empty_mappings")
        );
        let text = serde_json::to_string(&refusal).unwrap();
        for protected in [
            "private-legacy-obligation",
            "private-successor-obligation",
            "private-evidence-reference",
            "private-evidence-digest",
        ] {
            assert!(!text.contains(protected));
        }
    };
    assert_eq!(
        tect_domain::pipeline_definition_digest(&current, &DefinitionDigest).unwrap(),
        current.digest
    );
    for version in [
        "0.6.0-native.engineering.2",
        "0.4.0-native.skills.1",
        "0.1.0-native.1",
        "arbitrary-fifteen-phase-version",
    ] {
        let mut predecessor = old.clone();
        predecessor.version = version.into();
        let empty = request(&predecessor, &current);
        assert!(
            empty
                .validate_retirement_restart(&predecessor, &current, &DefinitionDigest)
                .is_ok()
        );
        let mut nonempty = empty.clone();
        nonempty.mappings = vec![mapping.clone()];
        assert!(nonempty.validate().is_ok());
        assert_refusal(
            nonempty
                .validate_retirement_restart(&predecessor, &current, &DefinitionDigest)
                .unwrap_err(),
            "WP6-MIGRATION-MAPPING-01",
            "arguments.params.mappings",
            "1",
        );
    }
    let mut altered = current.clone();
    altered.phases[0].instructions[0]
        .body
        .push_str(" altered body");
    assert_eq!(altered.digest, current.digest);
    assert_ne!(
        tect_domain::pipeline_definition_digest(&altered, &DefinitionDigest).unwrap(),
        current.digest
    );
    for mappings in [vec![], vec![mapping.clone()]] {
        let mut invalid = request(&old, &altered);
        invalid.mappings = mappings;
        assert_refusal(
            invalid
                .validate_retirement_restart(&old, &altered, &DefinitionDigest)
                .unwrap_err(),
            "WP6-MIGRATION-CONTRACT-01",
            "arguments.params.successor_definition_version",
            "noncanonical retirement successor",
        );
    }
    for field in 0..5 {
        let mut invalid = request(&old, &current);
        match field {
            0 => invalid.predecessor_definition_version = "wrong-predecessor-version".into(),
            1 => invalid.predecessor_definition_digest = "wrong-predecessor-digest".into(),
            2 => invalid.successor_definition_version = "wrong-successor-version".into(),
            3 => invalid.successor_definition_digest = "wrong-successor-digest".into(),
            _ => invalid.request_id = uuid::Uuid::nil(),
        }
        assert_refusal(
            invalid
                .validate_retirement_restart(&old, &current, &DefinitionDigest)
                .unwrap_err(),
            "WP6-MIGRATION-CONTRACT-01",
            "arguments.params",
            "invalid retirement contract",
        );
    }
    let full_old = StaticPipelineDefinitions
        .definition_for(
            PipelineKind::FullDesignToExecution,
            Some("0.6.0-native.engineering.2"),
        )
        .unwrap();
    let full_current = StaticPipelineDefinitions
        .definition_for(PipelineKind::FullDesignToExecution, None)
        .unwrap();
    let mut generic = request(&full_old, &full_current);
    generic.mappings = vec![mapping];
    assert!(generic.validate().is_ok());
    assert!(
        generic
            .validate_retirement_restart(&full_old, &full_current, &DefinitionDigest)
            .is_ok()
    );
    let current_to_current = request(&current, &current);
    assert_eq!(
        current_to_current.validate_retirement_restart(&current, &current, &DefinitionDigest),
        current_to_current.validate()
    );
    assert!(current_to_current.validate().is_err());
}
