use super::*;

#[test]
fn amendment_every_original_predicate_has_field_specific_rule_without_secret_echo() {
    let valid: PipelineSourceAmendment = serde_json::from_value(amendment()).unwrap();
    assert!(validate_source_amendment(&valid).is_ok());
    for (pointer, changed, suffix) in [
        ("/target_phase_id", json!(" "), "TARGET-PHASE"),
        (
            "/predecessor/output_id",
            json!(Uuid::nil()),
            "PREDECESSOR-OUTPUT-ID",
        ),
        (
            "/predecessor/output_revision",
            json!(0),
            "PREDECESSOR-REVISION",
        ),
        (
            "/predecessor/output_digest",
            json!(" "),
            "PREDECESSOR-OUTPUT-DIGEST-BLANK",
        ),
        (
            "/predecessor/output_digest",
            json!("x".repeat(129)),
            "PREDECESSOR-OUTPUT-DIGEST-LIMIT",
        ),
        (
            "/predecessor/artifact_name",
            json!(" "),
            "PREDECESSOR-ARTIFACT-NAME-BLANK",
        ),
        (
            "/predecessor/artifact_name",
            json!("x".repeat(MAX_SOURCE_PATH_BYTES + 1)),
            "PREDECESSOR-ARTIFACT-NAME-LIMIT",
        ),
        (
            "/predecessor/artifact_digest",
            json!(" "),
            "PREDECESSOR-ARTIFACT-DIGEST-BLANK",
        ),
        (
            "/predecessor/artifact_digest",
            json!("x".repeat(129)),
            "PREDECESSOR-ARTIFACT-DIGEST-LIMIT",
        ),
        (
            "/predecessor/source_path",
            json!("../秘密.md"),
            "PREDECESSOR-PATH",
        ),
        (
            "/predecessor/source_digest",
            json!(" "),
            "PREDECESSOR-SOURCE-DIGEST-BLANK",
        ),
        (
            "/predecessor/source_digest",
            json!("x".repeat(129)),
            "PREDECESSOR-SOURCE-DIGEST-LIMIT",
        ),
        ("/successor/path", json!("../秘密.md"), "SUCCESSOR-PATH"),
        (
            "/successor/artifact/name",
            json!("../秘密.md"),
            "ARTIFACT-PATH",
        ),
        (
            "/successor/artifact/name",
            json!("other.md"),
            "PATH-NAME-MATCH",
        ),
        ("/successor/artifact/media_type", json!(" bad "), "MEDIA"),
        ("/successor/artifact/body", json!(" "), "BODY-BLANK"),
        (
            "/successor/artifact/body",
            json!("秘密".repeat(MAX_PIPELINE_OUTPUT_BYTES / 6 + 1)),
            "BODY-SIZE",
        ),
        (
            "/successor/artifact/digest",
            json!("a".repeat(63)),
            "DIGEST-SIZE",
        ),
        (
            "/successor/artifact/digest",
            json!("A".repeat(64)),
            "DIGEST-HEX",
        ),
        (
            "/successor/artifact/reference",
            json!(" "),
            "REFERENCE-BLANK",
        ),
        (
            "/successor/artifact/reference",
            json!("x".repeat(MAX_SOURCE_PATH_BYTES + 1)),
            "REFERENCE-SIZE",
        ),
        (
            "/authorization_scope",
            json!(" "),
            "AUTHORIZATION-SCOPE-BLANK",
        ),
        (
            "/authorization_scope",
            json!("SECRET".repeat(MAX_PIPELINE_INPUT_BYTES / 6 + 1)),
            "AUTHORIZATION-SCOPE-SIZE",
        ),
        (
            "/authorization_provenance",
            json!(" "),
            "AUTHORIZATION-PROVENANCE-BLANK",
        ),
        (
            "/authorization_provenance",
            json!("SECRET".repeat(MAX_PIPELINE_INPUT_BYTES / 6 + 1)),
            "AUTHORIZATION-PROVENANCE-SIZE",
        ),
    ] {
        let mut value = amendment();
        set(&mut value, pointer, changed);
        let error = validate_source_amendment(&serde_json::from_value(value).unwrap()).unwrap_err();
        let serialized = serde_json::to_string(&error).unwrap();
        assert!(!serialized.contains("秘密"));
        assert!(!serialized.contains("SECRET"));
        assert_failure(
            Err(error),
            &format!("WP6-SOURCE-AMENDMENT-{suffix}"),
            &format!(
                "arguments.params.source_amendment{}",
                pointer.replace('/', ".")
            ),
        );
    }
}
#[test]
fn amendment_limits_hex_path_media_and_record_priority_keep_existing_acceptance() {
    let mut value = amendment();
    value["predecessor"]["output_digest"] = json!("z".repeat(128));
    value["predecessor"]["artifact_digest"] = json!("z".repeat(128));
    value["predecessor"]["source_digest"] = json!("z".repeat(128));
    value["predecessor"]["artifact_name"] = json!("/".repeat(MAX_SOURCE_PATH_BYTES));
    value["successor"]["artifact"]["reference"] = json!("/".repeat(MAX_SOURCE_PATH_BYTES));
    value["successor"]["artifact"]["body"] = json!("x".repeat(MAX_PIPELINE_OUTPUT_BYTES));
    value["authorization_scope"] = json!("x".repeat(MAX_PIPELINE_INPUT_BYTES));
    value["authorization_provenance"] = json!("x".repeat(MAX_PIPELINE_INPUT_BYTES));
    assert!(validate_source_amendment(&serde_json::from_value(value).unwrap()).is_ok());
    for path in ["a.md", "a/b.md", "ユニコード.md"] {
        let mut value = amendment();
        value["successor"]["path"] = json!(path);
        value["successor"]["artifact"]["name"] = json!(path);
        assert!(validate_source_amendment(&serde_json::from_value(value).unwrap()).is_ok());
    }
    for media in ["text/plain", "application/x+json", "a/b/c"] {
        let mut value = amendment();
        value["successor"]["artifact"]["media_type"] = json!(media);
        assert!(validate_source_amendment(&serde_json::from_value(value).unwrap()).is_ok());
    }
    let mut value = amendment();
    value["target_phase_id"] = json!("");
    value["predecessor"]["output_id"] = json!(Uuid::nil());
    assert_failure(
        validate_source_amendment(&serde_json::from_value(value).unwrap()),
        "WP6-SOURCE-AMENDMENT-TARGET-PHASE",
        "arguments.params.source_amendment.target_phase_id",
    );
    let mut value = input();
    value["request_id"] = json!(Uuid::nil());
    value["source_amendment"] = amendment();
    value["source_amendment"]["target_phase_id"] = json!("");
    assert_failure(
        serde_json::from_value::<RecordPipelineInput>(value)
            .unwrap()
            .validate(),
        "WP6-INPUT-REQUEST-ID",
        "arguments.params.request_id",
    );
}

