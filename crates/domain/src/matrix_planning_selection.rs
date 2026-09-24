use crate::{Error, Result};
use serde::{Deserialize, Serialize};
use uuid::Uuid;

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct MatrixPlanningSelection {
    pub task_id: Uuid,
    pub task_revision: i64,
    pub disposition_id: Uuid,
    pub selected_choice_id: String,
    pub expected_input_digest: String,
    pub expected_choice_set_digest: String,
    pub expected_verification_digest: String,
    /// Ordered positions in the submitted native draft attributed to this choice.
    pub mapped_draft_node_indices: Vec<usize>,
}

impl MatrixPlanningSelection {
    pub fn validate(&self) -> Result<()> {
        if self.task_id.is_nil()
            || self.task_revision < 1
            || self.disposition_id.is_nil()
            || self.selected_choice_id.is_empty()
            || self.selected_choice_id.len() > 4096
            || self.selected_choice_id.trim() != self.selected_choice_id
            || self.selected_choice_id.contains('\0')
            || !valid_digest(&self.expected_input_digest)
            || !valid_digest(&self.expected_choice_set_digest)
            || !valid_digest(&self.expected_verification_digest)
            || self.mapped_draft_node_indices.is_empty()
            || self.mapped_draft_node_indices.len() > 100
            || self
                .mapped_draft_node_indices
                .windows(2)
                .any(|pair| pair[0] >= pair[1])
        {
            return Err(Error::InvalidArguments);
        }
        Ok(())
    }
}

fn valid_digest(value: &str) -> bool {
    value.len() == 64
        && value
            .bytes()
            .all(|b| b.is_ascii_hexdigit() && !b.is_ascii_uppercase())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn selected_planning_binding_requires_exact_nonempty_ids_and_digests() {
        let selection = MatrixPlanningSelection {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            disposition_id: Uuid::new_v4(),
            selected_choice_id: "choice-a".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![0, 2],
        };
        assert_eq!(selection.validate(), Ok(()));
        let mut invalid = selection.clone();
        invalid.task_revision = 0;
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
        invalid = selection.clone();
        invalid.selected_choice_id = " choice-a".into();
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
        invalid = selection;
        invalid.expected_verification_digest = "C".repeat(64);
        assert_eq!(invalid.validate(), Err(Error::InvalidArguments));
    }

    #[test]
    fn mapped_indices_are_nonempty_bounded_and_unique_in_order() {
        let mut selection = MatrixPlanningSelection {
            task_id: Uuid::new_v4(),
            task_revision: 1,
            disposition_id: Uuid::new_v4(),
            selected_choice_id: "choice-a".into(),
            expected_input_digest: "a".repeat(64),
            expected_choice_set_digest: "b".repeat(64),
            expected_verification_digest: "c".repeat(64),
            mapped_draft_node_indices: vec![],
        };
        assert_eq!(selection.validate(), Err(Error::InvalidArguments));
        selection.mapped_draft_node_indices = vec![1, 1];
        assert_eq!(selection.validate(), Err(Error::InvalidArguments));
        selection.mapped_draft_node_indices = vec![2, 1];
        assert_eq!(selection.validate(), Err(Error::InvalidArguments));
        selection.mapped_draft_node_indices = (0..101).collect();
        assert_eq!(selection.validate(), Err(Error::InvalidArguments));
        selection.mapped_draft_node_indices = vec![0, 2];
        assert_eq!(selection.validate(), Ok(()));
        let mut encoded = serde_json::to_value(&selection).unwrap();
        encoded
            .as_object_mut()
            .unwrap()
            .remove("mapped_draft_node_indices");
        assert!(serde_json::from_value::<MatrixPlanningSelection>(encoded).is_err());
    }
}
