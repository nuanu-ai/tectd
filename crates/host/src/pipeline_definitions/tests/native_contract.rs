use super::*;

#[test]
fn full_engineering_two_remains_byte_identical_and_selectable() {
    let raw = include_str!(
        "../../../pipeline-definitions/full-design-to-execution-0.6.0-native.engineering.2.json"
    );
    assert_eq!(
        hex(&Sha256::digest(raw.as_bytes())),
        "f08130bdf758bce57c52c69c1af865dfe8c069c69beff6bee2373113195db9e2"
    );
    let old = StaticPipelineDefinitions
        .definition_for(
            PipelineKind::FullDesignToExecution,
            Some("0.6.0-native.engineering.2"),
        )
        .unwrap();
    assert_eq!(
        old.digest,
        "1274c531dfd433bf01e6b2354adcd0082c906749e1c8e34a158604f77e77a9a5"
    );
    assert!(
        old.phases[3]
            .required_fields
            .iter()
            .any(|f| f == "member_id")
    );
    assert!(
        !old.phases
            .iter()
            .flat_map(|p| &p.resources)
            .any(|r| r.id == "tect:native-slice-work-contract-schema")
    );
    let current = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    assert_eq!(current.version, "0.6.0-native.engineering.4");
    assert_ne!(old.digest, current.digest);
    assert!(
        StaticPipelineDefinitions
            .definition_for(
                PipelineKind::FullDesignToExecution,
                Some("unavailable-version")
            )
            .is_err()
    );
}

#[test]
fn native_schema_is_real_closed_json_and_pinned_to_writer_and_consumers() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    let schema_source = include_str!(concat!(
        env!("CARGO_MANIFEST_DIR"),
        "/../../skills/pipelines/full-design-to-execution/native-slice-work-contract-engineering.4.schema.json"
    ));
    let schema: serde_json::Value = serde_json::from_str(schema_source).unwrap();
    assert_eq!(schema["additionalProperties"], false);
    assert_eq!(
        schema["properties"]["contract_kind"]["const"],
        "native_slice_work_contract_v1"
    );
    assert_eq!(
        schema["properties"]["authority"]["properties"]["scope"]["items"]["properties"]["action"]["enum"],
        serde_json::json!(["source_plan", "source_edit", "source_test"])
    );
    for forbidden in ["member_id", "principal_id", "route_target_ref"] {
        assert!(schema["properties"].get(forbidden).is_none());
    }
    for ordinal in [4, 10, 11, 13] {
        let phase = &definition.phases[ordinal - 1];
        let resource = phase
            .resources
            .iter()
            .find(|r| r.id == "tect:native-slice-work-contract-schema")
            .unwrap();
        assert_eq!(resource.body, schema_source);
        assert_eq!(
            resource.digest,
            hex(&Sha256::digest(schema_source.as_bytes()))
        );
        assert_eq!(resource.version, "1.1.0");
        assert!(phase.output_contract.contains("before"));
    }
    let writer = &definition.phases[3];
    assert!(!writer.required_fields.iter().any(|f| matches!(
        f.as_str(),
        "member_id" | "route_target_ref" | "template_exact_keys"
    )));
    let requirement = &writer.required_artifacts[0];
    let resource = writer
        .resources
        .iter()
        .find(|r| r.id == "tect:native-slice-work-contract-schema")
        .unwrap();
    assert_eq!(requirement.name_pattern, "work-order-contract.json");
    assert_eq!(
        requirement.schema_resource_id.as_deref(),
        Some(resource.id.as_str())
    );
    assert_eq!(
        requirement.schema_resource_digest.as_deref(),
        Some(resource.digest.as_str())
    );
    assert!(
        requirement
            .schema_ref
            .as_ref()
            .unwrap()
            .contains(&resource.digest)
    );
}

#[test]
fn native4_digest_material_matches_published_digest() {
    let raw = include_str!("../../../pipeline-definitions/full-design-to-execution.json");
    let mut definition: PipelineDefinitionSnapshot = serde_json::from_str(raw).unwrap();
    let published = std::mem::take(&mut definition.digest);
    let actual = hex(&Sha256::digest(serde_json::to_vec(&definition).unwrap()));
    assert_eq!(published, actual);
}