#[test]
fn amendment_path_media_and_utf8_boundaries_preserve_existing_validation() {
    for (pointer, suffix) in [
        ("/predecessor/source_path", "PREDECESSOR-PATH"),
        ("/successor/path", "SUCCESSOR-PATH"),
        ("/successor/artifact/name", "ARTIFACT-PATH"),
    ] {
        for invalid in [
            String::new(),
            "a\\b".into(),
            "a\0b".into(),
            "a//b".into(),
            "./a".into(),
            "a/../b".into(),
            "/a".into(),
            " a".into(),
            "a ".into(),
            "a".repeat(MAX_SOURCE_PATH_BYTES + 1),
        ] {
            let mut value = amendment();
            set(&mut value, pointer, json!(invalid));
            assert_failure(
                validate_source_amendment(&serde_json::from_value(value).unwrap()),
                &format!("WP6-SOURCE-AMENDMENT-{suffix}"),
                &format!(
                    "arguments.params.source_amendment{}",
                    pointer.replace('/', ".")
                ),
            );
        }
    }
    let mut value = amendment();
    let path = "a".repeat(MAX_SOURCE_PATH_BYTES);
    value["predecessor"]["source_path"] = json!(path);
    value["successor"]["path"] = json!(path);
    value["successor"]["artifact"]["name"] = json!(path);
    value["successor"]["artifact"]["media_type"] = json!(format!("a/{}", "b".repeat(253)));
    assert!(validate_source_amendment(&serde_json::from_value(value.clone()).unwrap()).is_ok());
    value["successor"]["artifact"]["media_type"] = json!(format!("a/{}", "b".repeat(254)));
    assert_failure(
        validate_source_amendment(&serde_json::from_value(value).unwrap()),
        "WP6-SOURCE-AMENDMENT-MEDIA",
        "arguments.params.source_amendment.successor.artifact.media_type",
    );
    let mut value = input();
    value["input"] = json!(format!("{}x", "界".repeat(21845)));
    assert!(
        serde_json::from_value::<RecordPipelineInput>(value.clone())
            .unwrap()
            .validate()
            .is_ok()
    );
    value["input"] = json!(format!("{}xx", "界".repeat(21845)));
    assert_failure(
        serde_json::from_value::<RecordPipelineInput>(value)
            .unwrap()
            .validate(),
        "WP6-INPUT-SIZE",
        "arguments.params.input",
    );
    assert_eq!(phase_ordinal(u32::MAX as usize - 1), Ok(u32::MAX));
    assert_failure(
        phase_ordinal(u32::MAX as usize).map(|_| ()),
        "WP6-PHASE-ORDINAL-RANGE",
        &format!("pipeline_definition.phases[{}].ordinal", u32::MAX),
    );
}
