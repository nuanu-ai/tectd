use super::*;

#[test]
fn engineering_path_refusal_preserves_safe_indexed_metadata_on_wire() {
    let error = Error::PipelineRefused {
        source: Box::new(Error::InvalidArguments),
        refusal: Box::new(
            tect_domain::Refusal::new(tect_domain::RefusalCode::InputSchemaInvalid)
                .with_rule("ENG-REVIEW-FILE-PATH-01")
                .with_path("engineering-review.json/files/2/path")
                .with_expected("safe repository-relative path")
                .with_actual("unsafe path"),
        ),
    }
    .normalize_pipeline_refusal(
        "fallback-rule",
        "arguments.params",
        "valid input",
        "correct_input_and_retry",
        "schema_valid_input",
    );
    let value = failure(error, None);
    let body: Value = serde_json::from_str(value["content"][1]["text"].as_str().unwrap()).unwrap();
    assert_eq!(body["error"]["code"], "invalid_arguments");
    assert_eq!(body["error"]["refusal"]["code"], "INPUT_SCHEMA_INVALID");
    assert_eq!(body["error"]["refusal"]["rule"], "ENG-REVIEW-FILE-PATH-01");
    assert_eq!(
        body["error"]["refusal"]["path"],
        "engineering-review.json/files/2/path"
    );
    assert_eq!(body["error"]["refusal"]["actual"], "unsafe path");
}
