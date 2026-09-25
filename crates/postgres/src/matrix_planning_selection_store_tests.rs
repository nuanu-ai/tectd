use super::*;

fn link(node_id: Uuid) -> MatrixPlanningSelectionLink {
    MatrixPlanningSelectionLink {
        selection: MatrixPlanningSelection {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            disposition_id: Uuid::new_v4(),
            selected_choice_id: "chosen".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![1],
        },
        evaluation_digest: "d".repeat(64),
        catalogue_version: "v1".into(),
        caller_principal_id: Uuid::new_v4(),
        caller_session_id: Uuid::new_v4(),
        scope_id: Uuid::new_v4(),
        candidate_set_id: Uuid::new_v4(),
        caller_request_id: Uuid::new_v4(),
        result_revision: 2,
        mapped_nodes: vec![MatrixPlanningMappedNode {
            draft_index: 1,
            node_id,
            node_revision: 1,
        }],
    }
}

#[test]
fn mapped_node_must_match_exact_receipt_index_kind_identity_and_result() {
    let node_id = Uuid::new_v4();
    let binding = link(node_id);
    let request = serde_json::json!({"draft": {"nodes": [
        {"kind": "work", "identity": {"local": "other"}},
        {"kind": "decision", "identity": {"local": "selected"}}
    ]}});
    let mut result = serde_json::json!({"draft": {"nodes": [
        {"kind": "work", "id": Uuid::new_v4(), "revision": 1},
        {"kind": "decision", "id": node_id, "revision": 1}
    ]}});
    assert_eq!(verify_mapped_nodes(&request, &result, &binding), Ok(()));
    assert_eq!(
        decode_mapped_nodes(Some(mapped_nodes_json(&binding.mapped_nodes).unwrap())).unwrap(),
        binding.mapped_nodes
    );
    result["draft"]["nodes"][1]["id"] = serde_json::json!(Uuid::new_v4());
    assert_eq!(
        verify_mapped_nodes(&request, &result, &binding),
        Err(Error::InputConflict)
    );
    result["draft"]["nodes"][1]["id"] = serde_json::json!(node_id);
    result["draft"]["nodes"][1]["kind"] = serde_json::json!("work");
    assert_eq!(
        verify_mapped_nodes(&request, &result, &binding),
        Err(Error::InputConflict)
    );
    let mut wrong_index = binding.clone();
    wrong_index.mapped_nodes[0].draft_index = 0;
    assert_eq!(
        verify_mapped_nodes(&request, &result, &wrong_index),
        Err(Error::InputConflict)
    );
}
