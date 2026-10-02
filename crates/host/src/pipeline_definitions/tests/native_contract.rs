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
    assert_eq!(current.version, "0.6.0-native.engineering.3");
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
        "/../../skills/pipelines/full-design-to-execution/native-slice-work-contract.schema.json"
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
        assert_eq!(resource.version, "1.0.0");
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
