use super::{SingleOperation, Value, json};

pub(super) fn revalidation(source: Value) -> SingleOperation {
    SingleOperation {
        operation: "revalidate",
        unit_id: None,
        expected_revision: Some(1),
        expected_lifecycle: Some("active"),
        document: None,
        revalidation: Some(json!({"sources":[source.clone()],
            "evidence_basis":"Fresh exact publication observation for the unchanged fixture declaration.",
            "valid_until":"2030-09-14T09:00:00Z","review_due_at":"2027-09-14T09:00:00Z"})),
        successor: None,
        replacement_bindings: json!([]),
        sources: json!([source]),
        knowledge_kind: json!("constraint"),
        profiles: json!(["general"]),
        erasure: "not_required",
        authored_followup: false,
    }
}