#[test]
fn full_engineering_three_is_immutable_selectable_and_other_phases_are_preserved() {
    let raw = include_str!(
        "../../../pipeline-definitions/full-design-to-execution-0.6.0-native.engineering.3.json"
    );
    assert_eq!(
        hex(&Sha256::digest(raw.as_bytes())),
        "fb2cfe631cd0110ff33c12f6226031859c20c334646666347950ae79f9e834ba"
    );
    let old = StaticPipelineDefinitions
        .definition_for(
            PipelineKind::FullDesignToExecution,
            Some("0.6.0-native.engineering.3"),
        )
        .unwrap();
    assert_eq!(
        old.digest,
        "79c01395855e0be1ffb4fca6eec7a09a5326a44d64ad3aa545c1e1da7d829ff3"
    );
    let current = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    for (old_phase, new_phase) in old.phases.iter().zip(&current.phases) {
        assert_eq!(old_phase.id, new_phase.id);
        assert_eq!(old_phase.ordinal, new_phase.ordinal);
        if ![4, 10, 11, 13, 16].contains(&old_phase.ordinal) {
            assert_eq!(old_phase, new_phase);
        }
    }
    let old_schema = old.phases[3]
        .resources
        .iter()
        .find(|r| r.id == "tect:native-slice-work-contract-schema")
        .unwrap();
    assert_eq!(old_schema.version, "1.0.0");
    assert_eq!(
        old_schema.digest,
        "9222bcaf40113fa0563015d2d2829b3980d89e95dc0649b6118f9d16982ae258"
    );
    assert_eq!(
        old_schema.body,
        include_str!(
            "../../../../../skills/pipelines/full-design-to-execution/native-slice-work-contract.schema.json"
        )
    );
    for version in [
        "0.1.0-native.1",
        "0.4.0-native.skills.1",
        "0.6.0-native.engineering.5",
    ] {
        assert!(
            StaticPipelineDefinitions
                .definition_for(PipelineKind::FullDesignToExecution, Some(version))
                .is_err()
        );
    }
}

#[test]
fn local_result_has_an_honest_typed_route_and_rejects_contradictions() {
    let definition = StaticPipelineDefinitions
        .definition(PipelineKind::FullDesignToExecution)
        .unwrap();
    let phase = &definition.phases[15];
    let mut fields = phase
        .required_fields
        .iter()
        .map(|k| (k.clone(), "Explicit synthetic local fixture".to_string()))
        .collect::<std::collections::BTreeMap<_, _>>();
    for (key, value) in [
        ("route", "result_local_only"),
        ("highest_validated_truth", "local_verified"),
        ("terminal_state_candidate", "completed_local_verified"),
        ("deployment_required", "false"),
        ("disposition", "proof_gate"),
    ] {
        fields.insert(key.into(), value.into());
    }
    let body = "Synthetic local validation fixture";
    let request:tect_domain::CompletePipelinePhase=serde_json::from_value(serde_json::json!({"request_id":uuid::Uuid::new_v4(),"run_id":uuid::Uuid::new_v4(),"run_revision":16,"phase_id":phase.id,"outcome":"completed","transition":"continue","output":{"body":body,"producer_context_id":"synthetic-local-gate","fields":fields,"verdict":"completed_local_verified","dispositions":["proof_gate"],"artifacts":[{"name":"deployment-validation.md","media_type":"text/markdown","body":body,"digest":hex(&Sha256::digest(body.as_bytes()))}]}})).unwrap();
    request.validate(&definition).unwrap();
    for (key, value) in [
        ("route", "user_handoff"),
        ("highest_validated_truth", "live_verified"),
        ("terminal_state_candidate", "completed_deploy_verified"),
        ("deployment_required", "true"),
    ] {
        let mut bad = request.clone();
        bad.output.fields.insert(key.into(), value.into());
        assert!(bad.validate(&definition).is_err(), "{key}");
    }
    let mut handoff = request.clone();
    handoff.output.verdict = Some("handoff_required".into());
    handoff.output.dispositions = vec!["handoff_selected".into()];
    assert!(handoff.validate(&definition).is_err());
    handoff
        .output
        .fields
        .insert("route".into(), "user_handoff".into());
    handoff.validate(&definition).unwrap();
}
