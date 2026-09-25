use crate::tools::object_schema;
use serde_json::{Value, json};

include!("slice_schema/pipeline.rs");
include!("slice_schema/scope_slice.rs");
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn draft_matrix_binding_is_optional_and_review_excludes_it() {
        let schema = save();
        let draft = &schema["oneOf"][0];
        let review = &schema["oneOf"][1];
        let binding = &draft["properties"]["matrix_selection"];
        assert_eq!(binding["additionalProperties"], false);
        assert_eq!(
            binding["properties"]["expected_input_digest"]["pattern"],
            "^[0-9a-f]{64}$"
        );
        assert_eq!(
            binding["properties"]["mapped_draft_node_indices"]["minItems"],
            1
        );
        assert_eq!(
            binding["properties"]["mapped_draft_node_indices"]["uniqueItems"],
            true
        );
        assert_eq!(
            binding["properties"]["mapped_draft_node_indices"]["description"],
            "Nonempty, strictly increasing zero-based positions in the submitted draft.nodes array."
        );
        assert_eq!(binding["required"].as_array().unwrap().len(), 8);
        assert!(
            !draft["required"]
                .as_array()
                .unwrap()
                .contains(&json!("matrix_selection"))
        );
        assert!(review["properties"].get("matrix_selection").is_none());
        assert!(save_example().get("matrix_selection").is_none());
        assert_eq!(
            save_matrix_selection_example()["matrix_selection"]["selected_choice_id"],
            "choice-a"
        );
        assert_eq!(
            save_matrix_selection_example()["matrix_selection"]["mapped_draft_node_indices"],
            json!([0])
        );
    }
}
